use super::*;

#[cfg(windows)]
#[test]
#[ignore = "boots disposable containers and a microVM, pulls a small OCI image, and exercises project reconciliation without using user environments"]
fn project_real_yaml_compose_changes_and_microvm() {
    run_case(false);
}
#[cfg(windows)]
#[test]
#[ignore = "uses only build/cuda/integration-runtime; downloads a public model and tests real GPU chat and authenticated localhost API"]
fn model_real_gpu_chat_and_api() {
    run_case(true);
}
#[cfg(windows)]
fn run_case(model: bool) {
    use crate::{backup::BackupManager, workspace::WorkspaceManager};
    use std::sync::{Arc, Mutex};
    let data = tempfile::tempdir().unwrap();
    let directory = tempfile::tempdir().unwrap();
    let mut runtime =
        RuntimeManager::new(Path::new(env!("CARGO_MANIFEST_DIR")), data.path()).unwrap();
    if model {
        runtime.use_dedicated_cuda_test_runtime().unwrap();
    }
    let store = PlatformStore::load(data.path().join("state.json")).unwrap();
    store
        .mutate(|s| {
            s.host = crate::commands::collect_host_metrics(&s.host, runtime.storage_root());
            s.providers = runtime.provider_statuses();
            Ok(())
        })
        .unwrap();
    let outcome = Arc::new(Mutex::new(None));
    let result = outcome.clone();
    let backup = BackupManager::new(data.path()).unwrap();
    let workspace = WorkspaceManager::new(data.path());
    let mut context = tauri::generate_context!();
    context.config_mut().identifier =
        format!("com.yougori.project-test-{}", uuid::Uuid::new_v4().simple());
    context.config_mut().app.windows.clear();
    let app = tauri::Builder::default()
        .any_thread()
        .manage(store)
        .manage(runtime)
        .manage(backup)
        .manage(workspace)
        .manage(crate::peer_sharing::Sharing::default())
        .setup(move |app| {
            let app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let test_app = app.clone();
                let path = directory.path().to_owned();
                let tested = tokio::spawn(async move {
                    if model {
                        exercise_model(&test_app).await
                    } else {
                        exercise(&test_app, &path).await
                    }
                })
                .await
                .map_err(|e| e.to_string())
                .and_then(|v| v);
                app.state::<WorkspaceManager>()
                    .shutdown(&app.state::<RuntimeManager>())
                    .await;
                app.state::<RuntimeManager>().shutdown_all().await;
                *result.lock().unwrap() = Some(tested);
                drop(directory);
                drop(data);
                app.exit(0);
            });
            Ok(())
        })
        .build(context)
        .unwrap();
    assert_eq!(app.run_return(|_, _| {}), 0);
    outcome
        .lock()
        .unwrap()
        .take()
        .expect("test outcome")
        .unwrap();
}
#[cfg(windows)]
async fn exercise_model(app: &AppHandle) -> Result<(), String> {
    use std::time::{Duration, Instant};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    drop(listener);
    eprintln!("Model integration: create the TinyLlama CUDA workload");
    let model = crate::model_runner::run_model(
        "hf.co/TinyLlama/TinyLlama-1.1B-Chat-v1.0".into(),
        Some(port),
        app.clone(),
    )
    .await?;
    let id = model["id"].as_str().ok_or("Missing model ID")?.to_owned();
    let result = async {
        let started = Instant::now();
        let mut last = String::new();
        loop {
            match crate::model_runner::model_status(id.clone(), app.clone()).await {
                Ok(status) => {
                    let phase = status["status"].as_str().unwrap_or("unknown");
                    if phase != last {
                        eprintln!("Model integration: {phase}");
                        last = phase.into();
                    }
                    if phase == "ready" {
                        break;
                    }
                    if phase == "error" {
                        return Err(status["error"]
                            .as_str()
                            .unwrap_or("Model load failed")
                            .to_owned());
                    }
                }
                Err(e) => {
                    if started.elapsed() > Duration::from_secs(120) {
                        return Err(e);
                    }
                }
            }
            if started.elapsed() > Duration::from_secs(1800) {
                return Err("Model readiness timed out".into());
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|e| e.to_string())?;
        let url = model["apiUrl"].as_str().unwrap();
        let denied = client
            .get(format!("{url}/models"))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        assert_eq!(denied.status(), 401);
        let authorized = client
            .get(format!("{url}/models"))
            .bearer_auth(model["apiKey"].as_str().unwrap())
            .send()
            .await
            .map_err(|e| e.to_string())?;
        assert_eq!(authorized.status(), 200);
        let answer = crate::model_runner::model_chat(
            id.clone(),
            json!([{"role":"user","content":"Reply with a short greeting."}]),
            None,
            None,
            app.clone(),
        )
        .await?;
        assert!(!answer["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .trim()
            .is_empty());
        eprintln!("Model integration: real GPU generation and API authentication passed");
        Ok(())
    }
    .await;
    let _ = status(app, &id, false).await;
    let _ = call(app, "delete_environment", json!({"environmentId":id})).await;
    result
}
#[cfg(windows)]
async fn exercise(app: &AppHandle, root: &Path) -> Result<(), String> {
    let shared = root.join("files");
    std::fs::create_dir(&shared).map_err(|e| e.to_string())?;
    std::fs::write(shared.join("source.txt"), "before\n").map_err(|e| e.to_string())?;
    let image = "quay.io/libpod/alpine:latest";
    let compose=format!("name: integration\nservices:\n  database:\n    image: {image}\n    cpus: 1\n    mem_limit: 512MB\n    command: [/bin/sh, -c, 'printf startup > /data/startup; exec sleep 2147483647']\n    volumes: [data:/data]\n  frontend:\n    image: {image}\n    cpus: 1\n    mem_limit: 512MB\n    command: [sleep, '2147483647']\n    environment: {{VALUE: 'literal space and quote'}}\n    depends_on: [database]\n    volumes: ['./files:/workspace']\nvolumes: {{data: {{}}}}\n");
    let compose_path = root.join("compose.yaml");
    std::fs::write(&compose_path, compose).map_err(|e| e.to_string())?;
    eprintln!("Project integration: translate Compose and reject overwrite");
    import_compose(
        compose_path.to_string_lossy().into_owned(),
        true,
        None,
        app.clone(),
    )
    .await?;
    assert!(import_compose(
        compose_path.to_string_lossy().into_owned(),
        true,
        None,
        app.clone()
    )
    .await
    .is_err());
    let file = root.join("yougori.yaml");
    let path = file.to_string_lossy().into_owned();
    // Keep integration disk allocations modest.
    let mut p = manifest::parse(&manifest::read(&file)?)?;
    for e in p.environments.values_mut() {
        e.storage = json!(6);
        e.internet = false;
    }
    let host_listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let host_port = host_listener
        .local_addr()
        .map_err(|e| e.to_string())?
        .port();
    drop(host_listener);
    p.environments
        .get_mut("database")
        .unwrap()
        .ports
        .push(manifest::Port::Mapping(format!("{host_port}:8080")));
    std::fs::write(&file, manifest::to_yaml(&p)?).map_err(|e| e.to_string())?;
    eprintln!("Project integration: create nodes with variables, named volume and PC bind mount");
    let result = project_action(path.clone(), "up".into(), app.clone()).await?;
    let front = result["environments"]["frontend"].as_str().unwrap();
    let db = result["environments"]["database"].as_str().unwrap();
    let exec = |id: &str, command: &str| json!({"request":{"environmentId":id,"command":command}});
    let server = r#"printf '%s\n' '#!/bin/sh' 'while IFS= read -r header; do [ "$header" = "$(printf "\r")" ] && break; done' 'printf "HTTP/1.0 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nready"' 'cat >/dev/null' > /tmp/project-http; chmod 700 /tmp/project-http; sh -c 'while true; do nc -l -p 8080 -e /tmp/project-http; done' </dev/null >/tmp/project-http.log 2>&1 &"#;
    call(app, "execute_environment_command", exec(db, server)).await?;
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .get(format!("http://127.0.0.1:{host_port}"))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .text()
        .await
        .map_err(|e| e.to_string())?;
    assert_eq!(response, "ready");
    let verified = call(
        app,
        "execute_environment_command",
        exec(
            front,
            "printf '%s\\n' \"$VALUE\"; cat /workspace/source.txt; wget -qO- http://database:8080",
        ),
    )
    .await?;
    assert_eq!(verified["exitCode"], 0, "{verified}");
    assert!(
        verified["stdout"]
            .as_str()
            .unwrap()
            .contains("literal space and quote\nbefore\nready"),
        "{verified}"
    );
    let before =
        crate::changes::environment_changes(front.into(), false, None, app.clone()).await?;
    assert!(before["files"].as_array().unwrap().is_empty());
    call(
        app,
        "execute_environment_command",
        exec(
            front,
            "printf after > /workspace/source.txt; printf new > /workspace/created.txt",
        ),
    )
    .await?;
    let diff = crate::changes::environment_changes(front.into(), false, None, app.clone()).await?;
    assert_eq!(diff["summary"]["modified"], 1);
    assert_eq!(diff["summary"]["created"], 1);
    call(
        app,
        "execute_environment_command",
        exec(db, "printf persistent > /data/marker"),
    )
    .await?;
    eprintln!("Project integration: idempotent apply and variable updates preserve data");
    let connection_ids = app
        .state::<PlatformStore>()
        .snapshot()?
        .connections
        .into_iter()
        .map(|c| c.id)
        .collect::<Vec<_>>();
    let again = project_action(path.clone(), "apply".into(), app.clone()).await?;
    assert_eq!(again["environments"], result["environments"]);
    assert_eq!(
        app.state::<PlatformStore>()
            .snapshot()?
            .connections
            .into_iter()
            .map(|c| c.id)
            .collect::<Vec<_>>(),
        connection_ids
    );
    p.environments
        .get_mut("frontend")
        .unwrap()
        .environment
        .insert("VALUE".into(), "updated".into());
    std::fs::write(&file, manifest::to_yaml(&p)?).map_err(|e| e.to_string())?;
    project_action(path.clone(), "apply".into(), app.clone()).await?;
    let updated = call(
        app,
        "execute_environment_command",
        exec(front, "printf '%s' \"$VALUE\"; cat /workspace/source.txt"),
    )
    .await?;
    assert_eq!(updated["stdout"], "updatedafter");
    let kept = call(
        app,
        "execute_environment_command",
        exec(db, "cat /data/marker"),
    )
    .await?;
    assert_eq!(kept["stdout"], "persistent");
    eprintln!("Project integration: removing a declared port revokes only its publication");
    p.environments.get_mut("database").unwrap().ports.clear();
    std::fs::write(&file, manifest::to_yaml(&p)?).map_err(|e| e.to_string())?;
    project_action(path.clone(), "apply".into(), app.clone()).await?;
    let ports = call(
        app,
        "list_environment_services",
        json!({"environmentId":db}),
    )
    .await?;
    assert!(
        ports["publications"].as_array().unwrap().is_empty(),
        "{ports}"
    );
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(
        tokio::net::TcpStream::connect(("127.0.0.1", host_port))
            .await
            .is_err(),
        "Removed project port still accepts connections"
    );
    project_action(path.clone(), "down".into(), app.clone()).await?;
    assert!(app
        .state::<PlatformStore>()
        .snapshot()?
        .environments
        .iter()
        .all(|e| e.status == EnvironmentStatus::Stopped));
    eprintln!("Project integration: successful one-shot OCI commands exit normally");
    let run = yougori_cli::public::parse_run(
        &[
            "--name".into(),
            "one-shot".into(),
            "--storage".into(),
            "6GB".into(),
            "--memory".into(),
            "512MB".into(),
            image.into(),
            "echo".into(),
            "done".into(),
        ],
        false,
    )?;
    let one = run_workload(run.request, true, app.clone()).await?;
    let refreshed = call(app, "refresh_host_metrics", json!({})).await?;
    let env = refreshed["environments"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == one["id"])
        .unwrap();
    assert_eq!(env["status"], "stopped", "{env}");
    assert!(env["lastError"].is_null());
    eprintln!("Project integration: OCI image inside a dedicated microVM");
    let args = vec![
        "--name".into(),
        "micro-oci".into(),
        "--isolation".into(),
        "microvm".into(),
        "--memory".into(),
        "1GB".into(),
        "--storage".into(),
        "6GB".into(),
        image.into(),
        "sleep".into(),
        "2147483647".into(),
    ];
    let run = yougori_cli::public::parse_run(&args, false)?;
    let micro = run_workload(run.request, true, app.clone()).await?;
    let micro_id = micro["id"].as_str().unwrap();
    let check = call(
        app,
        "execute_environment_command",
        exec(
            micro_id,
            "cat /etc/os-release; printf isolated > /root/marker",
        ),
    )
    .await?;
    assert_eq!(check["exitCode"], 0, "{check}");
    assert!(check["stdout"].as_str().unwrap().contains("Alpine"));
    status(app, micro_id, false).await?;
    status(app, micro_id, true).await?;
    let check = call(
        app,
        "execute_environment_command",
        exec(micro_id, "cat /root/marker"),
    )
    .await?;
    assert_eq!(check["stdout"], "isolated");
    status(app, micro_id, false).await?;
    eprintln!(
        "Project integration passed: all data is disposable and the test runtime will shut down"
    );
    Ok(())
}


#[test]
fn project_cloud_identity_resolves_file_names_with_spaces_and_preserves_public_keys() {
    use base64::Engine;
    let directory = tempfile::tempdir().unwrap();
    let resolve = |identity: &str| {
        let project: Project = serde_json::from_value(json!({
            "project": "identity-test",
            "environments": {"server": {"type": "cloud", "cloud": {"identityFile": identity}}}
        })).unwrap();
        resolve_spec(project, directory.path())
    };
    let relative = "My Keys/cloud identity.pem";
    let resolved = resolve(relative).unwrap();
    assert_eq!(resolved.environments["server"].cloud.as_ref().unwrap()["identityFile"], directory.path().join(relative).to_string_lossy().as_ref());
    let absolute = directory.path().join("absolute key.pem").to_string_lossy().into_owned();
    assert_eq!(resolve(&absolute).unwrap().environments["server"].cloud.as_ref().unwrap()["identityFile"], absolute);
    let kind = "ssh-ed25519";
    let mut bytes = (kind.len() as u32).to_be_bytes().to_vec();
    bytes.extend_from_slice(kind.as_bytes());
    bytes.extend_from_slice(&[0, 0, 0, 32]);
    bytes.extend_from_slice(&[7; 32]);
    let public_key = format!("{kind} {} project key", base64::engine::general_purpose::STANDARD.encode(bytes));
    assert_eq!(resolve(&public_key).unwrap().environments["server"].cloud.as_ref().unwrap()["identityFile"], public_key);
    assert!(resolve("ssh-ed25519 invalid! comment").is_err());
}
