use super::RuntimeManager;
use crate::{
    file_import::CopyProgress,
    models::{Environment, EnvironmentKind},
};
use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[cfg(test)]
mod integration;

impl RuntimeManager {
    pub async fn import_file_archive(
        &self,
        environment: &Environment,
        archive: &Path,
        transfer: &str,
        expected_bytes: u64,
        destination_folder: Option<&str>,
        progress: Arc<impl Fn(CopyProgress) + Send + Sync + 'static>,
    ) -> Result<String, String> {
        if let Some(engine) = self.storage_runtime(environment.runtime_id.as_deref().unwrap_or(&environment.id))? { return Box::pin(engine.import_file_archive(environment, archive, transfer, expected_bytes, destination_folder, progress)).await; }

        if environment.kind == EnvironmentKind::FullVm {
            return Err("Use an imported-files drive for full VMs".into());
        }
        let _appliance = self.appliance_operations.read().await;
        let id = environment.runtime_id.as_deref().unwrap_or(&environment.id);
        let vm_lock = self.vm_lifecycle_mutex(id).await;
        let _vm = vm_lock.lock().await;
        let (base, token) = self.workspace_endpoint(environment).await?;
        let mut input = tokio::fs::File::open(archive)
            .await
            .map_err(|e| e.to_string())?;
        let total = input.metadata().await.map_err(|e| e.to_string())?.len();
        let (mut sender, receiver) = tokio::io::duplex(256 * 1024);
        let report = progress.clone();
        let reader = tokio::spawn(async move {
            let mut buffer = vec![0; 128 * 1024];
            let mut copied = 0;
            let mut last = Instant::now();
            report(CopyProgress {
                phase: "copying",
                completed_bytes: 0,
                total_bytes: total,
                scanned_entries: None,
            });
            loop {
                let count = input.read(&mut buffer).await?;
                if count == 0 {
                    break;
                }
                sender.write_all(&buffer[..count]).await?;
                copied += count as u64;
                if last.elapsed().as_millis() >= 150 {
                    report(CopyProgress {
                        phase: "copying",
                        completed_bytes: copied,
                        total_bytes: total,
                        scanned_entries: None,
                    });
                    last = Instant::now();
                }
            }
            sender.shutdown().await?;
            report(CopyProgress {
                phase: "finishing",
                completed_bytes: total,
                total_bytes: total,
                scanned_entries: None,
            });
            Ok::<_, std::io::Error>(())
        });
        let mut url =
            url::Url::parse(&format!("{base}/v1/files/import")).map_err(|e| e.to_string())?;
        url.query_pairs_mut()
            .append_pair("id", id)
            .append_pair("transfer", transfer)
            .append_pair("folder", destination_folder.unwrap_or(""));
        let reply = self
            .client
            .post(url)
            .bearer_auth(token)
            .header(reqwest::header::CONTENT_TYPE, "application/x-tar")
            .header(reqwest::header::CONTENT_LENGTH, total)
            .body(reqwest::Body::wrap_stream(
                tokio_util::io::ReaderStream::new(receiver),
            ))
            .timeout(Duration::from_secs(24 * 60 * 60))
            .send()
            .await;
        reader.abort();
        let _ = reader.await;
        let mut reply = reply.map_err(|e| format!("File transfer interrupted: {e}"))?;
        let status = reply.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err("This environment uses an older guest agent. Restart it after updating Yougori, then drop the files again.".into());
        }
        let mut body = Vec::new();
        while let Some(chunk) = reply.chunk().await.map_err(|e| e.to_string())? {
            if body.len() + chunk.len() > 64 * 1024 {
                return Err("The guest returned an oversized copy response".into());
            }
            body.extend_from_slice(&chunk);
        }
        let result: serde_json::Value = serde_json::from_slice(&body)
            .map_err(|_| "The guest did not return a valid copy result")?;
        if !status.is_success() {
            return Err(result["error"]
                .as_str()
                .unwrap_or("File transfer failed")
                .to_string());
        }
        let expected = format!("{}/yougori-import-{transfer}", destination_folder.unwrap_or("").trim_end_matches('/'));
        if result["destination"].as_str() != Some(&expected) {
            return Err("The guest returned an unexpected copy destination".into());
        }
        if result["bytes"].as_u64() != Some(expected_bytes) {
            return Err(format!(
                "Copy incomplete at {expected}: the guest did not confirm all file contents"
            ));
        }
        Ok(expected)
    }
}
