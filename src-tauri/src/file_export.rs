//! Copying files out of environments to this PC. Every source speaks the same read-only file API
//! (list/stat/read in 64 KiB chunks), so one walker serves containers, microVMs, cloud servers and
//! environments shared with you. Existing local files are never overwritten.
use crate::{models::*, runtime::RuntimeManager, store::PlatformStore};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use cap_std::fs::{Dir, OpenOptions};
use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;
use tauri::State;

const MAX_ENTRIES: usize = 50_000;
const MAX_BYTES: u64 = 100 * 1024 * 1024 * 1024;

pub(crate) enum Source<'a> {
    /// Someone else's environment reached through its sharing link and permissions.
    Shared(&'a Environment),
    /// A local container/microVM agent or a connected cloud server's agent.
    Guest(&'a RuntimeManager, &'a Environment),
    /// In-memory tree for tests: path -> file bytes, or None for a folder.
    #[cfg(test)]
    Fake(std::collections::BTreeMap<String, Option<Vec<u8>>>),
}

impl Source<'_> {
    async fn request(&self, operation: &str, path: &str, offset: u64, length: u64) -> Result<Value, String> {
        match self {
            Source::Shared(env) => crate::remote_access::client::request_saved(env, "files", json!({"operation":operation,"path":path,"offset":offset,"length":length})).await,
            Source::Guest(runtime, env) => runtime.workspace_request(env, "/v1/remote/files", json!({
                "id": env.runtime_id.as_deref().unwrap_or(&env.id), "root": "/", "path": path,
                "operation": operation, "offset": offset, "length": length, "readOnly": true,
            })).await,
            #[cfg(test)]
            Source::Fake(tree) => {
                let node = tree.get(path).ok_or("No such file")?;
                match (operation, node) {
                    ("stat", node) => Ok(json!({"info":{"directory":node.is_none(),"size":node.as_ref().map_or(0, Vec::len)}})),
                    ("list", None) => Ok(json!({"entries": tree.iter()
                        .filter(|(p, _)| p.rsplit_once('/').map_or(!p.is_empty() && path.is_empty(), |(parent, _)| parent == path))
                        .map(|(p, n)| json!({"name":p.rsplit('/').next(),"directory":n.is_none(),"size":n.as_ref().map_or(0, Vec::len)}))
                        .collect::<Vec<_>>()})),
                    ("read", Some(bytes)) => {
                        let start = (offset as usize).min(bytes.len());
                        let end = (start + length as usize).min(bytes.len());
                        Ok(json!({"data": B64.encode(&bytes[start..end])}))
                    }
                    _ => Err("Unsupported".into()),
                }
            }
        }
    }
}

pub(crate) fn valid_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    !name.is_empty()
        && name.len() <= 255
        && !name.chars().any(|c| c.is_control() || "/\\:<>\"|?*".contains(c))
        && !name.ends_with(['.', ' '])
        && !matches!(name, "." | "..")
        && !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !(stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) && stem.as_bytes()[3].is_ascii_digit())
}

/// Creates `name`, or `name (2)`, `name (3)` … so nothing existing is replaced. Returns the name used.
fn fresh_folder(parent: &Dir, name: &str) -> Result<String, String> {
    for attempt in 1..1000 {
        let candidate = if attempt == 1 { name.to_owned() } else { format!("{name} ({attempt})") };
        match parent.create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err("Cannot create the destination folder".into()),
        }
    }
    Err("Too many folders with this name already exist".into())
}

/// `report.pdf`, then `report (2).pdf`, `report (3).pdf` … for a file that must not replace an existing one.
fn numbered(name: &str, attempt: u32) -> String {
    if attempt == 1 { return name.to_owned(); }
    match name.rsplit_once('.').filter(|(stem, _)| !stem.is_empty()) {
        Some((stem, extension)) => format!("{stem} ({attempt}).{extension}"),
        None => format!("{name} ({attempt})"),
    }
}

