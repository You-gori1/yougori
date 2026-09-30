use super::*;
use std::{collections::HashSet, io::Read};
use tokio::io::AsyncWriteExt;

const EXPORT: &str = include_str!("export_files.py");

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileArchive {
    pub bytes: u64,
    pub files: u64,
    pub skipped: u64,
    pub archive_bytes: u64,
    pub checksum: String,
    pub receipt: Value,
}

pub(crate) fn inspect_archive(
    path: &Path,
    operation: &str,
    maximum: u64,
) -> Result<FileArchive, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let archive_bytes = file.metadata().map_err(|e| e.to_string())?.len();
    let mut archive = tar::Archive::new(file);
    let mut names = HashSet::new();
    let mut bytes = 0u64;
    let mut files = 0u64;
    let mut receipt = None;
    for entry in archive.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        if receipt.is_some() {
            return Err("Unexpected entries after the copy receipt".into());
        }
        let raw = entry.path_bytes();
        let name = std::str::from_utf8(&raw)
            .map_err(|_| "A filename is not valid UTF-8")?
            .trim_end_matches('/')
            .to_owned();
        if name.is_empty()
            || name.len() > 4096
            || name.contains(['\\', '\0'])
            || name
                .split('/')
                .any(|p| p.is_empty() || p == "." || p == "..")
            || !names.insert(name.clone())
            || names.len() > 1000001
        {
            return Err("The source returned an unsafe or repeated archive path".into());
        }
        let kind = entry.header().entry_type();
        if !kind.is_file() && !kind.is_dir() {
            return Err("Only ordinary files and folders may be copied".into());
        }
        let size = entry.size();
        if kind.is_dir() && size != 0 {
            return Err("Invalid directory in file archive".into());
        }
        if name == ".yougori-copy-complete.json" {
            if !kind.is_file() || size > 4096 {
                return Err("Invalid file-copy receipt".into());
            }
            let mut text = String::new();
            entry.read_to_string(&mut text).map_err(|e| e.to_string())?;
            let value: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
            if value["operationId"] != operation
                || value["bytes"].as_u64() != Some(bytes)
                || value["files"].as_u64() != Some(files)
                || value["skipped"].as_u64().is_none()
            {
                return Err("The cloud copy did not confirm every transferred file".into());
            }
            // The guest confirms receipt bytes as well as the user's files.
            bytes += size;
            receipt = Some(value);
        } else if kind.is_file() {
            bytes = bytes
                .checked_add(size)
                .filter(|n| *n <= maximum)
                .ok_or("Selected files exceed destination storage")?;
            let read =
                std::io::copy(&mut entry, &mut std::io::sink()).map_err(|e| e.to_string())?;
            if read != size {
                return Err("The source archive is incomplete".into());
            }
            files += 1;
        }
    }
    let receipt =
        receipt.ok_or("The source archive is incomplete: its completion receipt is missing")?;
    let mut input = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0; 1024 * 1024];
    loop {
        let count = input.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(FileArchive {
        bytes,
        files,
        skipped: receipt["skipped"].as_u64().unwrap(),
        archive_bytes,
        checksum: hex::encode(hash.finalize()),
        receipt,
    })
}

impl Cloud {
    pub(crate) async fn export_files(
        &self,
        id: &str,
        paths: &[String],
        operation: &str,
        destination: &Path,
        maximum: u64,
    ) -> Result<FileArchive, String> {
        let profile = self.profile(id)?;
        validate(&profile, true)?;
        check_identity_agent(&profile.identity_file).await?;
        let config = json!({"paths":paths,"operationId":operation,"maximumBytes":maximum});
        let mut command = ssh_command(&profile, &self.directory(id)?.join("known_hosts"))?;
        // Both arguments are base64 data, never shell-interpolated source paths.
        command
            .arg(format!(
                "python3 -c \"import base64;exec(base64.b64decode('{}'))\" {}",
                B64.encode(EXPORT),
                B64.encode(config.to_string())
            ))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        receive_archive(command, destination, operation, maximum).await
    }
}

