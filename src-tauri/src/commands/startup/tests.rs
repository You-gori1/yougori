use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "boots a disposable container to verify startup edits and original image defaults preserve files"]
async fn startup_command_changes_run_and_preserve_container_data() -> Result<(), String> {
    let data = tempfile::tempdir().unwrap();
    let store_path = data.path().join("state.json");
    let store = PlatformStore::load(store_path.clone())?;
    let runtime = RuntimeManager::new(std::path::Path::new(env!("CARGO_MANIFEST_DIR")), data.path())?;
    let id = format!("env-startup-{}", Uuid::new_v4().simple());
    let env: Environment = serde_json::from_value(serde_json::json!({
        "id":id,"name":"Startup fixture","kind":"container","provider":"yougoriOci","status":"stopped",
        "runtime":"quay.io/libpod/alpine:latest","containerCommand":"exec sleep 2147483647","description":"","createdAt":"test","cpuUsage":0,"memoryUsageGb":0,"storageDeltaGb":0,"networkRxMbps":0,
        "resourcePolicy":{"cpu":{"min":0.5,"preferred":0.5,"max":1,"current":0},"memoryGb":{"min":0.5,"preferred":0.5,"max":1,"current":0},"priority":"normal","dynamic":true}
    })).unwrap();
    store.mutate(|state| { state.environments = vec![env.clone()]; Ok(()) })?;
    let result = async {
        runtime.provision_container(&id, &env.runtime, env.container_command.as_deref().unwrap(), &env.resource_policy, false, false).await?;
        runtime.container_action(&id, "start", false).await?;
        let created = runtime.execute_container_command(&id, "printf keep > /root/preserved && dd if=/dev/zero of=/root/large-project bs=1M count=128 2>/dev/null && hostname && stat -c '%i:%s' /root/preserved /root/large-project").await?;
        assert_eq!(created.exit_code, 0);
        store.mutate(|state| { state.environments[0].status = EnvironmentStatus::Running; Ok(()) })?;
        assert!(update(&id, "echo changed", &store, &runtime).await.unwrap_err().contains("Stop"));
        runtime.container_action(&id, "stop", false).await?;
        store.mutate(|state| { state.environments[0].status = EnvironmentStatus::Stopped; Ok(()) })?;
        let command = "printf changed >> /root/startup-runs; exec sleep 2147483647";
        timed_update(&id, command, &store, &runtime).await?;
        assert_eq!(PlatformStore::load(store_path)?.snapshot()?.environments[0].container_command.as_deref(), Some(command));
        for _ in 0..2 {
            runtime.container_action(&id, "start", false).await?;
            let output = runtime.execute_container_command(&id, "cat /root/preserved /root/startup-runs").await?;
            assert_eq!(output.exit_code, 0);
            assert!(output.stdout.starts_with("keepchanged"));
            let identity = runtime.execute_container_command(&id, "hostname && stat -c '%i:%s' /root/preserved /root/large-project").await?;
            assert_eq!(identity.exit_code, 0);
            assert_eq!(identity.stdout, created.stdout, "Saving startup recreated the container or copied its files");
            runtime.container_action(&id, "stop", false).await?;
        }
        timed_update(&id, "", &store, &runtime).await?;
        assert!(store.snapshot()?.environments[0].container_command.is_none());
        if let Err(error) = runtime.container_action(&id, "start", false).await {
            // An image default that exits immediately may already be gone
            // when the agent applies its post-start network isolation.
            if !error.contains("container is not running") { return Err(error); }
        }
        // Alpine's default is an unattached shell, so it exits instead of
        // running the previous override. Give the guest time to reap it.
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert!(runtime.container_failure_detail(&id).await?.is_some(), "Clearing startup retained the previous long-running override");
        runtime.container_action(&id, "stop", false).await?;
        timed_update(&id, "exec sleep 2147483647", &store, &runtime).await?;
        runtime.container_action(&id, "start", false).await?;
        let output = runtime.execute_container_command(&id, "cat /root/preserved /root/startup-runs").await?;
        assert_eq!(output.stdout, "keepchangedchanged");
        assert_eq!(output.exit_code, 0);
        Ok(())
    }.await;
    runtime.shutdown_all().await;
    result
}

async fn timed_update(id: &str, command: &str, store: &PlatformStore, runtime: &RuntimeManager) -> Result<(), String> {
    let started = std::time::Instant::now();
    update(id, command, store, runtime).await?;
    let elapsed = started.elapsed();
    eprintln!("Startup save: {} ms", elapsed.as_millis());
    if elapsed > Duration::from_secs(5) {
        return Err(format!("Startup save took {elapsed:?}; changing launch arguments must not copy the filesystem"));
    }
    Ok(())
}