async fn copy_file(source: &Source<'_>, output: &Dir, remote: &str, local: &std::path::Path, size: u64) -> Result<(), String> {
    let file = output
        .open_with(local, OpenOptions::new().write(true).create_new(true))
        .map_err(|_| "Cannot create a copied file")?;
    write_file(source, file, remote, size).await
}

async fn write_file(source: &Source<'_>, mut file: cap_std::fs::File, remote: &str, size: u64) -> Result<(), String> {
    let mut offset = 0;
    while offset < size {
        let length = (size - offset).min(65536);
        let value = source.request("read", remote, offset, length).await?;
        let data = B64.decode(value["data"].as_str().unwrap_or("")).map_err(|_| "Invalid file data")?;
        if data.len() as u64 != length {
            return Err("A source file changed while copying; copy it again".into());
        }
        file.write_all(&data).map_err(|_| "Cannot write a copied file; check free disk space")?;
        offset += length;
    }
    file.sync_all().map_err(|_| "Cannot finish a copied file".to_string())
}

/// Copies `path` (a file or folder, relative to the source root) into a fresh folder named `folder` inside `destination`.
pub(crate) async fn copy_out(source: Source<'_>, path: &str, destination: &str, folder: &str) -> Result<Value, String> {
    let destination = PathBuf::from(destination);
    if !destination.is_absolute() || !destination.is_dir() {
        return Err("Choose an existing folder on this PC".into());
    }
    let parent = Dir::open_ambient_dir(&destination, cap_std::ambient_authority()).map_err(|_| "Cannot open the destination folder")?;
    let info = source.request("stat", path, 0, 0).await?;
    if info["info"]["directory"] != true {
        // A single file goes straight into the destination under its own name.
        let size = info["info"]["size"].as_u64().ok_or("Invalid file size")?;
        if size > MAX_BYTES { return Err("Copy exceeds 100 GiB".into()); }
        let file_name = path.rsplit('/').next().filter(|n| valid_name(n)).ok_or("The file name is not valid on this PC")?;
        let (name, file) = (1..1000).find_map(|attempt| {
            let name = numbered(file_name, attempt);
            match parent.open_with(&name, OpenOptions::new().write(true).create_new(true)) {
                Ok(file) => Some(Ok((name, file))),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => None,
                Err(_) => Some(Err("Cannot create the copied file".to_string())),
            }
        }).ok_or("Too many files with this name already exist")??;
        let copied = destination.join(&name).to_string_lossy().into_owned();
        write_file(&source, file, path, size).await.map_err(|error| {
            let _ = parent.remove_file(&name);
            error
        })?;
        return Ok(json!({"file": copied, "entries": 1, "bytes": size}));
    }
    let name = fresh_folder(&parent, folder)?;
    let output = parent.open_dir(&name).map_err(|_| "Cannot open the destination folder")?;
    let mut count = 0usize;
    let mut total = 0u64;
    let result: Result<(), String> = async {
        let mut pending = vec![(path.to_owned(), PathBuf::new(), 0)];
        while let Some((remote, local, depth)) = pending.pop() {
            if depth > 64 { return Err("Folder nesting exceeds 64 levels".into()); }
            let listing = source.request("list", &remote, 0, 0).await?;
            let entries = listing["entries"].as_array().filter(|e| e.len() <= 5000).ok_or("Invalid folder listing")?;
            for item in entries {
                count += 1;
                if count > MAX_ENTRIES { return Err("Copy exceeds 50,000 entries; copy smaller folders separately".into()); }
                let name = item["name"].as_str().filter(|n| valid_name(n)).ok_or("The folder contains a name that is not valid on this PC")?;
                let child = local.join(name);
                let remote_child = if remote.is_empty() { name.to_owned() } else { format!("{remote}/{name}") };
                if item["directory"] == true {
                    output.create_dir(&child).map_err(|_| "Cannot create a copied folder")?;
                    pending.push((remote_child, child, depth + 1));
                    continue;
                }
                let size = item["size"].as_u64().ok_or("Invalid file size")?;
                total = total.checked_add(size).ok_or("Copy too large")?;
                if total > MAX_BYTES { return Err("Copy exceeds 100 GiB; copy smaller folders separately".into()); }
                copy_file(&source, &output, &remote_child, &child, size).await?;
            }
        }
        Ok(())
    }
    .await;
    let folder = destination.join(name).to_string_lossy().into_owned();
    result.map_err(|error| format!("{error}. Files already copied remain in {folder}"))?;
    Ok(json!({"folder": folder, "entries": count, "bytes": total}))
}

