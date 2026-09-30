use super::*;

fn fixture() -> (Vec<u8>, Vec<u8>) {
    let mut rootfs = tar::Builder::new(Vec::new());
    let mut header = tar_header("project/database", 13).unwrap();
    header.set_mode(0o640);
    header.set_uid(1000);
    header.set_gid(1001);
    header.set_cksum();
    rootfs.append(&header, &b"database-data"[..]).unwrap();
    let rootfs = rootfs.into_inner().unwrap();
    let metadata = serde_json::to_vec(&json!({
        "architecture":"amd64", "os":"linux", "created":"2026-09-11T00:00:00Z",
        "config":{"WorkingDir":"/project", "User":"1000:1001", "Env":["MODE=production"], "Entrypoint":["/entrypoint"], "Cmd":["node", "server.js"]}
    })).unwrap();
    let mut wire = MAGIC.to_vec();
    wire.extend((metadata.len() as u32).to_be_bytes());
    wire.extend(metadata);
    let mut gzip = flate2::write::GzEncoder::new(wire, flate2::Compression::fast());
    gzip.write_all(&rootfs).unwrap();
    (gzip.finish().unwrap(), rootfs)
}

#[test]
fn streaming_snapshot_is_a_valid_oci_archive_with_preserved_configuration() {
    let directory = tempfile::tempdir().unwrap();
    let (wire, rootfs) = fixture();
    let (file, size, checksum) = build_archive(wire.as_slice(), NamedTempFile::new_in(directory.path()).unwrap(), directory.path(), "opendock.local/snapshots:test").unwrap();
    let bytes = std::fs::read(file.path()).unwrap();
    assert_eq!(size, bytes.len() as u64);
    assert_eq!(checksum, hex::encode(Sha256::digest(&bytes)));
    let mut files = std::collections::HashMap::new();
    for entry in tar::Archive::new(bytes.as_slice()).entries().unwrap() {
        let mut entry = entry.unwrap();
        let path = entry.path().unwrap().to_string_lossy().to_string();
        let mut data = Vec::new();
        entry.read_to_end(&mut data).unwrap();
        files.insert(path, data);
    }
    let blob = |descriptor: &Value| {
        let digest = descriptor["digest"].as_str().unwrap().strip_prefix("sha256:").unwrap();
        let data = &files[&format!("blobs/sha256/{digest}")];
        assert_eq!(descriptor["size"], data.len());
        assert_eq!(digest, hex::encode(Sha256::digest(data)));
        data
    };
    let index: Value = serde_json::from_slice(&files["index.json"]).unwrap();
    assert_eq!(index["manifests"][0]["annotations"]["io.containerd.image.name"], "opendock.local/snapshots:test");
    let manifest: Value = serde_json::from_slice(blob(&index["manifests"][0])).unwrap();
    let config: Value = serde_json::from_slice(blob(&manifest["config"])).unwrap();
    assert_eq!(config["config"]["WorkingDir"], "/project");
    assert_eq!(config["config"]["User"], "1000:1001");
    assert_eq!(config["config"]["Env"], json!(["MODE=production"]));
    assert_eq!(config["config"]["Entrypoint"], json!(["/entrypoint"]));
    assert_eq!(config["config"]["Cmd"], json!(["node", "server.js"]));
    assert_eq!(config["rootfs"]["diff_ids"][0], format!("sha256:{}", hex::encode(Sha256::digest(&rootfs))));
    let mut expanded = Vec::new();
    flate2::read::GzDecoder::new(blob(&manifest["layers"][0]).as_slice()).read_to_end(&mut expanded).unwrap();
    assert_eq!(expanded, rootfs);
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1, "must not create a second staging copy");
}

#[test]
fn failed_snapshot_streams_leave_no_archive() {
    let directory = tempfile::tempdir().unwrap();
    let (wire, _) = fixture();
    let mut corrupt = wire.clone();
    let last = corrupt.len() - 5;
    corrupt[last] ^= 0xff;
    let mut extra = wire.clone();
    extra.extend(b"trailing incomplete export");
    for bad in [wire[..wire.len() - 8].to_vec(), corrupt, extra, b"wrong version".to_vec()] {
        assert!(build_archive(bad.as_slice(), NamedTempFile::new_in(directory.path()).unwrap(), directory.path(), "opendock.local/snapshots:test").is_err());
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    }
}
