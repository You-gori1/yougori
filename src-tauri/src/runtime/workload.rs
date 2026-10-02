use super::RuntimeManager;
use yougori_cli::workload::Options;
impl RuntimeManager {
    pub fn save_micro_workload(&self, id: &str, spec: &serde_json::Value) -> Result<(), String> {
        let directory = self
            .environment_storage_root(id)?
            .join("environments")
            .join(id);
        std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
        let mut f = tempfile::NamedTempFile::new_in(&directory).map_err(|e| e.to_string())?;
        serde_json::to_writer(&mut f, spec).map_err(|e| e.to_string())?;
        f.as_file().sync_all().map_err(|e| e.to_string())?;
        f.persist(directory.join("oci-workload.json"))
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn is_micro_workload(&self, id: &str) -> Result<bool, String> {
        Ok(self
            .environment_storage_root(id)?
            .join("environments")
            .join(id)
            .join("oci-workload.json")
            .is_file())
    }
    pub async fn start_micro_workload(&self, id: &str) -> Result<(), String> {
        if let Some(engine) = self.storage_runtime(id)? {
            return Box::pin(engine.start_micro_workload(id)).await;
        }
        let path = self
            .data_root
            .join("environments")
            .join(id)
            .join("oci-workload.json");
        if !path.is_file() {
            return Ok(());
        }
        if std::fs::metadata(&path).map_err(|e| e.to_string())?.len() > 256 * 1024 {
            return Err("Invalid microVM workload metadata".into());
        }
        let mut spec: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        let options: Options = serde_json::from_value(spec["options"].clone()).map_err(|_| "Invalid microVM workload options")?;
        crate::projects::secrets::inject(&mut spec["options"], &options.secret_environment)?;
        // Existing helper waits for the guest agent to finish booting.
        self.execute_micro_vm_command(id, "true").await?;
        let endpoint = self
            .vms
            .lock()
            .await
            .get(id)
            .and_then(|v| v.micro_endpoint.clone())
            .ok_or("MicroVM agent unavailable")?;
        let response = self
            .client
            .post(format!("{}/v1/microvm/workload", endpoint.base_url))
            .bearer_auth(endpoint.token)
            .json(&spec)
            .timeout(std::time::Duration::from_secs(1250))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            return Err(format!(
                "MicroVM workload: {}",
                response.text().await.unwrap_or_default()
            ));
        }
        Ok(())
    }
    pub async fn execute_micro_workload_command(
        &self,
        id: &str,
        command: &str,
    ) -> Result<crate::models::CommandResult, String> {
        if self.is_micro_workload(id)? {
            self.execute_micro_vm_command(
                id,
                &format!(
                    "nerdctl --namespace yougori-workload exec app /bin/sh -lc '{}'",
                    command.replace('\'', "'\"'\"'")
                ),
            )
            .await
        } else {
            self.execute_micro_vm_command(id, command).await
        }
    }
    pub async fn update_workload_configuration(
        &self,
        env: &crate::models::Environment,
    ) -> Result<(), String> {
        self.update_container_configuration(
            env.runtime_id.as_deref().unwrap_or(&env.id),
            env.network_access,
            env.gpu_access,
            env.network_access,
            env.gpu_access,
            env.container_command.as_deref().unwrap_or_default(),
            &env.resource_policy,
        )
        .await
    }
    pub async fn prepare_workload_binds(&self, id: &str) -> Result<serde_json::Value, String> {
        if let Some(engine) = self.storage_runtime(id)? {
            return Box::pin(engine.prepare_workload_binds(id)).await;
        }
        let options = self.workload_options(id)?;
        let mut value = serde_json::to_value(&options).map_err(|e| e.to_string())?;
        crate::projects::secrets::inject(&mut value, &options.secret_environment)?;
        if options.binds.is_empty() {
            self.workload_shares.lock().await.remove(id);
            return Ok(value);
        }
        self.container_endpoint(id).await?;
        let mut retained = self.workload_shares.lock().await;
        let cuda = self.container_provider(id)? == crate::models::RuntimeProviderKind::YougoriCuda;
        let mut servers = Vec::new();
        // Reuse live servers but always restore guest mounts after an engine restart.
        let fingerprint = serde_json::to_string(&options.binds).map_err(|e| e.to_string())?;
        if let Some((old_key, old)) = retained.remove(id) {
            if old_key == fingerprint {
                servers = old
            }
        }
        let result=async {
            for (index,bind) in options.binds.iter().enumerate(){
                let root=std::path::Path::new(&bind.source).canonicalize().map_err(|e|format!("PC volume {}: {e}",bind.source))?;
                if !root.is_dir()||root.parent().is_none()||root.starts_with(&self.data_root)||self.data_root.starts_with(&root){return Err("Choose a project folder outside Yougori runtime storage".into())}
                if let Err(error)=crate::changes::ensure_folder_baseline(self,id,&root).await { eprintln!("Changes baseline for {id}: {error}"); }
                if servers.len()<=index{servers.push(crate::host_files::HostFolderServer::start(root,bind.read_only).await?)}
                let server=&servers[index];let endpoint=self.workload_folder_endpoint(id,cuda,server).await?;
                let slot=format!("{id}-{index}");
                let _:serde_json::Value=self.agent_post("/v1/workloads/mount",&serde_json::json!({"id":id,"slot":slot,"endpoint":endpoint,"token":server.token,"readOnly":bind.read_only})).await?;
                value["binds"][index]["source"]=slot.into();
            }Ok(value)
        }.await;
        retained.insert(id.to_owned(), (fingerprint, servers));
        result
    }
    pub fn save_workload_options(&self, id: &str, options: &Options) -> Result<(), String> {
        options.validate()?;
        let root = self.environment_storage_root(id)?.join("workload-options");
        std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let mut file = tempfile::NamedTempFile::new_in(&root).map_err(|e| e.to_string())?;
        serde_json::to_writer(&mut file, options).map_err(|e| e.to_string())?;
        file.as_file().sync_all().map_err(|e| e.to_string())?;
        file.persist(root.join(format!("{id}.json")))
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn workload_options(&self, id: &str) -> Result<Options, String> {
        let path = self
            .environment_storage_root(id)?
            .join("workload-options")
            .join(format!("{id}.json"));
        if !path.try_exists().map_err(|e| e.to_string())? {
            return Ok(Options::default());
        }
        if std::fs::metadata(&path).map_err(|e| e.to_string())?.len() > 256 * 1024 {
            return Err("Invalid workload metadata".into());
        }
        let options: Options =
            serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        options.validate()?;
        Ok(options)
    }
}