/// Copies a file or folder out of a running environment into a new folder on this PC.
#[tauri::command]
pub async fn copy_files_from_environment(
    environment_id: String,
    path: String,
    destination: String,
    store: State<'_, PlatformStore>,
    runtime: State<'_, RuntimeManager>,
) -> Result<Value, String> {
    let env = store.snapshot()?.environments.into_iter().find(|e| e.id == environment_id).ok_or("Environment not found")?;
    let shared = env.runtime.starts_with("shared://tunnel/");
    if !shared && env.status != EnvironmentStatus::Running {
        return Err("Start or connect this environment first".into());
    }
    if env.kind == EnvironmentKind::FullVm {
        return Err("Full VMs have no Yougori file agent. Copy files out over the VM's own SSH, or place them on its imported-files drive while it is shut down.".into());
    }
    if env.provider == Some(RuntimeProviderKind::NativeSandbox) || env.kind == EnvironmentKind::ComputerBranch {
        return Err("This environment type does not support copying files out".into());
    }
    let trimmed = path.trim();
    let parts = trimmed.split('/').filter(|p| !p.is_empty() && *p != ".").collect::<Vec<_>>();
    if (!shared && !trimmed.starts_with('/')) || parts.is_empty() || trimmed.len() > 4096
        || trimmed.contains(['\0', '\\']) || parts.contains(&"..") {
        return Err("Give an absolute file or folder path inside the environment, for example /app/data".into());
    }
    let relative = parts.join("/");
    let relative = relative.as_str();
    let folder = relative.rsplit('/').next().filter(|n| valid_name(n)).unwrap_or("Yougori copy").to_owned();
    let source = if shared { Source::Shared(&env) } else { Source::Guest(&runtime, &env) };
    copy_out(source, relative, &destination, &folder).await
}

