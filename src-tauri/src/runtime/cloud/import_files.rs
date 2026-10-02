use super::*;
use crate::file_import::{CopyProgress,transfers};
use tokio::io::{AsyncBufReadExt,BufReader};
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
    progress(CopyProgress { phase: "sending", completed_bytes: 0, total_bytes: archive_size, scanned_entries: None, ..Default::default() });
    loop {
        let count = input.read(&mut buffer).await.map_err(|e| e.to_string())?;
        if count == 0 { break; }
        stdin.write_all(&buffer[..count]).await.map_err(|e| format!("Cloud upload interrupted: {e}"))?;
        sent += count as u64;
        if last.elapsed().as_millis() >= 150 {
            progress(CopyProgress { phase: "sending", completed_bytes: sent, sent_bytes:Some(sent), total_bytes: archive_size, scanned_entries: None, ..Default::default() });
            last = Instant::now();
        }
    }
    if sent != archive_size { return Err("The local archive changed during upload".into()); }
    stdin.shutdown().await.map_err(|e| e.to_string())?;
    // The SSH receiver must see EOF, including when the tar is smaller than
    // its input buffer. Flushing alone leaves it waiting at "Finishing copy".
    drop(stdin);
    progress(CopyProgress { phase: "extracting", completed_bytes: sent, sent_bytes:Some(sent), total_bytes: archive_size, scanned_entries: None, ..Default::default() });
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
        let operation=transfers::find(transfer);
        let cancellation=operation.as_ref().map(|op|op.cancellation.clone()).unwrap_or_default();
        // A file drop must use the connected server and its pinned SSH identity.
        tokio::select! { value=tokio::time::timeout(Duration::from_secs(15),self.session(id))=>value.map_err(|_|"Cloud file transfer connection exceeded 15 seconds")??, _=cancellation.cancelled()=>return Err("YOUGORI_OPERATION_CANCELLED: cloud transfer cancelled before connecting".into()) };
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
        let mut hashed=0;
        loop {
            if let Some(operation)=&operation {operation.check()?;}
            let count = tokio::select!{
                value=tokio::time::timeout(Duration::from_secs(90),input.read(&mut buffer))=>value.map_err(|_|"YOUGORI_TRANSFER_INACTIVE: cloud archive hashing made no progress for 90 seconds; no guest copy started")?.map_err(|e|e.to_string())?,
                _=cancellation.cancelled()=>return Err("YOUGORI_OPERATION_CANCELLED: cloud archive verification cancelled; no guest copy started".into())
            };
            if count == 0 { break; }
            digest.update(&buffer[..count]);hashed+=count as u64;
            progress(CopyProgress{phase:"verifying",completed_bytes:hashed,total_bytes:archive_size,..Default::default()});
        }
        let config = json!({
            "transferId": transfer,
            "bytes": expected_bytes,
            "files": expected_files,
            "archiveBytes": archive_size,
            "checksum": hex::encode(digest.finalize()),
        });
        let original_report=progress.clone();
        let sent_counter=std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let confirmed_counter=std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let progress=Arc::new(move|mut event:CopyProgress|{
            use std::sync::atomic::Ordering;
            if let Some(sent)=event.sent_bytes{sent_counter.fetch_max(sent,Ordering::Relaxed);}
            if let Some(confirmed)=event.confirmed_bytes{confirmed_counter.fetch_max(confirmed,Ordering::Relaxed);}
            event.sent_bytes=Some(sent_counter.load(Ordering::Relaxed));event.confirmed_bytes=Some(confirmed_counter.load(Ordering::Relaxed));event.completed_bytes=event.sent_bytes.unwrap_or(0);original_report(event);
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
        let stdout = child.stdout.take().ok_or("Missing SSH upload response")?;
        let mut stderr = child.stderr.take().ok_or("Missing SSH upload diagnostics")?.take(16385);
        input.rewind().await.map_err(|e| e.to_string())?;
        let result = {
        let work = async {
            let send = send_archive(input, stdin, archive_size, progress.clone());
            let read_output = async {
                let mut reader=BufReader::new(stdout);let mut receipt=None;
                loop {
                    let mut line=String::new();
                    let count=tokio::io::AsyncReadExt::take(&mut reader,8193).read_line(&mut line).await.map_err(|_|"Cloud import progress stream failed")?;
                    if count==0{break;}if count>8192||!line.ends_with('\n'){return Err("Cloud import returned an oversized or incomplete record".into());}
                    if let Some(value)=line.trim_end().strip_prefix("yougori-import-progress ") {
                        let value:Value=serde_json::from_str(value).map_err(|_|"Invalid cloud import progress")?;
                        progress(CopyProgress{phase:"extracting",completed_bytes:0,total_bytes:archive_size,confirmed_bytes:value["confirmedBytes"].as_u64(),..Default::default()});
                    } else if line.starts_with("yougori-import-ready "){receipt=Some(line);}else{return Err("Unexpected cloud import output".into());}
                }
                receipt.ok_or_else(||"Cloud server did not confirm the file upload".to_owned())
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
        };
        tokio::pin!(work);
        let start=Instant::now();let mut tick=tokio::time::interval(Duration::from_secs(1));
        loop {tokio::select! {
            result=&mut work=>break result,
            _=cancellation.cancelled()=>break Err("YOUGORI_OPERATION_CANCELLED: cloud upload cancelled; unpublished staging is removed by the receiver on EOF; originals are unchanged".into()),
            _=tick.tick()=>{
                if start.elapsed()>Duration::from_secs(12*60*60){break Err("YOUGORI_TRANSFER_DEADLINE: cloud upload exceeded 12 hours".into());}
                if operation.as_ref().is_some_and(|op|op.idle_for()>Duration::from_secs(90)){break Err("YOUGORI_TRANSFER_INACTIVE: cloud upload made no progress for 90 seconds; SSH streams closed, unpublished staging removed on EOF".into());}
            }
        }}
        };
        if result.is_err() {
            let _ = tokio::time::timeout(Duration::from_secs(2),child.kill()).await;
            let _ = tokio::time::timeout(Duration::from_secs(2),child.wait()).await;
        }
        if result.is_ok(){progress(CopyProgress{phase:"verifying",completed_bytes:archive_size,total_bytes:archive_size,sent_bytes:Some(archive_size),confirmed_bytes:Some(expected_bytes),..Default::default()});}
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
