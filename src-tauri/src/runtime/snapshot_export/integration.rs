use super::*;
use crate::models::{Priority, ResourcePolicy, ResourceRange};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "boots a disposable runtime and verifies large snapshots, cancellation, pauses and restore"]
async fn snapshots_stream_without_guest_disk_copies_and_restore_files() -> Result<(), String> {
    let directory = tempfile::tempdir().map_err(|e| e.to_string())?;
    let runtime = RuntimeManager::new(Path::new(env!("CARGO_MANIFEST_DIR")), directory.path())?;
    let id = format!("snapshot-fixture-{}", uuid::Uuid::new_v4().simple());
    let snapshot_id = format!("snap-{}", uuid::Uuid::new_v4().simple());
    let image = "quay.io/libpod/alpine:latest";
    let command = "trap 'exit 0' TERM; while :; do sleep 1; done";
    let policy = ResourcePolicy {
        cpu: ResourceRange { min: 1.0, preferred: 2.0, max: 2.0, current: 0.0 },
        memory_gb: ResourceRange { min: 0.5, preferred: 0.5, max: 1.0, current: 0.0 },
        priority: Priority::Normal, dynamic: true,
    };
    macro_rules! ensure { ($condition:expr, $message:expr) => { if !$condition { return Err($message.to_string()); } }; }
    let result = async {
        runtime.provision_container(&id, image, command, &policy, false, false).await?;
        runtime.container_action(&id, "start", false).await?;
        let setup = runtime.execute_container_command(&id, "mkdir -p '/project with spaces' && dd if=/dev/urandom of='/project with spaces/crm.db' bs=1M count=256 2>/dev/null && chmod 640 '/project with spaces/crm.db' && ln -s crm.db '/project with spaces/database-link' && sha256sum '/project with spaces/crm.db' && stat -c '%a:%u:%g' '/project with spaces/crm.db' && readlink '/project with spaces/database-link'").await?;
        ensure!(setup.exit_code == 0, setup.stderr);
        let before = used_bytes(&runtime, &id).await?;
        // Interrupt a real export after headers arrive; the agent must resume
        // the container and release its temporary mount without a snapshot.
        let endpoint = runtime.container_endpoint(&id).await?;
        let response = runtime.client.post(format!("{}/v1/snapshots/export", endpoint.base_url))
            .bearer_auth(&endpoint.token).json(&json!({"id":id,"snapshotId":"cancelled-fixture"}))
            .send().await.map_err(|e| e.to_string())?;
        ensure!(response.status().is_success(), format!("Export failed: {}", response.status()));
        drop(response);
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let entries = runtime.container_telemetry(std::slice::from_ref(&id)).await?;
            ensure!(entries[0].running, "Cancelled export stopped the container");
            if !entries[0].paused { break; }
            ensure!(Instant::now() < deadline, "Cancelled export left the container paused");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        eprintln!("Cancelled export resumed the container");
        let started = Instant::now();
        let snapshot = runtime.create_container_snapshot(&id, &snapshot_id, image, command);
        tokio::pin!(snapshot);
        let mut saw_pause = false;
        let artifact = loop {
            tokio::select! {
                result = &mut snapshot => break result?,
                _ = tokio::time::sleep(Duration::from_millis(40)) => {
                    let entries = runtime.container_telemetry(std::slice::from_ref(&id)).await?;
                    ensure!(entries[0].running, "Snapshot reported a live container as exited");
                    saw_pause |= entries[0].paused;
                    ensure!(runtime.container_failure_detail(&id).await?.is_none(), "Snapshot pause reported an exit error");
                }
            }
        };
        ensure!(saw_pause, "Did not observe snapshot pause");
        let after = used_bytes(&runtime, &id).await?;
        ensure!(after.saturating_sub(before) < 64 * 1024 * 1024, format!("Snapshot duplicated data in the container disk: before={before}, after={after}"));
        ensure!(artifact.size_bytes > 256 * 1024 * 1024, "Incompressible database was not included");
        eprintln!("256 MiB snapshot: {:.2}s; guest storage increase: {} bytes; archive: {} bytes", started.elapsed().as_secs_f64(), after.saturating_sub(before), artifact.size_bytes);
        let changed = runtime.execute_container_command(&id, "printf changed > '/project with spaces/crm.db'").await?;
        ensure!(changed.exit_code == 0, changed.stderr);
        runtime.container_action(&id, "stop", false).await?;
        runtime.import_container_snapshot(&snapshot_id, &artifact.path).await?;
        runtime.restore_container_snapshot(&id, &snapshot_id, image, command, false, false).await?;
        runtime.update_container_resources(&id, 2.0, 0.5).await?;
        runtime.container_action(&id, "start", false).await?;
        let restored = runtime.execute_container_command(&id, "sha256sum '/project with spaces/crm.db' && stat -c '%a:%u:%g' '/project with spaces/crm.db' && readlink '/project with spaces/database-link'").await?;
        ensure!(restored.exit_code == 0 && restored.stdout == setup.stdout, format!("Restore changed data/permissions/links: {}", restored.stderr));
        eprintln!("Restore preserved database bytes, mode, owner and symlink");
        // Keep the remaining state tests small while exercising the same path.
        runtime.execute_container_command(&id, "rm '/project with spaces/crm.db'").await?;
        runtime.container_action(&id, "pause", false).await?;
        runtime.create_container_snapshot(&id, "already-paused-fixture", image, command).await?;
        ensure!(runtime.container_telemetry(std::slice::from_ref(&id)).await?[0].paused, "Snapshot resumed a user-paused container");
        runtime.container_action(&id, "resume", false).await?;
        runtime.container_action(&id, "stop", false).await?;
        runtime.create_container_snapshot(&id, "stopped-fixture", image, command).await?;
        ensure!(!runtime.container_telemetry(std::slice::from_ref(&id)).await?[0].running, "Snapshot started a stopped container");
        eprintln!("Already-paused and stopped containers retained their state");
        Ok(())
    }.await;
    runtime.shutdown_all().await;
    result
}

