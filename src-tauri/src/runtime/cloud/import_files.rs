use super::*;
use crate::file_import::CopyProgress;
use std::{path::Path, sync::Arc, time::Instant};
use tokio::io::AsyncSeekExt;

const RECEIVER: &str = include_str!("import_files.py");

async fn send_archive<F>(
    mut input: tokio::fs::File,
    mut stdin: tokio::process::ChildStdin,
    archive_size: u64,
    progress: Arc<F>,
) -> Result<(), String>
where
    F: Fn(CopyProgress) + Send + Sync + 'static,
{
    let mut buffer = vec![0u8; 128 * 1024];
    let mut sent = 0u64;
    let mut last = Instant::now();
    progress(CopyProgress { phase: "copying", completed_bytes: 0, total_bytes: archive_size, scanned_entries: None });
    loop {
        let count = input.read(&mut buffer).await.map_err(|e| e.to_string())?;
        if count == 0 { break; }
        stdin.write_all(&buffer[..count]).await.map_err(|e| format!("Cloud upload interrupted: {e}"))?;
        sent += count as u64;
        if last.elapsed().as_millis() >= 150 {
            progress(CopyProgress { phase: "copying", completed_bytes: sent, total_bytes: archive_size, scanned_entries: None });
            last = Instant::now();
        }
    }
    if sent != archive_size { return Err("The local archive changed during upload".into()); }
    stdin.shutdown().await.map_err(|e| e.to_string())?;
    // The SSH receiver must see EOF, including when the tar is smaller than
    // its input buffer. Flushing alone leaves it waiting at "Finishing copy".
    drop(stdin);
    progress(CopyProgress { phase: "finishing", completed_bytes: sent, total_bytes: archive_size, scanned_entries: None });
    Ok(())
}

impl Cloud {
    pub(crate) async fn import_file_archive<F>(
        &self,
        id: &str,
        archive: &Path,
        transfer: &str,
        expected_bytes: u64,
        expected_files: usize,
        progress: Arc<F>,
    ) -> Result<String, String>
    where
        F: Fn(CopyProgress) + Send + Sync + 'static,
    {
        // A file drop must use the connected server and its pinned SSH identity.
        self.session(id).await?;
        if transfer.len() != 32 || !transfer.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err("Invalid cloud import identifier".into());
        }
        let profile = self.profile(id)?;
        validate(&profile, true)?;
        check_identity_agent(&profile.identity_file).await?;
        let archive_size = tokio::fs::metadata(archive).await.map_err(|e| e.to_string())?.len();
        let mut input = tokio::fs::File::open(archive).await.map_err(|e| e.to_string())?;
        let mut digest = Sha256::new();
        let mut buffer = vec![0u8; 128 * 1024];
        loop {
            let count = input.read(&mut buffer).await.map_err(|e| e.to_string())?;
            if count == 0 { break; }
            digest.update(&buffer[..count]);
        }
        let config = json!({
            "transferId": transfer,
            "bytes": expected_bytes,
            "files": expected_files,
            "archiveBytes": archive_size,
            "checksum": hex::encode(digest.finalize()),
        });
        let mut command = ssh_command(&profile, &self.directory(id)?.join("known_hosts"))?;
        command.arg(format!(
            "python3 -u -c \"import base64;exec(base64.b64decode('{}'))\" {}",
            B64.encode(RECEIVER), B64.encode(config.to_string())
        ))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
        let mut child = command.spawn().map_err(|e| format!("Start SSH file upload: {e}"))?;
        let stdin = child.stdin.take().ok_or("Missing SSH upload stream")?;
        let mut stdout = child.stdout.take().ok_or("Missing SSH upload response")?.take(8193);
        let mut stderr = child.stderr.take().ok_or("Missing SSH upload diagnostics")?.take(16385);
        input.rewind().await.map_err(|e| e.to_string())?;
        let result = tokio::time::timeout(Duration::from_secs(24 * 60 * 60), async {
            let send = send_archive(input, stdin, archive_size, progress);
            let read_output = async {
                let mut bytes = Vec::new();
                stdout.read_to_end(&mut bytes).await.map_err(|e| e.to_string())?;
                if bytes.len() > 8192 { return Err("Cloud upload returned too much output".into()); }
                Ok::<_, String>(String::from_utf8_lossy(&bytes).into_owned())
            };
            let read_errors = async {
                let mut bytes = Vec::new();
                stderr.read_to_end(&mut bytes).await.map_err(|e| e.to_string())?;
                if bytes.len() > 16384 { return Err("Cloud upload returned too much diagnostic output".into()); }
                Ok::<_, String>(String::from_utf8_lossy(&bytes).into_owned())
            };
            let (_, output, errors) = tokio::try_join!(send, read_output, read_errors)?;
            let status = child.wait().await.map_err(|e| e.to_string())?;
            if !status.success() {
                return Err(format!("Cloud file upload failed: {}", errors.trim()));
            }
            let receipt = output.lines().find_map(|line| line.strip_prefix("yougori-import-ready "))
                .ok_or("Cloud server did not confirm the file upload")?;
            let receipt: Value = serde_json::from_str(receipt).map_err(|_| "Invalid cloud upload receipt")?;
            let destination = receipt["destination"].as_str().ok_or("Cloud upload destination is missing")?;
            if !destination.starts_with('/') || !destination.ends_with(&format!("/yougori-import-{transfer}"))
                || destination.len() > 4096 || destination.contains(['\n', '\r', '\0'])
                || receipt["bytes"].as_u64() != Some(expected_bytes)
                || receipt["files"].as_u64() != Some(expected_files as u64) {
                return Err("Cloud server returned an inconsistent file upload receipt".into());
            }
            Ok(destination.to_owned())
        }).await.unwrap_or_else(|_| Err("Cloud file upload timed out".into()));
        if result.is_err() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn eof_receiver_fixture() {
        if std::env::var_os("YOUGORI_TEST_COPY_EOF").is_none() { return; }
        let mut bytes = Vec::new();
        std::io::stdin().read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"small archive");
    }

    #[tokio::test]
    async fn small_archive_closes_child_stdin_before_waiting_for_receipt() {
        let archive = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(archive.path(), b"small archive").unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "runtime::cloud::import_files::tests::eof_receiver_fixture"])
            .env("YOUGORI_TEST_COPY_EOF", "1")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn().unwrap();
        let input = tokio::fs::File::open(archive.path()).await.unwrap();
        let stdin = child.stdin.take().unwrap();
        tokio::time::timeout(Duration::from_secs(5), send_archive(input, stdin, 13, Arc::new(|_| {})))
            .await.unwrap().unwrap();
        let result = tokio::time::timeout(Duration::from_secs(5), child.wait_with_output()).await.unwrap().unwrap();
        assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stdout));
    }
}
