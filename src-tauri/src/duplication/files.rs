use super::*;
use crate::runtime::cloud::file_copy::FileArchive;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocalFiles {
    pub image: String,
    pub paths: Vec<String>,
    pub storage_gb: f64,
}

pub(super) fn validate(options: &LocalFiles) -> Result<(), String> {
    if !cloud::word(&options.image) {
        return Err("Choose a local OCI image or enter a valid registry reference".into());
    }
    if options.paths.is_empty()
        || options.paths.len() > 32
        || options.paths.iter().any(|path| {
            path.is_empty()
                || path.len() > 4096
                || path.chars().any(char::is_control)
                || !(path.starts_with('/') || path == "~" || path.starts_with("~/"))
        })
    {
        return Err("Choose 1–32 absolute cloud paths, or ~ for the SSH user's home folder".into());
    }
    if !options.storage_gb.is_finite()
        || options.storage_gb < 6.0
        || options.storage_gb > 16380.0
        || options.storage_gb.fract() != 0.0
    {
        return Err("Choose a whole-number local storage limit between 6 and 16380 GB".into());
    }
    Ok(())
}

fn staging(op: &Operation<'_>, runtime: &RuntimeManager) -> Result<PathBuf, String> {
    let selected = runtime.storage_runtime_on_drive(op.job.request.storage_drive.as_deref())?;
    let root = selected
        .as_deref()
        .unwrap_or(runtime)
        .storage_root()
        .join("duplications");
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let directory = root.join(&op.id);
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let canonical = directory.canonicalize().map_err(|e| e.to_string())?;
    if canonical.parent() != Some(root.as_path())
        || canonical.file_name() != Some(std::ffi::OsStr::new(&op.id))
    {
        return Err("The file-copy staging directory is outside managed storage".into());
    }
    Ok(canonical)
}

pub(super) fn cleanup(op: &mut Operation<'_>, runtime: &RuntimeManager) -> Result<(), String> {
    if op.job.status == "running" {
        return Err("Wait for this file copy to finish before discarding its staging files".into());
    }
    let directory = staging(op, runtime)?;
    for name in ["files.tar", "files.part"] {
        match std::fs::remove_file(directory.join(name)) {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(format!("Remove local transfer staging: {e}")),
        }
    }
    let _ = std::fs::remove_dir(directory);
    op.job.completed.insert("cleanup".into(), json!(true));
    op.job.error = None;
    op.save()
}

fn target_environment(op: &Operation<'_>, options: &LocalFiles) -> Result<Environment, String> {
    let host = op.store.snapshot()?.host;
    let cpu = (host.total_cpu as f64).min(2.0).max(1.0);
    let memory = host.total_memory_gb.min(2.0).max(0.5);
    serde_json::from_value(json!({
        "id":op.job.environment_id,"name":op.job.request.name,"kind":"container","provider":"yougoriOci",
        "runtimeId":op.job.environment_id,"runtime":options.image,"status":"provisioning",
        "containerCommand":"sleep 2147483647","networkAccess":false,"gpuAccess":false,
        "description":"Copying cloud files into a new local image.","createdAt":chrono::Utc::now().to_rfc3339(),
        "storageDrive":op.job.request.storage_drive,"storageLimitGb":options.storage_gb,
        "cpuUsage":0,"memoryUsageGb":0,"storageDeltaGb":0,"networkRxMbps":0,
        "resourcePolicy":{"cpu":{"min":cpu,"preferred":cpu,"max":cpu,"current":0},"memoryGb":{"min":memory,"preferred":memory,"max":memory,"current":0},"priority":"normal","dynamic":false}
    })).map_err(|e| e.to_string())
}

async fn receipt_matches(
    runtime: &RuntimeManager,
    id: &str,
    transfer: &str,
    expected: &Value,
) -> Result<bool, String> {
    if transfer.len() != 32 || !transfer.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Invalid saved local transfer ID".into());
    }
    let path = format!("/yougori-import-{transfer}/.yougori-copy-complete.json");
    let result = runtime.execute_container_command(id, &format!("if [ -f '{path}' ] && [ ! -L '{path}' ]; then head -c 4097 '{path}'; else printf %s YOUGORI_NO_COPY_RECEIPT; fi")).await?;
    if result.exit_code != 0 {
        return Err(format!("Cannot verify the local copy: {}", result.stderr));
    }
    Ok(serde_json::from_str::<Value>(&result.stdout).is_ok_and(|value| value == *expected))
}

