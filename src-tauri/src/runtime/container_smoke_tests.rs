use super::*;
use crate::models::{Priority, ResourcePolicy, ResourceRange};
use std::time::Duration;

#[tokio::test]
#[ignore = "boots an isolated appliance, pulls Alpine, and verifies larger resource limits and safe resizing"]
async fn container_capacity_grows_without_losing_data_or_restarting_active_workloads() -> Result<(), String> {
    let data = tempfile::tempdir().unwrap();
    let runtime = RuntimeManager::new(Path::new(env!("CARGO_MANIFEST_DIR")), data.path())?;
    let policy = ResourcePolicy {
        cpu: ResourceRange { min: 0.5, preferred: 4.0, max: 4.0, current: 0.0 },
        memory_gb: ResourceRange { min: 0.5, preferred: 2.0, max: 2.0, current: 0.0 },
        priority: Priority::Normal, dynamic: false,
    };
    let result = async {
        runtime.ensure_container_capacity(4.0, 16.625).await?;
        runtime.provision_container("env-capacity", "quay.io/libpod/alpine:latest", "sleep 2147483647", &policy, false, false).await?;
        runtime.container_action("env-capacity", "start", false).await?;
        let before = runtime.appliance.lock().await.as_ref().unwrap().child.id();
        let output = runtime.execute_container_command("env-capacity", "echo capacity-survives > /root/capacity-marker; cat /sys/fs/cgroup/memory.max; cat /sys/fs/cgroup/cpu.max; head -3 /proc/meminfo").await?;
        assert_eq!(output.exit_code, 0, "{}", output.stderr);
        assert!(output.stdout.contains("2147483648"), "{}", output.stdout);
        assert!(output.stdout.contains("400000 100000"), "{}", output.stdout);
        eprintln!("Large container initial limits: {}", output.stdout);
        let growth = runtime.ensure_container_capacity(8.0, 18.5).await;
        eprintln!("First hotplug result: {growth:?}; guest memory: {:?}", runtime.execute_container_command("env-capacity", "head -3 /proc/meminfo").await);
        growth?;
        assert_eq!(before, runtime.appliance.lock().await.as_ref().unwrap().child.id());
        runtime.update_container_resources("env-capacity", 8.0, 4.0).await?;
        let output = runtime.execute_container_command("env-capacity", "cat /root/capacity-marker; cat /sys/fs/cgroup/memory.max; cat /sys/fs/cgroup/cpu.max; head -3 /proc/meminfo").await?;
        assert_eq!(output.exit_code, 0, "{}", output.stderr);
        assert!(output.stdout.contains("capacity-survives"), "{}", output.stdout);
        assert!(output.stdout.contains("4294967296"), "{}", output.stdout);
        assert!(output.stdout.contains("800000 100000"), "{}", output.stdout);
        eprintln!("Resized container limits and preserved data: {}", output.stdout);
        // At this size the kernel's reserved memory exceeds the former fixed
        // 256 MiB allowance, even when every hotplug block is already online.
        let larger = runtime.ensure_container_capacity(8.0, 36.0).await;
        eprintln!("Large hotplug result: {larger:?}; guest memory: {:?}", runtime.execute_container_command("env-capacity", "head -3 /proc/meminfo").await);
        larger?;
        runtime.ensure_container_capacity(8.0, 36.0).await?;
        assert_eq!(before, runtime.appliance.lock().await.as_ref().unwrap().child.id());
        runtime.provision_container("env-delete-live", "quay.io/libpod/alpine:latest", "sleep 2147483647", &policy, false, false).await?;
        runtime.container_action("env-delete-live", "start", false).await?;
        runtime.delete_container("env-delete-live").await?;
        runtime.delete_container("env-delete-live").await?; // Retry remains safe.
        let deleted = runtime.execute_container_command("env-delete-live", "true").await;
        assert!(deleted.is_err() || deleted.as_ref().is_ok_and(|output| output.exit_code != 0), "Deleted container accepted a command: {deleted:?}");
        assert!(!runtime.appliance.lock().await.as_ref().unwrap().active_containers.contains("env-delete-live"));
        assert_eq!(runtime.execute_container_command("env-capacity", "cat /root/capacity-marker").await?.stdout.trim(), "capacity-survives");
        Ok(())
    }.await;
    runtime.shutdown_all().await;
    result
}

#[tokio::test]
#[ignore = "pulls and boots real MongoDB, Redis and Nginx in an isolated appliance"]
async fn service_images_start_their_default_processes() -> Result<(), String> {
    let data = tempfile::tempdir().unwrap();
    let runtime = RuntimeManager::new(Path::new(env!("CARGO_MANIFEST_DIR")), data.path())?;
    let policy = ResourcePolicy {
        cpu: ResourceRange {
            min: 0.1,
            preferred: 1.0,
            max: 1.0,
            current: 0.0,
        },
        memory_gb: ResourceRange {
            min: 0.125,
            preferred: 0.5,
            max: 0.5,
            current: 0.0,
        },
        priority: Priority::Normal,
        dynamic: false,
    };
    let result = async {
        runtime
            .provision_container(
                "env-smoke-shell",
                "quay.io/libpod/alpine:latest",
                "sleep 2147483647",
                &policy,
                false,
                false,
            )
            .await?;
        runtime
            .container_action("env-smoke-shell", "start", false)
            .await?;
        let features = runtime
            .execute_container_command(
                "env-smoke-shell",
                "head -20 /proc/cpuinfo; head -3 /proc/meminfo",
            )
            .await?;
        eprintln!("Guest CPU/memory: {}", features.stdout);
        for (name, image, check) in [
            (
                "mongo",
                "docker.io/library/mongo:latest",
                "mongosh --quiet --eval 'db.adminCommand({ping:1}).ok'",
            ),
            ("redis", "docker.io/library/redis:alpine", "redis-cli ping"),
            (
                "nginx",
                "docker.io/library/nginx:alpine",
                "wget -qO- http://127.0.0.1/",
            ),
        ] {
            let id = format!("env-smoke-{name}");
            eprintln!("Pulling and creating {image}");
            runtime
                .provision_container(&id, image, "", &policy, false, false)
                .await?;
            eprintln!("Starting {name}");
            runtime.container_action(&id, "start", false).await?;
            let deadline = tokio::time::Instant::now() + Duration::from_secs(45);
            loop {
                let output = runtime.execute_container_command(&id, check).await;
                match output {
                    Ok(output) if output.exit_code == 0 => {
                        eprintln!("{name}: {}", output.stdout.trim());
                        break;
                    }
                    other if tokio::time::Instant::now() >= deadline => {
                        return Err(format!(
                            "{name} failed its service health check: {other:?}; {:?}",
                            runtime.container_failure_detail(&id).await
                        ))
                    }
                    _ => tokio::time::sleep(Duration::from_millis(1000)).await,
                }
            }
            runtime.delete_container(&id).await?;
        }
        Ok(())
    }
    .await;
    runtime.shutdown_all().await;
    result
}
