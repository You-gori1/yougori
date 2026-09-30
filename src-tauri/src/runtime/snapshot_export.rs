use super::{appliance::{successful_response, SnapshotArtifact}, RuntimeManager};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{self, BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tempfile::NamedTempFile;
use tokio::io::AsyncWriteExt;
use tokio_util::io::SyncIoBridge;

const MAGIC: &[u8] = b"YOUGORI-SNAPSHOT\0\x01";
const CONTENT_TYPE: &str = "application/vnd.yougori.snapshot.v1";
const SPACE_RESERVE: u64 = 2 * 1024 * 1024 * 1024;

impl RuntimeManager {
    pub(super) async fn stream_container_snapshot(&self, id: &str, snapshot_id: &str) -> Result<SnapshotArtifact, String> {
        self.register_snapshot_provider(snapshot_id, &self.container_provider(id)?)?;
        let directory = self.data_root.join("snapshots");
        check_space(&directory)?;
        let file = NamedTempFile::new_in(&directory).map_err(|e| format!("Create snapshot file: {e}"))?;
        let tag = format!("opendock.local/snapshots:{}", snapshot_id.to_lowercase());
        let _lease = self.appliance_operations.read().await;
        let endpoint = self.container_endpoint(id).await?;
        let response = self.client.post(format!("{}/v1/snapshots/export", endpoint.base_url))
            .bearer_auth(endpoint.token)
            .json(&json!({"id": id, "snapshotId": snapshot_id}))
            .timeout(Duration::from_secs(24 * 60 * 60))
            .send().await.map_err(|e| format!("Start snapshot export: {e}"))?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err("This container runtime needs the updated guest agent. Restart Yougori, then retry the snapshot or backup.".into());
        }
        let mut response = successful_response(response).await?;
        if response.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()) != Some(CONTENT_TYPE) {
            return Err("The runtime returned an unsupported snapshot stream".into());
        }
        // One bounded network buffer and one host archive. Guest files are
        // never extracted on the host or duplicated inside the container disk.
        let (mut sender, receiver) = tokio::io::duplex(512 * 1024);
        let archive_tag = tag.clone();
        let worker = tokio::task::spawn_blocking(move || {
            build_archive(SyncIoBridge::new(receiver), file, &directory, &archive_tag)
        });
        let mut transfer_error = None;
        loop {
            match response.chunk().await {
                Ok(Some(chunk)) => if sender.write_all(&chunk).await.is_err() { break; },
                Ok(None) => break,
                Err(error) => { transfer_error = Some(error.to_string()); break; }
            }
        }
        drop(sender);
        drop(response); // Cancels the exporter if the disk writer failed.
        let (file, size_bytes, checksum_sha256) = worker.await
            .map_err(|e| format!("Snapshot writer stopped: {e}"))??;
        if let Some(error) = transfer_error { return Err(format!("Snapshot transfer was interrupted: {error}")); }
        let path = self.data_root.join("snapshots").join(format!("{snapshot_id}.oci.tar"));
        file.persist_noclobber(&path).map_err(|e| format!("Finalize snapshot: {}", e.error))?;
        Ok(SnapshotArtifact { provider_snapshot_id: tag, path, size_bytes, checksum_sha256 })
    }
}

fn check_space(directory: &Path) -> Result<(), String> {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let disk = super::storage::runtime_disk(&disks, directory).ok_or("Cannot determine free space on the snapshot drive")?;
    if disk.available_space() < SPACE_RESERVE + 16 * 1024 * 1024 {
        return Err("Not enough free space on the computer's snapshot drive. Free space there and retry; Yougori keeps 2 GB available for the computer. The container and its files were kept.".into());
    }
    Ok(())
}

struct LayerReceiver<R> {
    input: R,
    output: BufWriter<File>,
    hash: Sha256,
    bytes: u64,
    directory: PathBuf,
    checked_at: Instant,
    checked_bytes: u64,
}

impl<R: Read> Read for LayerReceiver<R> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        let n = self.input.read(bytes)?;
        if n == 0 { return Ok(0); }
        if self.bytes - self.checked_bytes >= 64 * 1024 * 1024 || self.checked_at.elapsed() >= Duration::from_secs(3) {
            check_space(&self.directory).map_err(io::Error::other)?;
            self.checked_at = Instant::now();
            self.checked_bytes = self.bytes;
        }
        self.output.write_all(&bytes[..n])?;
        self.hash.update(&bytes[..n]);
        self.bytes += n as u64;
        Ok(n)
    }
}