async fn receive_archive(
    mut command: Command,
    destination: &Path,
    operation: &str,
    maximum: u64,
) -> Result<FileArchive, String> {
    let mut child = command
        .spawn()
        .map_err(|e| format!("Start SSH file transfer: {e}"))?;
    let mut output = child.stdout.take().ok_or("Missing SSH stream")?;
    let mut errors = child
        .stderr
        .take()
        .ok_or("Missing SSH diagnostics")?
        .take(65537);
    let partial = destination.with_extension("part");
    let result = tokio::time::timeout(Duration::from_secs(24 * 60 * 60), async {
        let transfer = async {
            let mut file = tokio::fs::File::create(&partial)
                .await
                .map_err(|e| e.to_string())?;
            let mut buffer = vec![0; 128 * 1024];
            let mut received = 0u64;
            let mut checked = 0u64;
            loop {
                if received >= checked {
                    let disks = sysinfo::Disks::new_with_refreshed_list();
                    let disk = crate::runtime::storage::runtime_disk(
                        &disks,
                        destination.parent().ok_or("Missing transfer directory")?,
                    )
                    .ok_or("Cannot check transfer-drive free space")?;
                    if disk.available_space() < 2 * 1024u64.pow(3) + 64 * 1024 * 1024 {
                        return Err(
                            "File transfer paused: keep at least 2 GB free on the transfer drive"
                                .into(),
                        );
                    }
                    checked = received + 64 * 1024 * 1024;
                }
                let count = output.read(&mut buffer).await.map_err(|e| e.to_string())?;
                if count == 0 {
                    break;
                }
                received += count as u64;
                if received > maximum.saturating_add(8 * 1024u64.pow(3)) {
                    return Err("The archive exceeded the transfer allowance".into());
                }
                file.write_all(&buffer[..count])
                    .await
                    .map_err(|e| e.to_string())?;
            }
            file.sync_all().await.map_err(|e| e.to_string())?;
            Ok::<(), String>(())
        };
        let diagnostics = async {
            let mut bytes = Vec::new();
            errors
                .read_to_end(&mut bytes)
                .await
                .map_err(|e| e.to_string())?;
            if bytes.len() > 65536 {
                return Err("SSH returned too much diagnostic output".into());
            }
            Ok::<_, String>(String::from_utf8_lossy(&bytes).into_owned())
        };
        let (_, diagnostics) = tokio::try_join!(transfer, diagnostics)?;
        let status = child.wait().await.map_err(|e| e.to_string())?;
        if !status.success() {
            return Err(format!(
                "Cloud file transfer failed: {}",
                diagnostics.trim()
            ));
        }
        let path = partial.clone();
        let op = operation.to_owned();
        let result = tokio::task::spawn_blocking(move || inspect_archive(&path, &op, maximum))
            .await
            .map_err(|e| e.to_string())??;
        tokio::fs::rename(&partial, destination)
            .await
            .map_err(|e| e.to_string())?;
        Ok(result)
    })
    .await
    .unwrap_or_else(|_| Err("SSH file transfer timed out; resume to try again".into()));
    if result.is_err() {
        let _ = child.kill().await;
        let _ = child.wait().await;
        let _ = tokio::fs::remove_file(&partial).await;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn archive(entries: &[(&str, &[u8], u8)]) -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        let mut builder = tar::Builder::new(file.as_file_mut());
        for (name, contents, kind) in entries {
            let mut header = tar::Header::new_gnu();
            // Raw header names deliberately exercise traversal rejection.
            header.as_mut_bytes()[..name.len()].copy_from_slice(name.as_bytes());
            header.set_entry_type(tar::EntryType::new(*kind));
            header.set_mode(0o644);
            header.set_size(contents.len() as u64);
            header.set_cksum();
            builder.append(&header, *contents).unwrap();
        }
        builder.finish().unwrap();
        drop(builder);
        file
    }
    #[test]
    fn cloud_file_archive_requires_safe_paths_matching_counts_and_a_final_receipt() {
        let receipt = json!({"operationId":"copy","files":1,"bytes":4,"skipped":0}).to_string();
        let good = archive(&[
            ("project", b"", b'5'),
            ("project/file", b"data", b'0'),
            (".yougori-copy-complete.json", receipt.as_bytes(), b'0'),
        ]);
        let metadata = inspect_archive(good.path(), "copy", 1024).unwrap();
        assert_eq!(metadata.files, 1);
        assert_eq!(metadata.bytes, 4 + receipt.len() as u64);
        assert_eq!(metadata.checksum.len(), 64);
        assert!(inspect_archive(good.path(), "other-operation", 1024).is_err());
        assert!(inspect_archive(good.path(), "copy", 3).is_err());
        for entries in [
            vec![("../escape", &b"data"[..], b'0')],
            vec![("/absolute", &b"data"[..], b'0')],
            vec![("link", &b""[..], b'2')],
            vec![("file", &b"data"[..], b'0')],
            vec![("file", &b"data"[..], b'0'), ("file", &b"data"[..], b'0')],
            vec![
                ("file", &b"data"[..], b'0'),
                (".yougori-copy-complete.json", receipt.as_bytes(), b'0'),
                ("extra", &b""[..], b'0'),
            ],
        ] {
            let bad = archive(&entries);
            assert!(inspect_archive(bad.path(), "copy", 1024).is_err());
        }
    }

    #[tokio::test]
    async fn cloud_file_transport_creates_a_new_archive_and_rejects_failed_streams() {
        let receipt = json!({"operationId":"copy","files":1,"bytes":4,"skipped":0}).to_string();
        let source = archive(&[
            ("file", b"data", b'0'),
            (".yougori-copy-complete.json", receipt.as_bytes(), b'0'),
        ]);
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("files.tar");
        let python = if cfg!(windows) { "python" } else { "python3" };
        let mut command = Command::new(python);
        command
            .args([
                "-c",
                "import sys;sys.stdout.buffer.write(open(sys.argv[1],'rb').read())",
            ])
            .arg(source.path())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        super::super::super::configure_background_process(&mut command);
        let result = receive_archive(command, &destination, "copy", 1024)
            .await
            .unwrap();
        assert_eq!(result.files, 1);
        assert_eq!(
            std::fs::read(&destination).unwrap(),
            std::fs::read(source.path()).unwrap()
        );
        let failed = directory.path().join("failed.tar");
        let mut command = Command::new(python);
        command.args(["-c", "import sys;sys.stdout.buffer.write(b'partial');sys.stderr.write('read permission denied');sys.exit(1)"])
            .stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).kill_on_drop(true);
        super::super::super::configure_background_process(&mut command);
        assert!(receive_archive(command, &failed, "copy", 1024)
            .await
            .unwrap_err()
            .contains("read permission denied"));
        assert!(!failed.exists());
        assert!(!failed.with_extension("part").exists());
    }
}