/// Copy a source file or folder into another environment using a temporary,
/// automatically removed host staging directory. The destination's normal
/// import path creates a new folder, so an existing guest file is not replaced.
#[tauri::command]
pub async fn copy_files_between_environments(
    source_id: String,
    target_id: String,
    path: String,
    store: State<'_, PlatformStore>,
    runtime: State<'_, RuntimeManager>,
) -> Result<Value, String> {
    if source_id == target_id { return Err("Choose two different environments".into()); }
    let state = store.snapshot()?;
    let source_env = state.environments.iter().find(|e| e.id == source_id).ok_or("Source environment not found")?;
    let target_env = state.environments.iter().find(|e| e.id == target_id).ok_or("Destination environment not found")?;
    if target_env.status != EnvironmentStatus::Running {
        return Err("Start or connect the destination environment first".into());
    }
    if !matches!(target_env.kind, EnvironmentKind::Container | EnvironmentKind::MicroVm | EnvironmentKind::FullVm | EnvironmentKind::Cloud)
        || target_env.runtime.starts_with("shared://") || target_env.provider == Some(RuntimeProviderKind::NativeSandbox) {
        return Err("This destination does not support imported files".into());
    }
    if source_env.status != EnvironmentStatus::Running && !source_env.runtime.starts_with("shared://tunnel/") {
        return Err("Start or connect the source environment first".into());
    }
    if source_env.kind == EnvironmentKind::FullVm || source_env.provider == Some(RuntimeProviderKind::NativeSandbox) {
        return Err("This source has no readable Yougori file service. Use its own SSH file transfer or export from its guest OS first.".into());
    }
    let trimmed = path.trim();
    let parts = trimmed.split('/').filter(|part| !part.is_empty() && *part != ".").collect::<Vec<_>>();
    if !trimmed.starts_with('/') || parts.is_empty() || trimmed.len() > 4096
        || trimmed.contains(['\0', '\\']) || parts.contains(&"..") {
        return Err("Choose an absolute source file or folder path, such as /app/data".into());
    }
    let relative = parts.join("/");
    let folder = parts.last().filter(|name| valid_name(name)).ok_or("The source name is not valid on this PC")?;
    let staging_root = runtime.storage_root().join("cross-environment-transfers");
    std::fs::create_dir_all(&staging_root).map_err(|_| "Cannot prepare temporary transfer storage")?;
    let staging = tempfile::Builder::new().prefix("copy-").tempdir_in(&staging_root).map_err(|_| "Cannot prepare temporary transfer storage")?;
    let shared = source_env.runtime.starts_with("shared://tunnel/");
    let source = if shared { Source::Shared(source_env) } else { Source::Guest(&runtime, source_env) };
    let copied = copy_out(source, &relative, &staging.path().to_string_lossy(), folder).await?;
    let local = copied["file"].as_str().or(copied["folder"].as_str()).ok_or("The source copy did not produce a file or folder")?;
    let imported = crate::file_import::copy_files(&target_id, vec![local.to_owned()], &store, &runtime, |_| {}).await?;
    Ok(json!({"source": trimmed, "sourceEnvironment":source_env.name, "destination": imported.destination,
        "files": imported.files, "bytes": imported.bytes}))
}

pub(crate) fn valid_volume_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    (1..=80).contains(&bytes.len()) && bytes[0].is_ascii_alphanumeric()
        && bytes.iter().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
}

/// Environments whose settings mount each named volume: name -> [{environmentId, environment, target, readOnly, running}].
fn volume_mounts(state: &PlatformState, runtime: &RuntimeManager) -> std::collections::BTreeMap<String, Vec<Value>> {
    let mut mounts = std::collections::BTreeMap::<String, Vec<Value>>::new();
    for env in &state.environments {
        if !matches!(env.kind, EnvironmentKind::Container | EnvironmentKind::MicroVm) || env.runtime.starts_with("shared://") {
            continue;
        }
        let Ok(options) = runtime.workload_options(env.runtime_id.as_deref().unwrap_or(&env.id)) else { continue };
        for volume in options.volumes {
            mounts.entry(volume.source).or_default().push(json!({
                "environmentId": env.id, "environment": env.name, "target": volume.target,
                "readOnly": volume.read_only, "running": env.status == EnvironmentStatus::Running,
            }));
        }
    }
    mounts
}