async fn used_bytes(runtime: &RuntimeManager, id: &str) -> Result<u64, String> {
    let output = runtime.execute_container_command(id, "df -k / | tail -n 1 | awk '{print $3}'").await?;
    output.stdout.trim().parse::<u64>().map(|blocks| blocks * 1024).map_err(|e| e.to_string())
}

#[cfg(windows)]
struct StalledSnapshotRelay {
    base: String,
    started: Arc<tokio::sync::Notify>,
    cancel_requests: Arc<Mutex<Vec<Value>>>,
    task: tokio::task::JoinHandle<()>,
}
#[cfg(windows)]
impl Drop for StalledSnapshotRelay {
    fn drop(&mut self) { self.task.abort(); }
}

#[cfg(windows)]
async fn snapshot_relay(base: String, token: String) -> Result<StalledSnapshotRelay, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.map_err(|error| error.to_string())?;
    let relay_base = format!("http://{}", listener.local_addr().map_err(|error| error.to_string())?);
    let started = Arc::new(tokio::sync::Notify::new());
    let cancel_requests = Arc::new(Mutex::new(Vec::new()));
    let ready = started.clone();
    let cancellations = cancel_requests.clone();
    let client = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(60)).build().map_err(|error| error.to_string())?;
    let task = tokio::spawn(async move {
        let mut tasks = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                accepted = listener.accept() => {
                    let Ok((mut socket, _)) = accepted else { break; };
                    let base = base.clone(); let token = token.clone(); let ready = ready.clone();
                    let cancellations = cancellations.clone(); let client = client.clone();
                    tasks.spawn(async move {
                        let mut header = Vec::new();
                        loop {
                            header.push(socket.read_u8().await.ok()?);
                            if header.ends_with(b"\r\n\r\n") { break; }
                            if header.len() > 16384 { return None; }
                        }
                        let header = String::from_utf8(header).ok()?;
                        let mut request_line = header.lines().next()?.split_whitespace();
                        let method = reqwest::Method::from_bytes(request_line.next()?.as_bytes()).ok()?;
                        let path = request_line.next()?;
                        let authorization = header.lines().find_map(|line| line.split_once(':')
                            .filter(|(name, _)| name.eq_ignore_ascii_case("authorization")).map(|(_, value)| value.trim()));
                        if authorization != Some(format!("Bearer {token}").as_str()) { return None; }
                        let length = header.lines().find_map(|line| line.to_ascii_lowercase().strip_prefix("content-length:")
                            .and_then(|value| value.trim().parse::<usize>().ok())).unwrap_or(0);
                        if length > 65536 { return None; }
                        let mut body = vec![0; length]; socket.read_exact(&mut body).await.ok()?;
                        if path == "/v1/snapshots/export/cancel" {
                            cancellations.lock().unwrap().push(serde_json::from_slice(&body).ok()?);
                        }
                        let mut response = client.request(method, format!("{base}{path}")).bearer_auth(&token)
                            .header(reqwest::header::CONTENT_TYPE, "application/json").body(body).send().await.ok()?;
                        let status = response.status().as_u16();
                        if path == "/v1/snapshots/export" && response.status().is_success() {
                            socket.write_all(format!("HTTP/1.1 {status} OK\r\nContent-Type: {CONTENT_TYPE}\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n").as_bytes()).await.ok()?;
                            let mut received = 0;
                            while received < 1024 * 1024 {
                                let chunk = response.chunk().await.ok()??;
                                received += chunk.len();
                                socket.write_all(format!("{:x}\r\n", chunk.len()).as_bytes()).await.ok()?;
                                socket.write_all(&chunk).await.ok()?; socket.write_all(b"\r\n").await.ok()?;
                            }
                            // Stop consuming the real guest's stream after active file
                            // copying starts. This applies real HTTP backpressure while
                            // keeping cancellation and lifecycle requests independent.
                            ready.notify_one();
                            let _ = socket.read_u8().await;
                            drop(response);
                        } else {
                            let body = response.bytes().await.ok()?;
                            socket.write_all(format!("HTTP/1.1 {status} Response\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).as_bytes()).await.ok()?;
                            socket.write_all(&body).await.ok()?;
                        }
                        Some(())
                    });
                },
                _ = tasks.join_next(), if !tasks.is_empty() => {},
            }
        }
    });
    Ok(StalledSnapshotRelay { base: relay_base, started, cancel_requests, task })
}