// Reserve one tar header, stream the compressed layer once, then fill in its
// final name/size and append OCI metadata. No second large host staging file.
fn build_archive<R: Read>(mut input: R, file: NamedTempFile, directory: &Path, tag: &str) -> Result<(NamedTempFile, u64, String), String> {
    let mut magic = vec![0; MAGIC.len()];
    input.read_exact(&mut magic).map_err(|e| format!("Incomplete snapshot header: {e}"))?;
    if magic != MAGIC { return Err("Invalid snapshot stream version".into()); }
    let mut length = [0u8; 4];
    input.read_exact(&mut length).map_err(|e| e.to_string())?;
    let length = u32::from_be_bytes(length) as usize;
    if length == 0 || length > 1024 * 1024 { return Err("Invalid snapshot metadata length".into()); }
    let mut metadata = vec![0; length];
    input.read_exact(&mut metadata).map_err(|e| e.to_string())?;
    let mut image: Value = serde_json::from_slice(&metadata).map_err(|e| format!("Invalid snapshot metadata: {e}"))?;
    if !image["config"].is_object() || image["architecture"].as_str().is_none_or(str::is_empty) || image["os"].as_str().is_none_or(str::is_empty) {
        return Err("Snapshot is missing its image configuration or platform".into());
    }
    let mut output = BufWriter::with_capacity(256 * 1024, file.reopen().map_err(|e| e.to_string())?);
    output.write_all(&[0u8; 512]).map_err(|e| e.to_string())?;
    let receiver = LayerReceiver { input, output, hash: Sha256::new(), bytes: 0, directory: directory.to_owned(), checked_at: Instant::now(), checked_bytes: 0 };
    let mut decoder = flate2::bufread::GzDecoder::new(BufReader::with_capacity(128 * 1024, receiver));
    let mut raw_hash = Sha256::new();
    let mut raw_bytes = 0u64;
    let mut buffer = vec![0; 128 * 1024];
    loop {
        let n = decoder.read(&mut buffer).map_err(|e| format!("Snapshot export did not finish or could not be saved: {e}. No partial snapshot was kept."))?;
        if n == 0 { break; }
        raw_hash.update(&buffer[..n]);
        raw_bytes += n as u64;
    }
    let mut buffered = decoder.into_inner();
    if !buffered.fill_buf().map_err(|e| e.to_string())?.is_empty() || raw_bytes < 1024 || raw_bytes % 512 != 0 {
        return Err("Snapshot export returned an incomplete or invalid filesystem archive".into());
    }
    let receiver = buffered.into_inner();
    let compressed_digest = hex::encode(receiver.hash.finalize());
    let compressed_bytes = receiver.bytes;
    let mut output = receiver.output.into_inner().map_err(|e| e.to_string())?;
    let layer_header = tar_header(&format!("blobs/sha256/{compressed_digest}"), compressed_bytes)?;
    output.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    output.write_all(layer_header.as_bytes()).map_err(|e| e.to_string())?;
    output.seek(SeekFrom::End(0)).map_err(|e| e.to_string())?;
    output.write_all(&vec![0; ((512 - compressed_bytes % 512) % 512) as usize]).map_err(|e| e.to_string())?;
    image["rootfs"] = json!({"type":"layers", "diff_ids":[format!("sha256:{}", hex::encode(raw_hash.finalize()))]});
    // Flattening produces one complete filesystem layer; discard old history.
    image["history"] = json!([{"created":image["created"],"created_by":"Yougori snapshot"}]);
    let mut archive = tar::Builder::new(&mut output);
    let config = append_blob(&mut archive, &image, "application/vnd.oci.image.config.v1+json")?;
    let manifest = append_blob(&mut archive, &json!({
        "schemaVersion":2, "mediaType":"application/vnd.oci.image.manifest.v1+json", "config":config,
        "layers":[{"mediaType":"application/vnd.oci.image.layer.v1.tar+gzip","digest":format!("sha256:{compressed_digest}"),"size":compressed_bytes}]
    }), "application/vnd.oci.image.manifest.v1+json")?;
    let mut manifest = manifest;
    manifest["annotations"] = json!({"io.containerd.image.name":tag,"org.opencontainers.image.ref.name":tag});
    append_json(&mut archive, "index.json", &json!({"schemaVersion":2,"manifests":[manifest]}))?;
    append_json(&mut archive, "oci-layout", &json!({"imageLayoutVersion":"1.0.0"}))?;
    archive.finish().map_err(|e| e.to_string())?;
    drop(archive);
    output.sync_all().map_err(|e| format!("Flush snapshot: {e}"))?;
    output.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let size = output.metadata().map_err(|e| e.to_string())?.len();
    let mut hash = Sha256::new();
    loop {
        let n = output.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 { break; }
        hash.update(&buffer[..n]);
    }
    Ok((file, size, hex::encode(hash.finalize())))
}

fn tar_header(path: &str, size: u64) -> Result<tar::Header, String> {
    let mut header = tar::Header::new_gnu();
    header.set_path(path).map_err(|e| e.to_string())?;
    header.set_size(size);
    header.set_mode(0o600);
    header.set_cksum();
    Ok(header)
}

fn append_json<W: Write>(archive: &mut tar::Builder<W>, path: &str, value: &Value) -> Result<(), String> {
    let data = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    archive.append(&tar_header(path, data.len() as u64)?, data.as_slice()).map_err(|e| e.to_string())
}

fn append_blob<W: Write>(archive: &mut tar::Builder<W>, value: &Value, media_type: &str) -> Result<Value, String> {
    let data = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    let hash = hex::encode(Sha256::digest(&data));
    archive.append(&tar_header(&format!("blobs/sha256/{hash}"), data.len() as u64)?, data.as_slice()).map_err(|e| e.to_string())?;
    Ok(json!({"mediaType":media_type,"digest":format!("sha256:{hash}"),"size":data.len()}))
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod integration;