/// Named volumes: where environments mount them, and what each container runtime actually stores,
/// including volumes left behind by deleted environments. Only running runtimes are asked unless
/// `scan` is set, which starts stopped ones. `size` adds up each volume's files.
pub(crate) async fn volumes(store: &PlatformStore, runtime: &RuntimeManager, scan: bool, size: bool) -> Result<Value, String> {
    let state = store.snapshot()?;
    let names: std::collections::HashMap<String, String> = state.environments.iter()
        .flat_map(|env| [(env.id.clone(), env.name.clone()), (env.runtime_id.clone().unwrap_or_default(), env.name.clone())])
        .filter(|(id, _)| !id.is_empty()).collect();
    let mut volumes: std::collections::BTreeMap<String, Value> = volume_mounts(&state, runtime).into_iter()
        .map(|(name, mounts)| (name.clone(), json!({"name": name, "mounts": mounts, "stored": []}))).collect();
    let (stores, unavailable) = runtime.volume_stores(scan).await;
    let gpu = stores.iter().any(|place| place.provider == RuntimeProviderKind::YougoriCuda);
    let mut checked = Vec::new();
    let mut problems: Vec<String> = unavailable;
    for listing in runtime.list_named_volumes(&stores, size).await {
        let Some(list) = listing["volumes"].as_array() else {
            problems.push(format!("{}: {}", listing["location"].as_str().unwrap_or("?"), outdated_agent(listing["error"].as_str().unwrap_or("unavailable"), listing["runtime"] == "gpu")));
            continue;
        };
        checked.push(listing["location"].clone());
        for volume in list {
            let Some(name) = volume["name"].as_str() else { continue };
            let used_by: Vec<Value> = volume["usedBy"].as_array().into_iter().flatten()
                .filter_map(|c| c.as_str()).map(|c| Value::from(names.get(c).cloned().unwrap_or_else(|| c.to_owned()))).collect();
            let entry = volumes.entry(name.to_owned()).or_insert_with(|| json!({"name": name, "mounts": [], "stored": []}));
            if let Some(stored) = entry["stored"].as_array_mut() {
                stored.push(json!({
                    "location": listing["location"], "runtime": listing["runtime"], "usedBy": used_by,
                    "sizeBytes": volume["sizeBytes"], "sizeComplete": volume["sizeComplete"],
                }));
            }
        }
    }
    let volumes: Vec<Value> = volumes.into_values().map(|mut volume| {
        let used = volume["mounts"].as_array().is_some_and(|m| !m.is_empty())
            || volume["stored"].as_array().into_iter().flatten().any(|s| s["usedBy"].as_array().is_some_and(|u| !u.is_empty()));
        volume["inUse"] = used.into();
        volume
    }).collect();
    Ok(json!({
        "volumes": volumes, "checked": checked, "problems": problems,
        "note": match (scan, gpu) {
            (true, true) => "Every container runtime was checked.",
            (true, false) => "Every container runtime was checked except the GPU runtime, which is checked while a GPU environment runs.",
            _ => "Only running container runtimes were checked; scan to find volumes kept by stopped ones.",
        },
    }))
}

fn outdated_agent(error: &str, gpu: bool) -> String {
    if error.ends_with(": Not Found") && gpu {
        "the GPU runtime runs an older Yougori agent; run GPU setup again (`yougori gpu setup --yes`) to update it".into()
    } else if error.ends_with(": Not Found") {
        "this runtime runs an older Yougori agent; stop its environments and start them again to update it".into()
    } else {
        error.to_owned()
    }
}

/// Removes a named volume and its data from every container runtime that keeps it. Refuses while
/// an environment mounts it. The GPU runtime is included only while it runs.
pub(crate) async fn remove_named_volume(name: &str, store: &PlatformStore, runtime: &RuntimeManager) -> Result<Value, String> {
    if !valid_volume_name(name) {
        return Err("Volume names use up to 80 letters, numbers, dots, dashes or underscores".into());
    }
    let state = store.snapshot()?;
    if let Some(mounts) = volume_mounts(&state, runtime).get(name) {
        let users = mounts.iter().filter_map(|m| m["environment"].as_str()).collect::<Vec<_>>().join(", ");
        return Err(format!("{name} is used by {users}. Delete those environments first; the volume and its data stay until then."));
    }
    let (stores, unavailable) = runtime.volume_stores(true).await;
    let mut removed = Vec::new();
    let mut failures = Vec::new();
    for place in &stores {
        match runtime.volume_request(place, &json!({"action": "remove", "name": name})).await {
            Ok(_) => removed.push(place.location.clone()),
            Err(error) if error.contains("no volume named") => {}
            Err(error) => failures.push(format!("{}: {}", place.location, outdated_agent(&error, place.provider == RuntimeProviderKind::YougoriCuda))),
        }
    }
    if !failures.is_empty() {
        let done = if removed.is_empty() { String::new() } else { format!(" Removed from {}.", removed.join(", ")) };
        return Err(format!("Could not remove {name}: {}.{done}", failures.join("; ")));
    }
    if removed.is_empty() {
        let skipped = if unavailable.is_empty() { String::new() } else { format!(" Not checked: {}.", unavailable.join("; ")) };
        return Err(format!("No volume named {name}.{skipped}"));
    }
    Ok(json!({"removed": name, "from": removed, "notChecked": unavailable}))
}

