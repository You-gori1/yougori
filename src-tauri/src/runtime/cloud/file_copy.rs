use super::*;
use std::{collections::HashSet, io::{Read, Write}, time::Instant};
use tokio_util::sync::CancellationToken;
use crate::file_import::{transfers, CopyProgress};

const EXPORT: &str = include_str!("export_files.py");

/// The SSH stream and blocking verifier share one measured inactivity clock.
/// A quiet stderr stream does not time out while file bytes are still moving.
struct ArchiveControl {
    transfer: Option<Arc<transfers::Transfer>>,
    cancellation: CancellationToken,
    operation: Option<crate::automation::context::OperationContext>,
    started: Instant,
    measurement: Mutex<(&'static str, u64, Instant)>,
    inactivity: Duration,
    total_limit: Duration,
    received: std::sync::atomic::AtomicU64,
    blocking_workers:std::sync::atomic::AtomicUsize,
}
impl ArchiveControl {
    fn new(transfer: Option<Arc<transfers::Transfer>>) -> Arc<Self> {
        let operation = crate::automation::context::current();
        Arc::new(Self {
            transfer, cancellation:operation.as_ref().map(|operation|operation.cancellation.child_token()).unwrap_or_default(), operation,
            started:Instant::now(), measurement:Mutex::new(("connecting",0,Instant::now())),
            inactivity:Duration::from_secs(90), total_limit:Duration::from_secs(12 * 60 * 60), received:Default::default(), blocking_workers:Default::default(),
        })
    }
    fn check(&self) -> Result<(), String> {
        if self.cancellation.is_cancelled() || self.transfer.as_ref().is_some_and(|transfer|transfer.cancellation.is_cancelled()) {
            return Err("YOUGORI_OPERATION_CANCELLED: cloud export cancelled; originals unchanged; unverified host staging is removed after its stream/disk handles close".into());
        }
        if self.started.elapsed() >= self.total_limit {
            return Err("YOUGORI_TRANSFER_DEADLINE: cloud export exceeded its 12-hour total limit; originals unchanged; unverified host staging is removed after its handles close".into());
        }
        let measurement = self.measurement.lock().unwrap_or_else(|poisoned|poisoned.into_inner());
        if measurement.2.elapsed() >= self.inactivity {
            return Err(format!("YOUGORI_TRANSFER_INACTIVE: cloud export made no progress for {} seconds during {}; {} bytes received. Originals unchanged; unverified host staging is removed after its handles close", self.inactivity.as_secs(),measurement.0,self.received.load(std::sync::atomic::Ordering::Relaxed)));
        }
        Ok(())
    }
    fn report(&self, phase: &'static str, completed: u64, total: u64) {
        let mut measurement = self.measurement.lock().unwrap_or_else(|poisoned|poisoned.into_inner());
        if measurement.0 != phase || measurement.1 != completed { *measurement = (phase,completed,Instant::now()); }
        drop(measurement);
        let received = self.received.load(std::sync::atomic::Ordering::Relaxed);
        let mut progress = CopyProgress { phase,completed_bytes:completed,total_bytes:total,sent_bytes:Some(received),confirmed_bytes:Some(if phase == "complete" {received} else {0}),..Default::default() };
        if let Some(transfer) = &self.transfer { progress = transfer.report(progress); }
        if let Some(operation) = &self.operation { (operation.progress)(json!({"phase":phase,"completedBytes":completed,"totalBytes":total,"receivedBytes":received,"sentBytes":received,"confirmedBytes":progress.confirmed_bytes,"lastProgressAt":progress.last_progress_at.unwrap_or_else(||chrono::Utc::now().to_rfc3339()),"transferId":self.transfer.as_ref().map(|transfer|&transfer.id),"partialCopyPolicy":"remove_unverified_host_archive_preserve_originals"})); }
    }
    async fn watch(&self) -> String {
        loop {
            if let Err(error) = self.check() { return error; }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
    async fn blocking<T:Send+'static>(self:&Arc<Self>, work:impl FnOnce()->Result<T,String>+Send+'static) -> Result<T,String> {
        self.blocking_workers.fetch_add(1,std::sync::atomic::Ordering::SeqCst);
        let control = self.clone();
        tokio::task::spawn_blocking(move|| {
            struct Worker(Arc<ArchiveControl>);
            impl Drop for Worker {fn drop(&mut self) {self.0.blocking_workers.fetch_sub(1,std::sync::atomic::Ordering::SeqCst);}}
            let _worker = Worker(control);
            work()
        }).await.map_err(|error|error.to_string())?
    }
    async fn drain_disk_workers(&self) -> bool {
        tokio::time::timeout(Duration::from_secs(3),async {
            while self.blocking_workers.load(std::sync::atomic::Ordering::SeqCst) > 0 {tokio::time::sleep(Duration::from_millis(10)).await;}
        }).await.is_ok()
    }
}

struct VerifiedReader<R> { inner:R,control:Option<Arc<ArchiveControl>>,completed:u64,total:u64 }
impl<R:Read> Read for VerifiedReader<R> {
    fn read(&mut self, buffer:&mut [u8]) -> std::io::Result<usize> {
        if let Some(control) = &self.control { control.check().map_err(std::io::Error::other)?; }
        let count = self.inner.read(buffer)?;
        let previous = self.completed;
        self.completed = self.completed.saturating_add(count as u64);
        if previous / (1024 * 1024) != self.completed / (1024 * 1024) || count == 0 {
            if let Some(control) = &self.control { control.report("verifying",self.completed,self.total); }
        }
        Ok(count)
    }
}

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

#[cfg(test)]
pub(crate) fn inspect_archive(
    path: &Path,
    operation: &str,
    maximum: u64,
) -> Result<FileArchive, String> {
    inspect_archive_controlled(path,operation,maximum,None)
}
fn inspect_archive_controlled(path:&Path,operation:&str,maximum:u64,control:Option<Arc<ArchiveControl>>) -> Result<FileArchive,String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let archive_bytes = file.metadata().map_err(|e| e.to_string())?.len();
    let mut archive = tar::Archive::new(VerifiedReader {inner:file,control:control.clone(),completed:0,total:archive_bytes.saturating_mul(2)});
    let mut names = HashSet::new();
    let mut bytes = 0u64;
    let mut files = 0u64;
    let mut receipt = None;
    for entry in archive.entries().map_err(|e| e.to_string())? {
        if let Some(control) = &control { control.check()?; }
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
    let mut input = VerifiedReader { inner:std::fs::File::open(path).map_err(|e|e.to_string())?,control:control.clone(),completed:archive_bytes,total:archive_bytes.saturating_mul(2) };
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
        let lease = transfers::begin(id)?;
        lease.transfer.use_host_archive_export();
        let control = ArchiveControl::new(Some(lease.transfer.clone()));
        control.report("connecting",0,0);
        let profile = self.profile(id)?;
        validate(&profile, true)?;
        tokio::select! { result=check_identity_agent(&profile.identity_file)=>result?, error=control.watch()=>return Err(error) };
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
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        receive_archive_controlled(command,destination,operation,maximum,control).await
    }
}

#[cfg(test)]
async fn receive_archive(
    command: Command,
    destination: &Path,
    operation: &str,
    maximum: u64,
) -> Result<FileArchive, String> {
    receive_archive_controlled(command,destination,operation,maximum,ArchiveControl::new(None)).await
}
async fn receive_archive_controlled(mut command:Command,destination:&Path,operation:&str,maximum:u64,control:Arc<ArchiveControl>) -> Result<FileArchive,String> {
    control.check()?;
    // Probe/create on a blocking worker before opening SSH. A stalled host
    // filesystem cannot occupy an async runtime worker or leave a stream open.
    let parent = destination.parent().ok_or("Missing transfer directory")?.to_owned();
    let target = destination.to_owned();
    let temporary = control.blocking(move || {
        if target.try_exists().map_err(|error|error.to_string())? { return Err("A completed archive already exists at the destination; inspect or resume that operation instead of overwriting it".to_string()); }
        tempfile::Builder::new().prefix("yougori-export-").suffix(".part").tempfile_in(parent).map_err(|error|error.to_string())
    });
    let mut file = tokio::select! { result=temporary=>result?, error=control.watch()=>return Err(format!("YOUGORI_OPERATION_INTERRUPTED: {error}; host staging preparation may still have OS I/O pending; its private cleanup guard runs when that worker exits; inspect this operation before retrying")) };
    command.kill_on_drop(true);
    let mut child = command
        .spawn()
        .map_err(|e| format!("Start SSH file transfer: {e}"))?;
    let mut output = child.stdout.take().ok_or("Missing SSH stream")?;
    let mut errors = child
        .stderr
        .take()
        .ok_or("Missing SSH diagnostics")?
        .take(65537);
    // A unique NamedTempFile owns cleanup through blocked disk workers. The
    // final create-new publish never replaces another operation's archive.
    let transfer_result = async {
        let transfer = async {
            let mut buffer = vec![0; 128 * 1024];
            let mut received = 0u64;
            let mut checked = 0u64;
            loop {
                if received >= checked {
                    let parent = destination.parent().ok_or("Missing transfer directory")?.to_owned();
                    control.blocking(move || {
                        let disks = sysinfo::Disks::new_with_refreshed_list();
                        let disk = crate::runtime::storage::runtime_disk(&disks,&parent).ok_or("Cannot check transfer-drive free space")?;
                        if disk.available_space() < 2 * 1024u64.pow(3) + 64 * 1024 * 1024 { return Err("File transfer paused: keep at least 2 GB free on the transfer drive".to_string()); }
                        Ok(())
                    }).await?;
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
                let bytes = buffer[..count].to_vec();
                file = control.blocking(move || { file.write_all(&bytes).map_err(|error|error.to_string())?; Ok::<_,String>(file) }).await?;
                control.received.store(received,std::sync::atomic::Ordering::Relaxed);
                control.report("receiving",received,0);
            }
            control.report("waitingForSourceCompletion",received,0);
            file = control.blocking(move || { file.as_file().sync_all().map_err(|error|error.to_string())?; Ok::<_,String>(file) }).await?;
            Ok::<_, String>(file)
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
        let (file, diagnostics) = tokio::try_join!(transfer, diagnostics)?;
        let status = child.wait().await.map_err(|e| e.to_string())?;
        if !status.success() {
            return Err(format!(
                "Cloud file transfer failed: {}",
                crate::lifecycle::safe_diagnostic(diagnostics.trim())
            ));
        }
        let op = operation.to_owned();
        let verifier = control.clone();
        let received = control.received.load(std::sync::atomic::Ordering::Relaxed);
        control.report("verifying",0,received.saturating_mul(2));
        control.blocking(move || {
            let metadata = inspect_archive_controlled(file.path(),&op,maximum,Some(verifier))?;
            Ok::<_,String>((metadata,file))
        }).await
    };
    let result = tokio::select! { result=transfer_result=>result, error=control.watch()=>Err(error) };
    let (metadata,file) = match result {
        Ok(result)=>result,
        Err(error)=> {
            control.cancellation.cancel();
            let _ = child.start_kill();
            let stopped = tokio::time::timeout(Duration::from_secs(3),child.wait()).await;
            let stream_stopped = matches!(stopped,Ok(Ok(_)));
            let disk_released = control.drain_disk_workers().await;
            return Err(export_failure(&error,stream_stopped,disk_released));
        }
    };
    control.check()?;
    let destination = destination.to_owned();
    let published = control.blocking(move || {
        file.persist_noclobber(&destination).map_err(|error|format!("Cannot publish verified archive without overwriting an existing file: {}",error.error))?;
        Ok::<_,String>(metadata)
    });
    // Once the atomic publication is accepted, report its actual outcome even
    // if cancellation races it. Retrying an unknown committed copy is unsafe.
    let metadata = tokio::time::timeout(Duration::from_secs(90),published).await.map_err(|_| "YOUGORI_OPERATION_INTERRUPTED: archive publication OS I/O is still pending; inspect the destination and its completion receipt before retrying".to_string())??;
    control.report("complete",metadata.archive_bytes,metadata.archive_bytes);
    Ok(metadata)
}

fn export_failure(error:&str,stream_stopped:bool,disk_released:bool) -> String {
    if !stream_stopped || !disk_released {
        format!("YOUGORI_OPERATION_INTERRUPTED: {error}; SSH termination verified={stream_stopped}, host disk handles released={disk_released}. Reconciliation required: inspect the private export staging and published destination before retrying. Pending host I/O retains its cleanup guard until its handle closes.")
    } else {format!("{error}; owned SSH stream stopped and host disk workers released; unverified private staging cleanup was attempted; originals unchanged")}
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
    fn stalled_command(pid_file:&Path) -> Command {
        let mut command = Command::new(if cfg!(windows) {"python"} else {"python3"});
        command.args(["-c","import os,sys,time;open(sys.argv[1],'w').write(str(os.getpid()));sys.stdout.buffer.write(b'partial');sys.stdout.buffer.flush();time.sleep(60)"])
            .arg(pid_file).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).kill_on_drop(true);
        super::super::super::configure_background_process(&mut command);
        command
    }
    fn assert_owned_child_stopped(pid_file:&Path) {
        let pid:u32 = std::fs::read_to_string(pid_file).unwrap().parse().unwrap();
        let system = sysinfo::System::new_all();
        assert!(system.process(sysinfo::Pid::from_u32(pid)).is_none(),"the export's owned SSH-like process was not stopped");
    }
    #[tokio::test]
    async fn stopped_progress_times_out_closes_the_owned_stream_and_removes_staging() {
        let directory = tempfile::tempdir().unwrap();
        let pid_file = directory.path().join("owned-process.pid");
        let destination = directory.path().join("files.tar");
        let mut control = ArchiveControl::new(None);
        Arc::get_mut(&mut control).unwrap().inactivity = Duration::from_millis(350);
        let started = Instant::now();
        let error = receive_archive_controlled(stalled_command(&pid_file),&destination,"copy",1024,control.clone()).await.unwrap_err();
        assert!(error.contains("YOUGORI_TRANSFER_INACTIVE"),"{error}");
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_eq!(control.received.load(std::sync::atomic::Ordering::Relaxed),7);
        assert!(!destination.exists());
        assert!(std::fs::read_dir(directory.path()).unwrap().all(|entry|!entry.unwrap().path().extension().is_some_and(|extension|extension == "part")));
        assert_owned_child_stopped(&pid_file);
    }
    #[tokio::test]
    async fn cancelling_native_cloud_export_closes_stream_without_publishing_partial_data() {
        let directory = tempfile::tempdir().unwrap();
        let pid_file = directory.path().join("owned-process.pid");
        let destination = directory.path().join("files.tar");
        let environment = format!("cloud-export-cancel-{}",uuid::Uuid::new_v4());
        let lease = transfers::begin(&environment).unwrap();
        lease.transfer.use_host_archive_export();
        let control = ArchiveControl::new(Some(lease.transfer.clone()));
        let observed = control.clone();
        let command = stalled_command(&pid_file);
        let output = destination.clone();
        let task = tokio::spawn(async move {receive_archive_controlled(command,&output,"copy",1024,control).await});
        tokio::time::timeout(Duration::from_secs(3),async {while observed.received.load(std::sync::atomic::Ordering::Relaxed) == 0 {tokio::time::sleep(Duration::from_millis(5)).await;}}).await.unwrap();
        let cancelled = transfers::cancel(Some(&environment),None);
        assert_eq!(cancelled["transfers"][0]["partialCopyPolicy"],"remove_unverified_host_archive_preserve_originals");
        let error = tokio::time::timeout(Duration::from_secs(3),task).await.unwrap().unwrap().unwrap_err();
        assert!(error.contains("YOUGORI_OPERATION_CANCELLED"),"{error}");
        assert!(!destination.exists());
        assert_owned_child_stopped(&pid_file);
        drop(lease);
        assert!(transfers::find(&cancelled["transfers"][0]["transferId"].as_str().unwrap()).is_none());
    }
    #[tokio::test]
    async fn an_existing_archive_is_not_overwritten_and_verification_honors_cancellation() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("files.tar");
        std::fs::write(&destination,b"existing archive").unwrap();
        let pid_file = directory.path().join("never-started.pid");
        let error = receive_archive(stalled_command(&pid_file),&destination,"copy",1024).await.unwrap_err();
        assert!(error.contains("already exists"));
        assert_eq!(std::fs::read(&destination).unwrap(),b"existing archive");
        assert!(!pid_file.exists());
        let source = archive(&[("file",b"data",b'0')]);
        let control = ArchiveControl::new(None);
        control.cancellation.cancel();
        let error = inspect_archive_controlled(source.path(),"copy",1024,Some(control)).unwrap_err();
        assert!(error.contains("YOUGORI_OPERATION_CANCELLED"),"{error}");
    }
    #[test]
    fn repeating_a_progress_heartbeat_does_not_keep_a_stalled_export_alive() {
        let mut control = ArchiveControl::new(None);
        Arc::get_mut(&mut control).unwrap().inactivity = Duration::from_millis(10);
        control.report("receiving",5,0);
        std::thread::sleep(Duration::from_millis(15));
        control.report("receiving",5,0);
        assert!(control.check().unwrap_err().contains("YOUGORI_TRANSFER_INACTIVE"));
    }
    #[test]
    fn unverified_stream_or_disk_cleanup_is_interrupted_instead_of_safely_cancelled() {
        for (stream,disk) in [(false,true),(true,false),(false,false)] {
            let error = export_failure("YOUGORI_OPERATION_CANCELLED: requested",stream,disk);
            assert!(error.starts_with("YOUGORI_OPERATION_INTERRUPTED"),"{error}");
            assert!(error.contains("Reconciliation required"));
        }
        assert!(export_failure("YOUGORI_OPERATION_CANCELLED: requested",true,true).starts_with("YOUGORI_OPERATION_CANCELLED"));
    }
}