pub(super) async fn run(op: &mut Operation<'_>, runtime: &RuntimeManager) -> Result<(), String> {
    let options = op
        .job
        .request
        .local_files
        .clone()
        .ok_or("Choose a local image and cloud folders")?;
    validate(&options)?;
    let directory = staging(op, runtime)?;
    let archive = directory.join("files.tar");
    let maximum = ((options.storage_gb - 2.0) * 1024f64.powi(3)) as u64;
    op.phase("Reading cloud files over verified SSH")?;
    let saved: Option<FileArchive> = op
        .job
        .completed
        .get("file-archive")
        .map(|v| serde_json::from_value(v.clone()))
        .transpose()
        .map_err(|e| e.to_string())?;
    let metadata = if let Some(saved) = saved.filter(|_| archive.is_file()) {
        runtime
            .verify_duplication_artifact(&archive, saved.archive_bytes, &saved.checksum)
            .await?;
        saved
    } else {
        if op.job.completed.contains_key("file-archive") {
            return Err("The staged cloud files are missing. Do not resume with different source contents; start a new copy.".into());
        }
        let copied = runtime
            .cloud
            .export_files(
                &op.job.request.environment_id,
                &options.paths,
                &op.id,
                &archive,
                maximum,
            )
            .await?;
        op.job.completed.insert(
            "file-archive".into(),
            serde_json::to_value(&copied).map_err(|e| e.to_string())?,
        );
        op.save()?;
        copied
    };
    let id = op.job.environment_id.clone();
    let lock = crate::commands::environment_network_lock(&id).await;
    let _guard = lock.lock().await;
    runtime.select_storage_drive(&id, op.job.request.storage_drive.as_deref())?;
    runtime.register_container_provider(&id, &RuntimeProviderKind::YougoriOci)?;
    let environment = if let Some(existing) = op
        .store
        .snapshot()?
        .environments
        .into_iter()
        .find(|e| e.id == id)
    {
        if existing.kind != EnvironmentKind::Container || existing.runtime != options.image {
            return Err("The destination no longer matches this file copy".into());
        }
        existing
    } else {
        if op.job.completed.contains_key("local-container") {
            return Err("The local destination was removed. Create a new file copy.".into());
        }
        let environment = target_environment(op, &options)?;
        op.store.mutate(|state| {
            if state
                .environments
                .iter()
                .any(|e| e.name.eq_ignore_ascii_case(&environment.name))
            {
                return Err("Another environment uses the destination name".into());
            }
            state.environments.push(environment.clone());
            Ok(())
        })?;
        op.resource(format!("Local container · {id}"))?;
        environment
    };
    op.phase("Preparing the selected local image")?;
    if !op.job.completed.contains_key("local-container") {
        let existing = if op.job.pending.as_deref() == Some("local-container") {
            match runtime.container_failure_detail(&id).await {
                Ok(_) => true,
                Err(error) if error.to_lowercase().contains("no such object") => false,
                Err(error) => {
                    return Err(format!("Inspect the interrupted local creation: {error}"))
                }
            }
        } else {
            false
        };
        op.job.pending = Some("local-container".into());
        op.save()?;
        if existing {
            runtime
                .set_container_storage(&id, options.storage_gb)
                .await?;
        } else {
            runtime
                .provision_container_with_storage(
                    &id,
                    &options.image,
                    "sleep 2147483647",
                    &environment.resource_policy,
                    false,
                    false,
                    options.storage_gb,
                )
                .await?;
        }
        op.job
            .completed
            .insert("local-container".into(), json!(true));
        op.job.pending = None;
        op.save()?;
    }
    let copied = async {
        if runtime.container_failure_detail(&id).await?.is_some() {
            runtime.container_action(&id, "start", false).await?;
        }
        op.phase("Copying files into the local environment")?;
        let prior = op
            .job
            .completed
            .get("file-transfer")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let transfer = if let Some(prior) = prior {
            if receipt_matches(runtime, &id, &prior, &metadata.receipt).await? {
                prior
            } else {
                // Keep an incomplete directory for inspection; never overwrite it or
                // silently remove a file the user may have edited after a failure.
                op.resource(format!("Incomplete local folder · /yougori-import-{prior}"))?;
                uuid::Uuid::new_v4().simple().to_string()
            }
        } else {
            uuid::Uuid::new_v4().simple().to_string()
        };
        op.job
            .completed
            .insert("file-transfer".into(), json!(transfer));
        op.save()?;
        if !receipt_matches(runtime, &id, &transfer, &metadata.receipt).await? {
            runtime
                .import_file_archive(
                    &environment,
                    &archive,
                    &transfer,
                    metadata.bytes,
                    None,
                    std::sync::Arc::new(|_| {}),
                )
                .await?;
            if !receipt_matches(runtime, &id, &transfer, &metadata.receipt).await? {
                return Err("The destination did not confirm the complete file copy".into());
            }
        }
        Ok::<String, String>(format!("/yougori-import-{transfer}"))
    }
    .await;
    let stopped = runtime.container_action(&id, "stop", false).await;
    let destination = copied?;
    stopped.map_err(|e| {
        format!(
            "Files copied, but the local container could not be stopped: {e}. Resume to finish."
        )
    })?;
    op.job
        .completed
        .insert("files-destination".into(), json!(destination));
    op.store.mutate(|state| {
        let destination_env = state.environments.iter_mut().find(|e| e.id == id).ok_or("Local destination disappeared")?;
        destination_env.status = EnvironmentStatus::Stopped;
        destination_env.last_error = None;
        destination_env.description = format!("Cloud files: {destination}. {} files copied; {} links, special files or nested mounts skipped. Apps and services must be installed separately.", metadata.files, metadata.skipped);
        Ok(())
    })?;
    op.phase(&format!(
        "Files copied to {destination} · {} files · {} skipped",
        metadata.files, metadata.skipped
    ))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn cloud_to_local_files_do_not_use_provider_credentials_or_disk_snapshots() {
        let environment: Environment = serde_json::from_value(json!({"id":"env-source","name":"Cloud","kind":"cloud","provider":"cloudSsh","status":"running","runtime":"SSH","description":"","createdAt":"now","cpuUsage":0,"memoryUsageGb":0,"storageDeltaGb":0,"networkRxMbps":0,"resourcePolicy":{"cpu":{"min":1,"preferred":1,"max":1,"current":0},"memoryGb":{"min":1,"preferred":1,"max":1,"current":0},"priority":"normal","dynamic":false}})).unwrap();
        let mut request: Request = serde_json::from_value(json!({"operationId":uuid::Uuid::new_v4().to_string(),"environmentId":"env-source","name":"Files","destination":"local","source":null,"target":null,"storageDrive":null,"reviewed":true,"localFiles":{"image":"alpine:3.24","paths":["~"],"storageGb":20}})).unwrap();
        assert!(validate_request(&request, &environment).is_ok());
        let cli: Box<dyn FnMut(&str, &[String]) -> Result<String, String>> =
            Box::new(|_, _| panic!("Cloud-to-local files must not call a cloud provider CLI"));
        TEST_CLI
            .scope(std::cell::RefCell::new(cli), preflight(&request))
            .await
            .unwrap();
        request.destination = "cloud".into();
        assert!(validate_request(&request, &environment).is_err());
        request.destination = "local".into();
        request.local_files = None;
        assert!(validate_request(&request, &environment)
            .unwrap_err()
            .contains("copies files over SSH"));
    }

    #[test]
    fn file_copy_requires_explicit_data_paths_and_a_bounded_image_destination() {
        let mut options = LocalFiles {
            image: "docker.io/library/alpine:3.24".into(),
            paths: vec!["~".into(), "/srv/my project".into()],
            storage_gb: 20.0,
        };
        assert!(validate(&options).is_ok());
        for path in ["", "relative/path", "/srv\n/secret"] {
            options.paths = vec![path.into()];
            assert!(validate(&options).is_err());
        }
        options.paths = vec!["~".into()];
        options.image = "--image".into();
        assert!(validate(&options).is_err());
        options.image = "alpine:3.24".into();
        options.storage_gb = f64::NAN;
        assert!(validate(&options).is_err());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "boots a disposable local container and verifies image preservation, file delivery and resume"]
    async fn cloud_files_real_local_image_copy_and_resume() -> Result<(), String> {
        use crate::runtime::cloud::file_copy::inspect_archive;
        let temp = tempfile::tempdir().map_err(|e| e.to_string())?;
        let runtime = RuntimeManager::new(Path::new(env!("CARGO_MANIFEST_DIR")), temp.path())?;
        let store = PlatformStore::load(temp.path().join("state.json"))?;
        let operation = uuid::Uuid::new_v4().to_string();
        let request: Request = serde_json::from_value(json!({"operationId":operation,"environmentId":"env-source","name":"File copy","destination":"local","source":null,"target":null,"storageDrive":null,"reviewed":true,"localFiles":{"image":"quay.io/libpod/alpine:latest","paths":["/etc","/home/project"],"storageGb":6}})).map_err(|e| e.to_string())?;
        let mut op = Operation {
            id: operation.clone(),
            store: &store,
            job: Job {
                request,
                environment_id: format!("env-{}", uuid::Uuid::new_v4()),
                status: "running".into(),
                phase: "test".into(),
                error: None,
                resources: vec![],
                completed: BTreeMap::new(),
                pending: None,
            },
        };
        let directory = staging(&op, &runtime)?;
        let path = directory.join("files.tar");
        let original = b"cloud image should not replace the local OS";
        let bytes = original.len() as u64 + 12;
        let receipt =
            json!({"operationId":operation,"bytes":bytes,"files":2,"skipped":1}).to_string();
        let file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
        let mut archive = tar::Builder::new(file);
        for (name, contents, kind) in [
            ("etc", &b""[..], b'5'),
            ("etc/os-release", &original[..], b'0'),
            ("project", &b""[..], b'5'),
            ("project/.hidden", &b"project data"[..], b'0'),
            ("project/empty", &b""[..], b'5'),
            (".yougori-copy-complete.json", receipt.as_bytes(), b'0'),
        ] {
            let mut header = tar::Header::new_gnu();
            header.set_size(contents.len() as u64);
            header.set_mode(0o755);
            header.set_entry_type(tar::EntryType::new(kind));
            header.set_cksum();
            archive
                .append_data(&mut header, name, contents)
                .map_err(|e| e.to_string())?;
        }
        archive.finish().map_err(|e| e.to_string())?;
        drop(archive);
        let metadata = inspect_archive(&path, &operation, 1024)?;
        op.job.completed.insert(
            "file-archive".into(),
            serde_json::to_value(metadata).unwrap(),
        );
        op.save()?;
        let result = async {
            run(&mut op, &runtime).await?;
            let id = op.job.environment_id.clone();
            let first_transfer = op.job.completed["file-transfer"].clone();
            let destination = op.job.completed["files-destination"].as_str().unwrap().to_owned();
            assert_eq!(store.snapshot()?.environments.len(), 1);
            assert_eq!(store.snapshot()?.environments[0].status, EnvironmentStatus::Stopped);
            // Simulate losing the final status response: resume the same job.
            run(&mut op, &runtime).await?;
            assert_eq!(op.job.completed["file-transfer"], first_transfer);
            assert_eq!(store.snapshot()?.environments.len(), 1);
            runtime.container_action(&id, "start", false).await?;
            let checked = runtime.execute_container_command(&id, &format!("grep -q Alpine /etc/os-release && test \"$(cat '{destination}/etc/os-release')\" = 'cloud image should not replace the local OS' && test \"$(cat '{destination}/project/.hidden')\" = 'project data' && test -d '{destination}/project/empty'")).await?;
            if checked.exit_code != 0 { return Err(format!("Local file-copy check failed: {}",checked.stderr)); }
            runtime.container_action(&id, "stop", false).await?;
            op.job.status = "complete".into(); cleanup(&mut op, &runtime)?;
            assert!(!path.exists());
            assert!(store.snapshot()?.environments[0].description.contains("1 links"));
            Ok(())
        }.await;
        runtime.shutdown_all().await;
        result
    }
}