#[tauri::command]
pub async fn list_volumes(scan: Option<bool>, size: Option<bool>, store: State<'_, PlatformStore>, runtime: State<'_, RuntimeManager>) -> Result<Value, String> {
    volumes(&store, &runtime, scan.unwrap_or(false), size.unwrap_or(false)).await
}

#[tauri::command]
pub async fn remove_volume(name: String, store: State<'_, PlatformStore>, runtime: State<'_, RuntimeManager>) -> Result<Value, String> {
    remove_named_volume(&name, &store, &runtime).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn volume_names_match_the_guest_rules() {
        for good in ["data", "db.cache", "a_b-1", &"x".repeat(80)] { assert!(valid_volume_name(good), "{good}"); }
        for bad in ["", "-data", ".x", "a/b", "a b", "a:b", &"x".repeat(81)] { assert!(!valid_volume_name(bad), "{bad}"); }
        assert!(outdated_agent("runtime operation failed: Not Found", false).contains("start them again"));
        assert!(outdated_agent("runtime operation failed: Not Found", true).contains("gpu setup"));
        assert_eq!(outdated_agent("disk full", true), "disk full");
    }
    #[test]
    fn copied_names_are_safe_on_windows() {
        for good in ["data", "report.pdf", "a b", ".env"] { assert!(valid_name(good), "{good}"); }
        for bad in ["", ".", "..", "a/b", "a\\b", "con", "COM1.txt", "name.", "x:y", "tab\t"] { assert!(!valid_name(bad), "{bad}"); }
    }
    #[tokio::test]
    async fn folders_and_large_files_are_copied_in_chunks_without_overwriting() {
        let big = (0..200_000u32).map(|i| (i % 251) as u8).collect::<Vec<_>>();
        let tree = || std::collections::BTreeMap::from([
            ("app".to_owned(), None),
            ("app/data".to_owned(), None),
            ("app/data/big.bin".to_owned(), Some(big.clone())),
            ("app/data/notes".to_owned(), None),
            ("app/data/notes/a.txt".to_owned(), Some(b"hello".to_vec())),
        ]);
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().to_string_lossy().into_owned();
        let first = copy_out(Source::Fake(tree()), "app/data", &destination, "data").await.unwrap();
        assert_eq!((first["entries"].as_u64(), first["bytes"].as_u64()), (Some(3), Some(200_005)));
        assert_eq!(std::fs::read(root.path().join("data/big.bin")).unwrap(), big);
        assert_eq!(std::fs::read(root.path().join("data/notes/a.txt")).unwrap(), b"hello");
        copy_out(Source::Fake(tree()), "app/data", &destination, "data").await.unwrap();
        assert!(root.path().join("data (2)/big.bin").exists());
        for expected in ["a.txt", "a (2).txt"] {
            let copied = copy_out(Source::Fake(tree()), "app/data/notes/a.txt", &destination, "a.txt").await.unwrap();
            assert_eq!(copied["file"].as_str().unwrap(), root.path().join(expected).to_string_lossy());
            assert_eq!(std::fs::read(root.path().join(expected)).unwrap(), b"hello");
        }
        assert_eq!(numbered("archive.tar.gz", 2), "archive.tar (2).gz");
        assert_eq!(numbered(".env", 3), ".env (3)");
    }
    #[test]
    fn existing_folders_are_never_reused() {
        let root = tempfile::tempdir().unwrap();
        let dir = Dir::open_ambient_dir(root.path(), cap_std::ambient_authority()).unwrap();
        assert_eq!(fresh_folder(&dir, "data").unwrap(), "data");
        assert_eq!(fresh_folder(&dir, "data").unwrap(), "data (2)");
        assert_eq!(fresh_folder(&dir, "data").unwrap(), "data (3)");
    }
}
