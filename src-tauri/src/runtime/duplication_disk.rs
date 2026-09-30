use super::{command_output, path_string, RuntimeManager};
use serde_json::Value;
use std::path::Path;

fn validate_info(info: &Value) -> Result<&str, String> {
    let format = info["format"].as_str().ok_or("Missing disk format")?;
    if !matches!(format, "qcow2" | "vmdk" | "vpc" | "raw") {
        return Err("Unsupported cloud disk format".into());
    }
    if info["backing-filename"]
        .as_str()
        .is_some_and(|s| !s.is_empty())
        || info.pointer("/format-specific/data/data-file").is_some()
    {
        return Err(
            "The transferred disk must be standalone, without backing files or external data files"
                .into(),
        );
    }
    if info["encrypted"].as_bool() == Some(true) {
        return Err("Encrypted disk containers cannot be converted".into());
    }
    let size = info["virtual-size"]
        .as_u64()
        .ok_or("Missing virtual disk size")?;
    if size == 0 || size > 2 * 1024_u64.pow(4) {
        return Err("The virtual disk is empty or larger than 2 TiB".into());
    }
    Ok(format)
}

impl RuntimeManager {
    pub(crate) async fn verify_duplication_artifact(
        &self,
        path: &Path,
        expected_size: u64,
        expected_checksum: &str,
    ) -> Result<(), String> {
        let (size, checksum) = super::vm::hash_file(path.to_path_buf()).await?;
        if size != expected_size || checksum != expected_checksum {
            return Err(
                "The staged VM disk failed its integrity check. Resume to export a fresh copy."
                    .into(),
            );
        }
        Ok(())
    }

    pub(crate) async fn convert_duplication_disk(
        &self,
        source: &Path,
        destination: &Path,
        format: &str,
    ) -> Result<(), String> {
        if !matches!(format, "qcow2" | "vmdk" | "vpc") {
            return Err("Unsupported destination disk format".into());
        }
        let source = source.canonicalize().map_err(|e| e.to_string())?;
        let root = self
            .data_root
            .join("duplications")
            .canonicalize()
            .map_err(|e| e.to_string())?;
        let parent = destination
            .parent()
            .ok_or("Missing staging directory")?
            .canonicalize()
            .map_err(|e| e.to_string())?;
        if !source.starts_with(&root) || !parent.starts_with(&root) {
            return Err("Disk conversion must stay inside managed duplication staging".into());
        }
        // Fixed VHDs have their header at EOF and can be probed as raw. Never
        // guess a provider transfer format, or its footer becomes guest data.
        let expected = match source.extension().and_then(|value| value.to_str()) {
            Some("vhd" | "vpc") => "vpc",
            Some("vmdk") => {
                use std::io::Read;
                let mut header = [0; 4];
                std::fs::File::open(&source)
                    .and_then(|mut file| file.read_exact(&mut header))
                    .map_err(|e| e.to_string())?;
                if &header != b"KDMV" {
                    return Err("Use a standalone sparse VMDK, not a descriptor referencing external disk files".into());
                }
                "vmdk"
            }
            Some("qcow2") => "qcow2",
            Some("raw") => "raw",
            _ => return Err("Missing explicit cloud disk format".into()),
        };
        let info = command_output(
            &self.layout.qemu_img,
            &[
                "info".into(),
                "-f".into(),
                expected.into(),
                "--output=json".into(),
                path_string(&source),
            ],
            "inspect transferred disk",
        )
        .await?;
        let info: Value = serde_json::from_slice(&info.stdout).map_err(|e| e.to_string())?;
        let original = validate_info(&info)?;
        if format == "vpc" && info["virtual-size"].as_u64().unwrap_or(0) % (1024 * 1024) != 0 {
            return Err("Azure requires a disk capacity aligned to 1 MiB. Resize the source disk before exporting it.".into());
        }
        let temporary = parent.join(format!("convert-{}.part", uuid::Uuid::new_v4()));
        let mut args = vec![
            "convert".into(),
            "-f".into(),
            original.into(),
            "-O".into(),
            format.into(),
        ];
        if format == "vpc" {
            args.extend(["-o".into(), "subformat=fixed,force_size=on".into()]);
        }
        if format == "vmdk" {
            args.extend(["-o".into(), "subformat=streamOptimized".into()]);
        }
        args.extend([path_string(&source), path_string(&temporary)]);
        let result = async {
            command_output(&self.layout.qemu_img, &args, "convert copied disk").await?;
            // Compare every guest-visible byte, rather than merely trusting conversion success.
            command_output(
                &self.layout.qemu_img,
                &[
                    "compare".into(),
                    "-f".into(),
                    original.into(),
                    "-F".into(),
                    format.into(),
                    path_string(&source),
                    path_string(&temporary),
                ],
                "verify copied disk contents",
            )
            .await?;
            if destination.exists() {
                tokio::fs::remove_file(destination)
                    .await
                    .map_err(|e| e.to_string())?;
            }
            tokio::fs::rename(&temporary, destination)
                .await
                .map_err(|e| e.to_string())?;
            Ok(())
        }
        .await;
        if result.is_err() {
            let _ = tokio::fs::remove_file(&temporary).await;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn only_standalone_bounded_disks_can_be_transferred() {
        let valid = json!({"format":"qcow2","virtual-size":1024});
        assert_eq!(validate_info(&valid).unwrap(), "qcow2");
        for extra in [
            json!({"backing-filename":"/host/private"}),
            json!({"encrypted":true}),
            json!({"format-specific":{"data":{"data-file":"other.raw"}}}),
            json!({"format":"vdi"}),
            json!({"virtual-size":0}),
        ] {
            let mut info = valid.clone();
            info.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            assert!(validate_info(&info).is_err());
        }
    }

    #[tokio::test]
    async fn real_cloud_disk_formats_preserve_every_byte() {
        use std::io::{Seek, SeekFrom, Write};
        let directory = tempfile::tempdir().unwrap();
        let runtime =
            RuntimeManager::new(Path::new(env!("CARGO_MANIFEST_DIR")), directory.path()).unwrap();
        let staging = runtime
            .data_root
            .join("duplications")
            .join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&staging).unwrap();
        let source = staging.join("source.raw");
        let mut file = std::fs::File::create(&source).unwrap();
        file.set_len(8 * 1024 * 1024).unwrap();
        file.seek(SeekFrom::Start(1024 * 1024 + 17)).unwrap();
        file.write_all(b"installed application and persistent files")
            .unwrap();
        file.sync_all().unwrap();
        drop(file);
        for format in ["qcow2", "vmdk", "vpc"] {
            let destination = staging.join(format!("copy.{format}"));
            runtime
                .convert_duplication_disk(&source, &destination, format)
                .await
                .unwrap();
            let roundtrip = staging.join(format!("roundtrip-{format}.qcow2"));
            runtime
                .convert_duplication_disk(&destination, &roundtrip, "qcow2")
                .await
                .unwrap();
            command_output(
                &runtime.layout.qemu_img,
                &[
                    "compare".into(),
                    "-f".into(),
                    "raw".into(),
                    "-F".into(),
                    "qcow2".into(),
                    path_string(&source),
                    path_string(&roundtrip),
                ],
                "compare round-trip disk",
            )
            .await
            .unwrap();
        }
        assert!(runtime
            .convert_duplication_disk(&source, &directory.path().join("outside.qcow2"), "qcow2")
            .await
            .unwrap_err()
            .contains("managed duplication staging"));
    }
}