#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires YOUGORI_TEST_RESOURCE_ROOT; boots two private OCI fixtures and stalls only their snapshot stream; no user environments"]
async fn stalled_snapshot_stop_cancels_updated_guest_and_preserves_independent_environment() -> Result<(), String> {
    let resource = PathBuf::from(std::env::var_os("YOUGORI_TEST_RESOURCE_ROOT").ok_or("Set YOUGORI_TEST_RESOURCE_ROOT to the isolated, verified candidate resources")?);
    let directory = tempfile::tempdir().map_err(|error| error.to_string())?;
    let runtime = Arc::new(RuntimeManager::new_with_fixture_resources(&resource, directory.path())?);
    eprintln!("Snapshot Stop fixture candidate: {}", runtime.layout.appliance_initramfs.display());
    let id = format!("snapshot-stop-{}", uuid::Uuid::new_v4().simple());
    let other = format!("snapshot-peer-{}", uuid::Uuid::new_v4().simple());
    let snapshot_id = format!("snap-{}", uuid::Uuid::new_v4().simple());
    let image = "quay.io/libpod/alpine:latest";
    let command = "trap 'exit 0' TERM; while :; do sleep 1; done";
    let policy = ResourcePolicy {
        cpu: ResourceRange { min: 0.5, preferred: 0.5, max: 1.0, current: 0.0 },
        memory_gb: ResourceRange { min: 0.25, preferred: 0.25, max: 0.5, current: 0.0 },
        priority: Priority::Normal, dynamic: true,
    };
    let mut relay = None;
    let mut original_endpoint = None;
    let mut snapshot_task = None;
    let result = async {
        for environment in [&id, &other] {
            runtime.provision_container(environment, image, command, &policy, false, false).await?;
            runtime.container_action(environment, "start", false).await?;
        }
        let output = runtime.execute_container_command(&id, "printf 'unicode 日本語'; printf 'stderr marker' >&2; exit 17").await?;
        if output.exit_code != 17 || output.stdout != "unicode 日本語" || output.stderr != "stderr marker" {
            return Err(format!("Guest execution did not preserve its actual exit/output: {output:?}"));
        }
        let endpoint = runtime.container_endpoint(&id).await?;
        let guest_boot = runtime.execute_container_command(&id, "cat /proc/cmdline 2>/dev/null || true").await?;
        if guest_boot.stdout.contains(&endpoint.token) {
            return Err("Container can read the private appliance credential through /proc/cmdline".into());
        }
        let firmware = runtime.execute_container_command(&id, "cat /sys/firmware/qemu_fw_cfg/by_name/opt/yougori/control-token/raw 2>/dev/null || true").await?;
        if firmware.stdout.contains(&endpoint.token) {
            return Err("Container can read the private appliance firmware credential".into());
        }
        let mut archive = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_size(4); header.set_mode(0o600); header.set_cksum();
        archive.append_data(&mut header, "audit.bin", &[0_u8, 255, 0, 17][..]).map_err(|error|error.to_string())?;
        let complete = archive.into_inner().map_err(|error|error.to_string())?;
        let transfer = uuid::Uuid::new_v4().simple().to_string();
        let response = runtime.client.post(format!("{}/v1/files/import?id={id}&transfer={transfer}",endpoint.base_url))
            .bearer_auth(&endpoint.token).body(complete.clone()).send().await.map_err(|error|error.to_string())?;
        let status = response.status(); let receipt: Value = response.json().await.map_err(|error|error.to_string())?;
        if !status.is_success() || receipt["bytes"] != 4 || receipt["destination"] != format!("/yougori-import-{transfer}") {
            return Err(format!("Candidate did not acknowledge its complete binary import: {receipt}"));
        }
        let imported = runtime.execute_container_command(&id, &format!("od -An -tx1 /yougori-import-{transfer}/audit.bin; stat -c '%a' /yougori-import-{transfer}/audit.bin")).await?;
        if imported.exit_code != 0 || imported.stdout.split_whitespace().collect::<Vec<_>>() != ["00","ff","00","11","600"] {
            return Err("Candidate import changed binary data or its private permissions".into());
        }
        // The guest must reject both a bare file and orphan GNU metadata whose
        // zero-filled name data could previously masquerade as end-of-archive.
        let bare = complete[..complete.len()-1024].to_vec();
        let mut metadata = complete[..512].to_vec();
        metadata[156] = b'L'; metadata[124..136].copy_from_slice(b"00000004010\0");
        metadata[148..156].fill(b' ');
        let checksum: usize = metadata.iter().map(|byte|usize::from(*byte)).sum();
        metadata[148..156].copy_from_slice(format!("{checksum:06o}\0 ").as_bytes());
        metadata.extend_from_slice(b"orphan\0\0"); metadata.resize(512+2560,0);
        let mut orphan = bare.clone(); orphan.extend(metadata);
        for invalid in [bare, orphan] {
            let transfer = uuid::Uuid::new_v4().simple().to_string();
            let response = runtime.client.post(format!("{}/v1/files/import?id={id}&transfer={transfer}",endpoint.base_url))
                .bearer_auth(&endpoint.token).body(invalid).send().await.map_err(|error|error.to_string())?;
            if response.status() != reqwest::StatusCode::BAD_REQUEST {
                return Err("Candidate acknowledged an archive without an actual terminator".into());
            }
        }
        eprintln!("Candidate preserves exit 17, Unicode/stderr, complete binary imports, permissions, and rejects orphan metadata");
        let setup = runtime.execute_container_command(&id, "dd if=/dev/urandom of=/snapshot-original.bin bs=1M count=128 2>/dev/null && sha256sum /snapshot-original.bin").await?;
        if setup.exit_code != 0 { return Err(format!("Create owned snapshot fixture: {}", setup.stderr)); }
        let peer_setup = runtime.execute_container_command(&other, "printf INDEPENDENT_FIXTURE > /snapshot-peer-marker").await?;
        if peer_setup.exit_code != 0 { return Err("Could not prepare independent fixture".into()); }
        let endpoint = runtime.container_endpoint(&id).await?;
        let proxy = snapshot_relay(endpoint.base_url.clone(), endpoint.token).await?;
        original_endpoint = Some(endpoint.base_url);
        {
            let mut appliance = runtime.appliance.lock().await;
            appliance.as_mut().ok_or("Owned appliance disappeared")?.endpoint.base_url = proxy.base.clone();
        }
        let task_runtime = runtime.clone(); let task_id = id.clone(); let task_snapshot = snapshot_id.clone();
        snapshot_task = Some(tokio::spawn(async move {
            task_runtime.create_container_snapshot(&task_id, &task_snapshot, image, command).await
        }));
        relay = Some(proxy);
        tokio::time::timeout(Duration::from_secs(30), relay.as_ref().unwrap().started.notified()).await
            .map_err(|_| "Real snapshot never reached its stalled file-copy phase")?;
        let telemetry = runtime.container_telemetry(&[id.clone(), other.clone()]).await?;
        if !telemetry.iter().any(|entry| entry.id == id && entry.running && entry.paused)
            || !telemetry.iter().any(|entry| entry.id == other && entry.running && !entry.paused) {
            return Err("Snapshot did not pause only its source while keeping the independent environment running".into());
        }
        let began = Instant::now();
        let interruption = runtime.interrupt_snapshot_exports(&id);
        tokio::time::timeout(Duration::from_secs(10), runtime.container_action(&id, "stop", false)).await
            .map_err(|_| "Stop remained blocked by the stalled snapshot stream")??;
        let joined = tokio::time::timeout(Duration::from_secs(8), snapshot_task.as_mut().unwrap()).await
            .map_err(|_| "Snapshot task did not release its cancelled stream")?;
        snapshot_task.take();
        let snapshot = joined.map_err(|error| error.to_string())?;
        let error = snapshot.err().ok_or("Cancelled snapshot unexpectedly published an archive")?;
        if !error.starts_with("YOUGORI_OPERATION_CANCELLED: snapshot export interrupted;") { return Err(error); }
        if !relay.as_ref().unwrap().cancel_requests.lock().unwrap().iter().any(|request| request == &json!({"id":id,"snapshotId":snapshot_id})) {
            return Err("Backend did not cancel the exact guest snapshot export".into());
        }
        if directory.path().join("runtime/snapshots").read_dir().map_err(|error| error.to_string())?.next().is_some() {
            return Err("Cancelled snapshot retained an unpublished host archive".into());
        }
        let telemetry = runtime.container_telemetry(&[id.clone(), other.clone()]).await?;
        if telemetry.iter().any(|entry| entry.id == id && entry.running)
            || !telemetry.iter().any(|entry| entry.id == other && entry.running && !entry.paused) {
            return Err("Stop changed the independent environment or failed its own postcondition".into());
        }
        eprintln!("Real snapshot stream + exact cancellation + Stop completed in {:.3}s", began.elapsed().as_secs_f64());
        drop(interruption);
        runtime.container_action(&id, "start", false).await?;
        let preserved = runtime.execute_container_command(&id, "sha256sum /snapshot-original.bin").await?;
        if preserved.exit_code != 0 || preserved.stdout != setup.stdout { return Err("Snapshot interruption changed its original source file".into()); }
        let peer = runtime.execute_container_command(&other, "cat /snapshot-peer-marker").await?;
        if peer.exit_code != 0 || peer.stdout != "INDEPENDENT_FIXTURE" { return Err("Snapshot interruption changed the independent fixture".into()); }
        eprintln!("Original 128 MiB source checksum and independent fixture preserved");
        Ok(())
    }.await;
    let _cleanup_interruption = runtime.interrupt_snapshot_exports(&id);
    if let Some(task) = snapshot_task { task.abort(); let _ = task.await; }
    if let Some(endpoint) = original_endpoint {
        if let Some(appliance) = runtime.appliance.lock().await.as_mut() { appliance.endpoint.base_url = endpoint; }
    }
    drop(relay);
    runtime.shutdown_all().await;
    result
}
