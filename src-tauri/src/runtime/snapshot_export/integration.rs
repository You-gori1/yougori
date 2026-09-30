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
