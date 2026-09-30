//! Three-way reconciliation: only the side unchanged since the last shared
//! checksum may be overwritten. Concurrent edits remain on both sides.
use super::*;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Write},
};

#[derive(Debug, PartialEq)]
enum Change {
    Same,
    Push,
    Pull,
    Conflict,
}

fn prioritized(change: Change, priority: sync::Priority) -> Change {
    if change != Change::Conflict { return change; }
    match priority { sync::Priority::Local => Change::Push, sync::Priority::Remote => Change::Pull, sync::Priority::KeepBoth => Change::Conflict }
}

fn change<'a>(base: Option<&'a str>, local: Option<&'a str>, remote: Option<&'a str>) -> Change {
    if local == remote {
        Change::Same
    } else if local == base {
        Change::Pull
    } else if remote == base {
        Change::Push
    } else {
        Change::Conflict
    }
}

fn validate(relative: &str) -> Result<(), String> {
    if relative.is_empty()
        || relative.len() > 4096
        || relative
            .chars()
            .any(|c| c.is_control() || c == '\\' || c == ':')
        || relative.split('/').any(|part| {
            part.is_empty()
                || part == "."
                || part == ".."
                || part.starts_with(".yougori-sync-")
                || part.ends_with(['.', ' '])
                || matches!(
                    part.split('.')
                        .next()
                        .unwrap_or("")
                        .to_ascii_uppercase()
                        .as_str(),
                    "CON"
                        | "PRN"
                        | "AUX"
                        | "NUL"
                        | "COM1"
                        | "COM2"
                        | "COM3"
                        | "COM4"
                        | "COM5"
                        | "COM6"
                        | "COM7"
                        | "COM8"
                        | "COM9"
                        | "LPT1"
                        | "LPT2"
                        | "LPT3"
                        | "LPT4"
                        | "LPT5"
                        | "LPT6"
                        | "LPT7"
                        | "LPT8"
                        | "LPT9"
                )
        })
    {
        return Err(format!("Unsupported sync path: {}", clean(relative)));
    }
    Ok(())
}

pub(super) fn destination(root: &Path, relative: &str) -> Result<PathBuf, String> {
    validate(relative)?;
    let mut path = root.to_path_buf();
    for part in relative.split('/') {
        path.push(part);
        match fs::symlink_metadata(&path) {
            Ok(meta) if sync::linked(&meta) => {
                return Err(format!(
                    "A symlink or junction blocks sync: {}",
                    clean(relative)
                ))
            }
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("Cannot inspect {}: {e}", clean(relative))),
        }
    }
    Ok(path)
}

fn local_hash(root: &Path, relative: &str) -> Result<Option<String>, String> {
    let path = destination(root, relative)?;
    let meta = match fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    if !meta.is_file() {
        return Err(format!("Not a supported sync file: {}", clean(relative)));
    }
    let mut file = fs::File::open(&path).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    let mut size = 0;
    loop {
        let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        size += n as u64;
        hash.update(&buffer[..n]);
    }
    if size != meta.len()
        || file.metadata().map_err(|e| e.to_string())?.modified().ok() != meta.modified().ok()
    {
        return Err(format!("{} changed during sync; retrying", clean(relative)));
    }
    Ok(Some(format!("{:x}", hash.finalize())))
}

fn expect_local(root: &Path, relative: &str, expected: Option<&str>) -> Result<PathBuf, String> {
    if local_hash(root, relative)?.as_deref() != expected {
        return Err(format!(
            "{} changed during sync; both versions were kept",
            clean(relative)
        ));
    }
    destination(root, relative)
}

impl Session {
    pub(super) async fn rpc(&mut self, request: Value) -> Result<Value, String> {
        self.rpc_progress(request, None).await
    }
    pub(super) async fn rpc_progress(&mut self, request: Value, progress: Option<&ui::Task>) -> Result<Value, String> {
        let seq = nonce();
        self.write(
            format!(
                "X {seq} {}\n",
                B64.encode(serde_json::to_vec(&request).map_err(|e| e.to_string())?)
            )
            .as_bytes(),
        )
        .await?;
        let prefix = format!("rpc:{seq}:");
        let progress_prefix = format!("progress:{seq}:");
        let end = format!("end:{seq}");
        let mut deadline = Instant::now() + Duration::from_secs(60);
        let mut data = String::new();
        loop {
            let (_, events) = self.read().await?;
            for event in events {
                if let Some(encoded) = event.strip_prefix(&progress_prefix) {
                    let measurement: Value = serde_json::from_slice(&B64.decode(encoded).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
                    if let Some(progress) = progress { progress.database_progress(&measurement, "container"); }
                    // A measured operation is active, even if a large database needs
                    // more than a minute to finish indexing or committing.
                    deadline = Instant::now() + Duration::from_secs(60);
                    continue;
                }
                if let Some(chunk) = event.strip_prefix(&prefix) {
                    data.push_str(chunk);
                    if data.len() > 256 * 1024 {
                        return Err("Sync reply exceeded its limit".into());
                    }
                } else if event == end {
                    let reply: Value =
                        serde_json::from_slice(&B64.decode(data).map_err(|e| e.to_string())?)
                            .map_err(|e| e.to_string())?;
                    if let Some(error) = reply["error"].as_str() {
                        return Err(error.into());
                    }
                    return Ok(reply["result"].clone());
                } else if let Some(error) = event.strip_prefix("error:") {
                    return Err(
                        String::from_utf8_lossy(&B64.decode(error).unwrap_or_default()).into(),
                    );
                }
            }
            if Instant::now() > deadline {
                return Err("Timed out waiting for two-way sync".into());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

impl Live {
    /// Recover an interrupted/missing checkpoint without replacing the workspace.
    /// Only host files and previously tracked deletions are candidates; guest-only
    /// runtime data must survive a reconnect.
    pub async fn reconcile_one_way(&mut self, manifest: &sync::Manifest) -> Result<(), String> {
        self.discover_databases(manifest);
        self.database_files.extend(self.saved.databases.keys().cloned());
        let paths: BTreeSet<_> = manifest.keys().chain(self.saved.files.keys())
            .chain(self.saved.pending.iter()).filter(|p| !sync::database_member(p, &self.database_files)).cloned().collect();
        let mut baseline = sync::Manifest::new();
        let progress = ui::task("Comparing project files to resume sync");
        let paths = paths.into_iter().collect::<Vec<_>>();
        progress.progress("Comparing files", 0, paths.len() as u64, "files");
        let mut completed = 0;
        for batch in paths.chunks(32) {
            let remote = self.receiver.rpc(json!({"op":"hashes","paths":batch})).await?;
            for path in batch {
                let hash = remote.get(path).ok_or("Missing recovery checksum")?;
                if let Some(stamp) = manifest.get(path) {
                    if local_hash(&self.folder, path)?.as_deref() == hash.as_str() {
                        baseline.insert(path.clone(), *stamp);
                    }
                } else if !hash.is_null() {
                    baseline.insert(path.clone(), (0, 0));
                }
            }
            completed += batch.len() as u64;
            progress.progress("Comparing files", completed, paths.len() as u64, "files");
        }
        progress.clear();
        self.saved.files = baseline;
        self.saved.pending.clear();
        self.saved.token.clear();
        self.saved.environment = self.receiver.environment.clone();
        self.saved.two_way = false;
        self.saved.save(&self.state_path)
    }

    /// The same bounded, atomic transfer works for large one-way edits too.
    pub(super) async fn sync_large_file(&mut self, path: &str) -> Result<(), String> {
        let local = local_hash(&self.folder, path)?.ok_or("File disappeared during sync; retrying")?;
        let reply = self.receiver.rpc(json!({"op":"hash","path":path})).await?;
        self.push_file(path, Some(&local), reply["hash"].as_str()).await
    }

    async fn remote_hashes(
        &mut self,
        filter: &sync::Filter,
        progress: &ui::Task,
    ) -> Result<(BTreeMap<String, String>, BTreeSet<String>), String> {
        let mut folders = vec![String::new()];
        self.remote_directories.clear();
        let mut paths = Vec::new();
        let mut seen = BTreeSet::new();
        let mut skipped = BTreeSet::new();
        while let Some(folder) = folders.pop() {
            progress.copy_status(&format!("Finding container files · {} found", paths.len()));
            progress.detail(&clean(&folder));
            let mut offset = 0;
            loop {
                let reply = self
                    .receiver
                    .rpc(json!({"op":"list","path":folder,"offset":offset}))
                    .await?;
                for entry in reply["entries"]
                    .as_array()
                    .ok_or("Invalid sync directory reply")?
                {
                    let name = entry["name"].as_str().ok_or("Missing sync filename")?;
                    let path = if folder.is_empty() {
                        name.to_owned()
                    } else {
                        format!("{folder}/{name}")
                    };
                    let dir = entry["kind"] == "dir";
                    if filter.excluded(&path, dir) {
                        continue;
                    }
                    validate(&path)?;
                    if name.contains('/') || path.split('/').count() > 128 {
                        return Err("Invalid or too deeply nested sync path".into());
                    }
                    // Windows filenames are case-insensitive. A Linux project containing
                    // both Foo and foo cannot be safely reconciled onto that filesystem.
                    let identity = if cfg!(windows) {
                        path.to_lowercase()
                    } else {
                        path.clone()
                    };
                    if !seen.insert(identity) {
                        return Err(format!("Conflicting filename casing: {}", clean(&path)));
                    }
                    if dir {
                        self.remote_directories.insert(path.clone());
                        folders.push(path);
                    } else if entry["kind"] == "sqlite" {
                        self.database_files.insert(path);
                    } else if entry["kind"] == "file" {
                        paths.push(path);
                    } else {
                        // Linux dependencies create symlinks after installation.
                        // Do not follow them or let one block unrelated file edits.
                        skipped.insert(path);
                    }
                }
                match reply["next"].as_u64() {
                    Some(next) if next > offset => offset = next,
                    Some(_) => return Err("Invalid sync directory offset".into()),
                    None => break,
                }
            }
        }
        let mut hashes = BTreeMap::new();
        paths.retain(|p| !sync::database_member(p, &self.database_files));
        progress.progress("Checking container files", 0, paths.len() as u64, "files");
        let mut completed = 0;
        for batch in paths.chunks(32) {
            let reply = self
                .receiver
                .rpc(json!({"op":"hashes","paths":batch}))
                .await?;
            for path in batch {
                if let Some(hash) = reply[path].as_str() {
                    if hash.len() != 64 || !hash.bytes().all(|c| c.is_ascii_hexdigit()) {
                        return Err("Invalid sync checksum".into());
                    }
                    hashes.insert(path.clone(), hash.into());
                } else if !reply.get(path).is_some_and(Value::is_null) {
                    return Err("Missing sync checksum".into());
                }
            }
            completed += batch.len() as u64;
            progress.progress("Checking container files", completed, paths.len() as u64, "files");
        }
        Ok((hashes, skipped))
    }

    async fn pull_file(
        &mut self,
        path: &str,
        local: Option<&str>,
        remote: Option<&str>,
    ) -> Result<(), String> {
        let destination = expect_local(&self.folder, path, local)?;
        let Some(hash) = remote else {
            if local.is_some() {
                // Recheck remote deletion before deleting the unchanged local file.
                let reply = self.receiver.rpc(json!({"op":"hash","path":path})).await?;
                if !reply["hash"].is_null() {
                    return Err("Remote file changed during sync; retrying".into());
                }
                let destination = expect_local(&self.folder, path, local)?;
                fs::remove_file(destination).map_err(|e| e.to_string())?;
            }
            return Ok(());
        };
        let parent = destination.parent().ok_or("Missing sync directory")?;
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let mut temp = tempfile::Builder::new()
            .prefix(".yougori-sync-")
            .tempfile_in(parent)
            .map_err(|e| e.to_string())?;
        let mut digest = Sha256::new();
        let mut offset = 0u64;
        let mut progress: Option<ui::Task> = None;
        loop {
            let reply = self
                .receiver
                .rpc(json!({"op":"read","path":path,"offset":offset}))
                .await?;
            let data = B64
                .decode(reply["data"].as_str().ok_or("Missing sync file data")?)
                .map_err(|e| e.to_string())?;
            offset += data.len() as u64;
            let total = reply["total"].as_u64().unwrap_or(offset);
            if total > 64 * 1024 && progress.is_none() {
                let task = ui::task("Downloading changed project file");
                task.detail(&clean(path)); progress = Some(task);
            }
            if let Some(progress) = &progress { progress.progress("Downloading file", offset, total, "bytes"); }
            if data.len() > 24 * 1024 {
                return Err("Sync file exceeded its limit".into());
            }
            temp.write_all(&data).map_err(|e| e.to_string())?;
            digest.update(&data);
            if reply["done"] == true {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mode = reply["mode"].as_u64().unwrap_or(0o644) as u32 & 0o777;
                    temp.as_file()
                        .set_permissions(fs::Permissions::from_mode(mode))
                        .map_err(|e| e.to_string())?;
                }
                break;
            }
            if data.is_empty() {
                return Err("Sync file read made no progress".into());
            }
        }
        if format!("{:x}", digest.finalize()) != hash {
            return Err(format!("{} changed during download; retrying", clean(path)));
        }
        let reply = self.receiver.rpc(json!({"op":"hash","path":path})).await?;
        if reply["hash"] != hash {
            return Err("Remote file changed during download; retrying".into());
        }
        let destination = expect_local(&self.folder, path, local)?;
        temp.as_file().sync_all().map_err(|e| e.to_string())?;
        temp.persist(destination).map_err(|e| e.to_string())?;
        if let Some(progress) = progress { progress.clear(); }
        Ok(())
    }

    async fn push_file(
        &mut self,
        path: &str,
        local: Option<&str>,
        remote: Option<&str>,
    ) -> Result<(), String> {
        let source = expect_local(&self.folder, path, local)?;
        let Some(hash) = local else {
            self.receiver
                .rpc(json!({"op":"delete","path":path,"expected":remote}))
                .await?;
            return Ok(());
        };
        self.receiver
            .rpc(json!({"op":"begin","path":path,"expected":remote}))
            .await?;
        let result = async {
            let mut file = fs::File::open(source).map_err(|e| e.to_string())?;
            let total = file.metadata().map_err(|e| e.to_string())?.len();
            let progress = (total > 64 * 1024).then(|| {
                let task = ui::task("Uploading changed project file"); task.detail(&clean(path));
                task.progress("Uploading file", 0, total, "bytes"); task
            });
            let mut completed = 0;
            let mut buffer = [0; 24 * 1024];
            loop {
                let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
                if n == 0 {
                    break;
                }
                self.receiver
                    .rpc(json!({"op":"chunk","data":B64.encode(&buffer[..n])}))
                    .await?;
                completed += n as u64;
                if let Some(progress) = &progress { progress.progress("Uploading file", completed, total, "bytes"); }
            }
            expect_local(&self.folder, path, local)?;
            self.receiver
                .rpc(json!({"op":"commit","hash":hash}))
                .await?;
            if let Some(progress) = progress { progress.clear(); }
            Ok(())
        }
        .await;
        if result.is_err() {
            let _ = self.receiver.rpc(json!({"op":"abort"})).await;
        }
        result
    }

    pub async fn sync_both(&mut self, env_files: bool) -> Result<usize, String> {
        self.sync_direction(env_files, sync::Direction::Both).await
    }

    pub(super) async fn sync_direction(&mut self, env_files: bool, direction: sync::Direction) -> Result<usize, String> {
        self.env_files = env_files;
        let progress = ui::task("Checking project changes");
        let filter = sync::Filter::load(&self.folder, env_files)?;
        if self.saved.environment != self.receiver.environment || !self.saved.two_way {
            self.saved.common.clear();
        }
        self.saved.two_way = true;
        self.saved.environment = self.receiver.environment.clone();
        self.saved
            .common
            .retain(|path, _| !filter.excluded(path, false));
        let manifest = sync::scan(&self.folder, &filter)?;
        self.discover_databases(&manifest);
        self.database_files.extend(self.saved.databases.keys().cloned());
        let mut local = BTreeMap::new();
        self.local_hashes
            .retain(|path, _| manifest.contains_key(path));
        let paths: Vec<_> = manifest.iter().filter(|(path, _)| !sync::database_member(path, &self.database_files)).collect();
        progress.progress("Checking local files", 0, paths.len() as u64, "files");
        for (index, (path, stamp)) in paths.iter().enumerate() {
            let (path, stamp) = (*path, *stamp);
            progress.detail(&clean(path));
            if let Some((_, hash)) = self.local_hashes.get(path).filter(|(old, _)| old == stamp) {
                local.insert(path.clone(), hash.clone());
            } else if let Some(hash) = local_hash(&self.folder, path)? {
                self.local_hashes
                    .insert(path.clone(), (*stamp, hash.clone()));
                local.insert(path.clone(), hash);
            }
            progress.progress("Checking local files", index as u64 + 1, paths.len() as u64, "files");
        }
        let (remote, skipped) = self.remote_hashes(&filter, &progress).await?;
        self.find_folder_conflicts(&local, &remote)?;
        let paths: BTreeSet<_> = local
            .keys()
            .chain(remote.keys())
            .chain(self.saved.common.keys())
            .cloned()
            .collect();
        let mut changed = Vec::new();
        let mut conflicts = BTreeSet::new();
        for path in paths {
            if sync::database_member(&path, &self.database_files) { continue; }
            // A skipped directory link also protects all previously tracked children
            // from being mistaken for remote deletions.
            if skipped.iter().any(|p| path == *p || path.starts_with(&format!("{p}/"))) {
                continue;
            }
            let local_hash = local.get(&path).map(String::as_str);
            let remote_hash = remote.get(&path).map(String::as_str);
            let ordinary = change(
                self.saved.common.get(&path).map(String::as_str),
                local_hash,
                remote_hash,
            );
            let action = prioritized(if ordinary != Change::Same && self.folder_conflict(&path) { Change::Conflict } else { ordinary }, self.priority);
            // Do not advance the common checkpoint for deferred changes. A later
            // handoff in the other direction must still see them, even after restart.
            if (action == Change::Push && !direction.sends()) || (action == Change::Pull && !direction.receives()) { continue; }
            if action != Change::Same {
                progress.detail(&clean(&path));
            }
            let hash = match action {
                Change::Conflict => {
                    if !self.conflicts.contains(&path) {
                        ui::warn(&format!("Sync conflict: {} — both versions kept. Make the two copies match to resume syncing this file.", clean(&path)));
                    }
                    conflicts.insert(path);
                    continue;
                }
                Change::Same => local_hash,
                Change::Push => {
                    self.push_file(&path, local_hash, remote_hash).await?;
                    changed.push(path.clone());
                    local_hash
                }
                Change::Pull => {
                    self.pull_file(&path, local_hash, remote_hash).await?;
                    changed.push(path.clone());
                    remote_hash
                }
            };
            if let Some(hash) = hash {
                self.saved.common.insert(path, hash.into());
            } else {
                self.saved.common.remove(&path);
            }
            if action != Change::Same {
                self.saved.save(&self.state_path)?;
            }
        }
        if !conflicts.is_empty() {
            ui::dash(|d| {
                d.note(&format!(
                    "{} sync conflict(s) · both versions kept",
                    conflicts.len()
                ))
            });
        }
        self.conflicts = conflicts;
        self.saved.files = sync::scan(&self.folder, &filter)?;
        self.saved.save(&self.state_path)?;
        let restart = direction.sends() && !changed.is_empty()
            && (self.crashed
                || changed.iter().any(|p| sync::needs_install(p))
                || package::Project::load(&self.folder)?.is_python());
        if restart && self.dev.is_some() {
            let mut project = package::Project::load(&self.folder)?;
            project.custom_command = self.custom_command.clone();
            let env = environment(&self.folder, &project, &self.saved.files, true, self.app_port, &self.nonce);
            execute(
                &self.receiver.environment,
                &container_command(
                    self.container.as_deref(),
                    &format!(
                        "printf '%s' {} | base64 -d > /yougori/launch/dev.env",
                        shell(&B64.encode(env))
                    ),
                    false,
                ),
            )
            .await?;
            self.receiver.write(b"K\n").await?;
        }
        progress.clear();
        Ok(changed.len() + self.sync_databases_direction(true, direction).await? + self.sync_folders_direction(true, direction).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local_sql(root: &Path, sql: &str) -> Result<String, String> {
        let result = std::process::Command::new("node").args(["--no-warnings", "-e",
            "const d=new(require('node:sqlite').DatabaseSync)(process.argv[1]); d.exec('PRAGMA journal_mode=WAL'); if(process.argv[2].startsWith('SELECT')) console.log(JSON.stringify(d.prepare(process.argv[2]).all())); else d.exec(process.argv[2]); d.close();"])
            .arg(root.join("data/records.sqlite")).arg(sql).output().map_err(|e| e.to_string())?;
        if !result.status.success() { return Err(String::from_utf8_lossy(&result.stderr).into()); }
        Ok(String::from_utf8_lossy(&result.stdout).into())
    }

    async fn remote_sql(id: &str, sql: &str) -> Result<String, String> {
        let code = format!("const d=new(require('node:sqlite').DatabaseSync)('/workspace/data/records.sqlite'); const q={}; if(q.startsWith('SELECT')) console.log(JSON.stringify(d.prepare(q).all())); else d.exec(q); d.close();", serde_json::to_string(sql).unwrap());
        let result = call("execute_environment_command", json!({"request":{"environmentId":id,"command":format!("node --no-warnings -e {}", shell(&code))}})).await?;
        if result["exitCode"] != 0 { return Err(result["stderr"].as_str().unwrap_or("SQLite test command failed").into()); }
        Ok(result["stdout"].as_str().unwrap_or("").into())
    }

    #[tokio::test]
    #[ignore = "Creates and deletes an isolated container in the running engine; no public ports or host shares"]
    async fn real_container_syncs_both_ways_preserves_conflicts_and_resumes() {
        use yougori_cli::public;
        let folder = tempfile::tempdir().unwrap();
        let settings = tempfile::tempdir().unwrap();
        let state = settings.path().join("sync.json");
        fs::write(
            folder.path().join("package.json"),
            r#"{"name":"sync-test","scripts":{"dev":"node server.js"}}"#,
        )
        .unwrap();
        fs::write(folder.path().join("code.txt"), "original").unwrap();
        fs::write(folder.path().join("server.js"), "require('http').createServer((q,s)=>s.end('sync fixture')).listen(3000,'0.0.0.0')").unwrap();
        fs::write(folder.path().join(".env"), "not sent").unwrap();
        fs::create_dir(folder.path().join("data")).unwrap();
        fs::write(folder.path().join("data/app.db"), vec![173; 70000]).unwrap();
        local_sql(folder.path(), "CREATE TABLE records(id TEXT PRIMARY KEY, value TEXT); INSERT INTO records VALUES ('base','original')").unwrap();
        // Cross the 64-record batch boundary and the large-record fallback.
        local_sql(folder.path(), "WITH RECURSIVE n(x) AS (SELECT 0 UNION ALL SELECT x+1 FROM n WHERE x<129) INSERT INTO records SELECT 'batch-'||x, 'value-'||x FROM n; INSERT INTO records VALUES ('large',zeroblob(120000))").unwrap();
        fs::write(folder.path().join(".gitignore"), "data/\n").unwrap();
        fs::write(folder.path().join(".yougoriignore"), "data/\n").unwrap();
        fs::create_dir_all(folder.path().join("empty/nested")).unwrap();
        let project = package::Project::load(folder.path()).unwrap();
        let canonical = folder.path().canonicalize().unwrap();
        let label = format!("sync-test-{}", nonce());
        let flags = [
            "--name",
            &label,
            "--cpu",
            "1",
            "--memory",
            "1GB",
            "--storage",
            "2GB",
            "--internet",
            "false",
            "docker.io/library/node:24-bookworm",
        ]
        .map(String::from);
        let mut request = public::parse_run(&flags, false).unwrap().request;
        request["containerCommand"] = json!("sleep infinity");
        request["description"] = json!(label);
        let created = call("run_workload", json!({"request":request,"start":true}))
            .await
            .unwrap();
        let id = created["id"].as_str().unwrap();
        let verify = |ok: bool, message: &str| -> Result<(), String> {
            if ok {
                Ok(())
            } else {
                Err(message.into())
            }
        };
        let result: Result<(), String> = async {
            let manifest = sync::scan(folder.path(), &sync::Filter::load(folder.path(), false)?)?;
            let staging = sync::stage(folder.path(), &manifest)?;
            let mut measurements = Vec::new();
            let copied = public::call_with_progress("copy_files_to_environment", json!({"environmentId":id,"paths":[staging.path().join("project")]}), |progress| measurements.push(progress.clone())).await?;
            verify(measurements.iter().any(|p| p["totalBytes"].as_u64().is_some_and(|n| n > 0)), "Copy job did not report measured byte progress")?;
            let imported = format!("{}/project/workspace", copied["destination"].as_str().ok_or("Missing import destination")?);
            execute(id, &format!("ln -s {} /workspace; test $(wc -c < /workspace/data/app.db) = 70000; test ! -e /workspace/.env", shell(&imported))).await?;
            // Exercise chunked one-way edits through the bulk import's workspace symlink.
            fs::write(folder.path().join("data/app.db"), vec![174; 80000]).map_err(|e| e.to_string())?;
            let initial = staging.databases.keys().cloned().collect();
            let saved = sync::Saved { environment: id.into(), databases: staging.databases, ..Default::default() };
            let mut live = Live::prepare(id, &canonical, &project, &sync::Manifest::new(), true, 3000, &state, saved).await?;
            live.initial_databases = initial;
            let run: Result<(), String> = async {
                let manifest = sync::scan(folder.path(), &sync::Filter::load(folder.path(), false)?)?;
                live.sync_to(manifest.clone()).await?;
                execute(id, "test -d /workspace/empty/nested").await?;
                verify(remote_sql(id, "SELECT value FROM records WHERE id='base'").await?.contains("original"), "Initial SQLite rows not copied")?;
                verify(remote_sql(id, "SELECT count(*) AS n FROM records WHERE id LIKE 'batch-%'").await?.contains("130"), "Batched SQLite rows were lost")?;
                verify(remote_sql(id, "SELECT length(value) AS n FROM records WHERE id='large'").await?.contains("120000"), "Large SQLite record fallback was truncated")?;
                remote_sql(id, "INSERT INTO records VALUES ('guest','guest-only')").await?;
                live.sync_to(manifest.clone()).await?;
                verify(!local_sql(folder.path(), "SELECT value FROM records WHERE id='guest'")?.contains("guest-only"), "One-way SQLite sync pulled a guest record")?;
                execute(id, "test $(wc -c < /workspace/data/app.db) = 80000").await?;
                execute(id, "printf guest > /workspace/remote.txt; test ! -e /workspace/.env").await?;
                let token = live.saved.token.clone();
                verify(live.sync_to(manifest.clone()).await? == 0, "No-op launch transferred files")?;
                let checkpoint = fs::metadata(&state).map_err(|e| e.to_string())?.modified().map_err(|e| e.to_string())?;
                verify(live.sync_to(manifest.clone()).await? == 0, "Unchanged database pass did not stay idle")?;
                verify(fs::metadata(&state).map_err(|e| e.to_string())?.modified().map_err(|e| e.to_string())? == checkpoint, "Unchanged database pass rewrote its checkpoint")?;
                verify(live.saved.token == token, "No-op launch rewrote the checkpoint")?;
                verify(!folder.path().join("remote.txt").exists(), "One-way sync unexpectedly downloaded a file")?;
                // Simulate interruption after an addition reached the guest, followed
                // by a local deletion. Recovery must not recopy unchanged files or
                // erase guest-only runtime data.
                execute(id, "printf partial > /workspace/interrupted.txt; printf incomplete > /yougori/launch/sync-token").await?;
                live.saved.pending.insert("interrupted.txt".into());
                live.saved.save(&state)?;
                fs::write(folder.path().join("code.txt"), "changed-offline").map_err(|e| e.to_string())?;
                let next = sync::scan(folder.path(), &sync::Filter::load(folder.path(), false)?)?;
                live.reconcile_one_way(&next).await?;
                verify(live.sync_to(next).await? == 2, "Recovery did not limit transfer to the edit and deletion")?;
                execute(id, "test ! -e /workspace/interrupted.txt; test \"$(cat /workspace/remote.txt)\" = guest; test \"$(cat /workspace/code.txt)\" = changed-offline").await?;
                // Package managers create these links; they must not block all sync.
                execute(id, "mkdir -p /workspace/node_modules/.bin; ln -s ../../code.txt /workspace/node_modules/.bin/tool").await?;
                live.sync_both(false).await?;
                verify(local_sql(folder.path(), "SELECT value FROM records WHERE id='guest'")?.contains("guest-only"), "Two-way SQLite sync did not pull a guest record")?;
                fs::write(folder.path().join("local-delete.txt"), "old").map_err(|e| e.to_string())?;
                fs::write(folder.path().join("remote-delete.txt"), "old").map_err(|e| e.to_string())?;
                local_sql(folder.path(), "INSERT INTO records VALUES ('local-delete-row','old'),('remote-delete-row','old')")?;
                live.sync_both(false).await?;
                // Wait beyond the former five-second interval. Manual sessions
                // must neither send a host edit nor pull a public-link edit.
                fs::write(folder.path().join("send-later.txt"), "host change").map_err(|e| e.to_string())?;
                fs::create_dir_all(folder.path().join("send-empty/nested")).map_err(|e| e.to_string())?;
                fs::remove_file(folder.path().join("local-delete.txt")).map_err(|e| e.to_string())?;
                local_sql(folder.path(), "INSERT INTO records VALUES ('send-later','host record'); DELETE FROM records WHERE id='local-delete-row'")?;
                execute(id, "printf 'container change' > /workspace/receive-later.txt; mkdir -p /workspace/receive-empty/nested; rm /workspace/remote-delete.txt").await?;
                remote_sql(id, "INSERT INTO records VALUES ('receive-later','container record'); DELETE FROM records WHERE id='remote-delete-row'").await?;
                live.start().await?;
                let cancelled = AtomicBool::new(false);
                let wait = async { tokio::time::sleep(Duration::from_secs(7)).await; cancelled.store(true, Ordering::Relaxed); };
                let (watched, _) = tokio::join!(live.watch("manual sync fixture", 3000, false, true, false, false, &cancelled), wait);
                watched?;
                execute(id, "test ! -e /workspace/send-later.txt; test -e /workspace/local-delete.txt; test ! -d /workspace/send-empty").await?;
                verify(!folder.path().join("receive-later.txt").exists() && folder.path().join("remote-delete.txt").exists(), "Manual watch transferred files")?;
                verify(!local_sql(folder.path(), "SELECT value FROM records WHERE id='receive-later'")?.contains("container record"), "Manual watch pulled records")?;
                verify(!remote_sql(id, "SELECT value FROM records WHERE id='send-later'").await?.contains("host record"), "Manual watch sent records")?;
                verify(live.handoff(false, false, sync::Direction::ToComputer).await.is_err(), "One-way mode allowed a receive handoff")?;
                live.handoff(false, true, sync::Direction::ToContainer).await?;
                execute(id, "test -e /workspace/send-later.txt; test ! -e /workspace/local-delete.txt; test -d /workspace/send-empty/nested").await?;
                verify(!folder.path().join("receive-later.txt").exists() && !folder.path().join("receive-empty").exists() && folder.path().join("remote-delete.txt").exists(), "Send handoff pulled files, folders or deletions")?;
                let sent = remote_sql(id, "SELECT id,value FROM records WHERE id='send-later'").await?;
                verify(sent.contains("host record"), &format!("Send handoff missed a host record: app read {sent:?}"))?;
                verify(!local_sql(folder.path(), "SELECT value FROM records WHERE id='receive-later'")?.contains("container record"), "Send handoff pulled a record")?;
                fs::write(folder.path().join("deferred-send.txt"), "pending host").map_err(|e| e.to_string())?;
                local_sql(folder.path(), "INSERT INTO records VALUES ('deferred-send','pending host record')")?;
                live.handoff(false, true, sync::Direction::ToComputer).await?;
                verify(folder.path().join("receive-later.txt").exists() && folder.path().join("receive-empty/nested").is_dir() && !folder.path().join("remote-delete.txt").exists(), "Receive handoff missed files, empty folders or deletions")?;
                execute(id, "test ! -e /workspace/deferred-send.txt").await?;
                verify(local_sql(folder.path(), "SELECT value FROM records WHERE id='receive-later'")?.contains("container record"), "Receive handoff missed a container record")?;
                verify(!remote_sql(id, "SELECT value FROM records WHERE id='deferred-send'").await?.contains("pending host record"), "Receive handoff sent records")?;
                live.close().await;
                live = Live::prepare(id, &canonical, &project, &sync::Manifest::new(), true, 3000, &state, sync::Saved::load(&state)).await?;
                live.handoff(false, true, sync::Direction::ToContainer).await?;
                execute(id, "test -e /workspace/deferred-send.txt").await?;
                verify(remote_sql(id, "SELECT value FROM records WHERE id='deferred-send'").await?.contains("pending host record"), "Receive consumed a deferred host checkpoint across restart")?;
                execute(id, "mkdir -p /workspace/guest-empty/nested; rmdir /workspace/empty/nested").await?;
                live.sync_both(false).await?;
                verify(folder.path().join("guest-empty/nested").is_dir(), "Empty guest folder not pulled")?;
                verify(!folder.path().join("empty/nested").exists(), "Empty guest folder deletion not pulled")?;
                local_sql(folder.path(), "UPDATE records SET value='from-pc' WHERE id='base'")?;
                live.sync_both(false).await?;
                verify(remote_sql(id, "SELECT value FROM records WHERE id='base'").await?.contains("from-pc"), "SQLite host update not sent")?;
                verify(fs::read_to_string(folder.path().join("remote.txt")).map_err(|e| e.to_string())? == "guest", "Remote addition was not downloaded")?;
                execute(id, "printf remote-edit > /workspace/code.txt; node -e \"require('fs').writeFileSync('/workspace/binary.bin', Buffer.alloc(70000, 173))\"").await?;
                live.sync_both(false).await?;
                verify(fs::read_to_string(folder.path().join("code.txt")).map_err(|e| e.to_string())? == "remote-edit", "Remote edit was not downloaded")?;
                verify(fs::read(folder.path().join("binary.bin")).map_err(|e| e.to_string())? == vec![173;70000], "Binary chunks were not reconstructed")?;
                fs::write(folder.path().join("code.txt"), "laptop-edit").map_err(|e| e.to_string())?;
                live.sync_both(false).await?;
                execute(id, "test \"$(cat /workspace/code.txt)\" = laptop-edit").await?;
                fs::write(folder.path().join("code.txt"), "local-conflict").map_err(|e| e.to_string())?;
                execute(id, "printf remote-conflict > /workspace/code.txt; rm /workspace/remote.txt").await?;
                live.sync_both(false).await?;
                verify(fs::read_to_string(folder.path().join("code.txt")).map_err(|e| e.to_string())? == "local-conflict", "Conflict overwrote the laptop")?;
                execute(id, "test \"$(cat /workspace/code.txt)\" = remote-conflict").await?;
                verify(!folder.path().join("remote.txt").exists(), "Remote deletion was not applied")?;
                fs::remove_file(folder.path().join("binary.bin")).map_err(|e| e.to_string())?;
                live.sync_both(false).await?;
                execute(id, "test ! -e /workspace/binary.bin").await?;
                Ok(())
            }.await;
            live.close().await;
            run?;
            // Resume from persisted common checksums, keeping the unresolved conflict.
            execute(id, "printf new > /workspace/after-reconnect.txt").await?;
            let mut live = Live::prepare(id, &canonical, &project, &sync::Manifest::new(), true, 3000, &state, sync::Saved::load(&state)).await?;
            let result = live.sync_both(false).await;
            live.close().await;
            result?;
            verify(fs::read_to_string(folder.path().join("after-reconnect.txt")).map_err(|e| e.to_string())? == "new", "Resume lost the remote addition")?;
            verify(fs::read_to_string(folder.path().join("code.txt")).map_err(|e| e.to_string())? == "local-conflict", "Resume lost the local conflict")?;
            execute(id, "test \"$(cat /workspace/code.txt)\" = remote-conflict").await?;
            local_sql(folder.path(), "UPDATE records SET value='pc-wins' WHERE id='base'")?;
            remote_sql(id, "UPDATE records SET value='guest-loses' WHERE id='base'").await?;
            // Priority is shared by file and record conflicts, including deletions.
            // Reopen the receiver after the prior check closed it.
            drop(live);
            let mut live = Live::prepare(id, &canonical, &project, &sync::Manifest::new(), true, 3000, &state, sync::Saved::load(&state)).await?;
            live.priority = sync::Priority::Local;
            let prioritized: Result<(), String> = async {
                live.sync_both(false).await?;
                execute(id, "test \"$(cat /workspace/code.txt)\" = local-conflict").await?;
                verify(remote_sql(id, "SELECT value FROM records WHERE id='base'").await?.contains("pc-wins"), "Local priority failed for SQLite")?;
                local_sql(folder.path(), "UPDATE records SET value='pc-loses' WHERE id='base'")?;
                remote_sql(id, "DELETE FROM records WHERE id='base'").await?;
                fs::write(folder.path().join("code.txt"), "local-loses").map_err(|e| e.to_string())?;
                execute(id, "rm /workspace/code.txt").await?;
                live.priority = sync::Priority::Remote;
                live.sync_both(false).await?;
                verify(!folder.path().join("code.txt").exists(), "Remote deletion did not win file conflict")?;
                verify(!local_sql(folder.path(), "SELECT id FROM records WHERE id='base'")?.contains("base"), "Remote deletion did not win SQLite conflict")?;
                let tree = folder.path().join("folder-conflict");
                fs::create_dir(&tree).map_err(|e| e.to_string())?;
                fs::write(tree.join("old.txt"), "old").map_err(|e| e.to_string())?;
                live.sync_both(false).await?;
                // A new child on one side conflicts with deleting its parent on the other.
                fs::remove_dir_all(&tree).map_err(|e| e.to_string())?;
                execute(id, "printf new > /workspace/folder-conflict/new.txt").await?;
                live.priority = sync::Priority::Local;
                live.sync_both(false).await?;
                execute(id, "test ! -e /workspace/folder-conflict").await?;
                fs::write(folder.path().join("priority-handoff.txt"), "base").map_err(|e| e.to_string())?;
                local_sql(folder.path(), "INSERT INTO records VALUES ('priority-handoff','base')")?;
                live.sync_both(false).await?;
                fs::write(folder.path().join("priority-handoff.txt"), "host wins").map_err(|e| e.to_string())?;
                local_sql(folder.path(), "UPDATE records SET value='host wins' WHERE id='priority-handoff'")?;
                execute(id, "printf 'container loses' > /workspace/priority-handoff.txt").await?;
                remote_sql(id, "UPDATE records SET value='container loses' WHERE id='priority-handoff'").await?;
                live.handoff(false, true, sync::Direction::ToComputer).await?;
                verify(fs::read_to_string(folder.path().join("priority-handoff.txt")).map_err(|e| e.to_string())? == "host wins", "Receive ignored host conflict priority")?;
                verify(local_sql(folder.path(), "SELECT value FROM records WHERE id='priority-handoff'")?.contains("host wins"), "Receive ignored host record priority")?;
                live.handoff(false, true, sync::Direction::ToContainer).await?;
                execute(id, "test \"$(cat /workspace/priority-handoff.txt)\" = 'host wins'").await?;
                verify(remote_sql(id, "SELECT value FROM records WHERE id='priority-handoff'").await?.contains("host wins"), "Deferred host record conflict was consumed")?;
                live.priority = sync::Priority::Remote;
                fs::write(folder.path().join("priority-handoff.txt"), "host loses").map_err(|e| e.to_string())?;
                local_sql(folder.path(), "UPDATE records SET value='host loses' WHERE id='priority-handoff'")?;
                execute(id, "printf 'container wins' > /workspace/priority-handoff.txt").await?;
                remote_sql(id, "UPDATE records SET value='container wins' WHERE id='priority-handoff'").await?;
                live.handoff(false, true, sync::Direction::ToContainer).await?;
                execute(id, "test \"$(cat /workspace/priority-handoff.txt)\" = 'container wins'").await?;
                verify(remote_sql(id, "SELECT value FROM records WHERE id='priority-handoff'").await?.contains("container wins"), "Send ignored container record priority")?;
                live.handoff(false, true, sync::Direction::ToComputer).await?;
                verify(fs::read_to_string(folder.path().join("priority-handoff.txt")).map_err(|e| e.to_string())? == "container wins", "Deferred container file conflict was consumed")?;
                verify(local_sql(folder.path(), "SELECT value FROM records WHERE id='priority-handoff'")?.contains("container wins"), "Deferred container record conflict was consumed")?;
                Ok(())
            }.await;
            live.close().await;
            prioritized?;
            Ok(())
        }.await;
        let cleanup = call("delete_environment", json!({"environmentId":id})).await;
        assert!(cleanup.is_ok(), "Test cleanup failed: {cleanup:?}");
        result.unwrap();
    }
    #[test]
    fn reconciliation_preserves_conflicts_and_propagates_unambiguous_edits_and_deletes() {
        assert_eq!(
            change(Some("base"), Some("base"), Some("remote")),
            Change::Pull
        );
        assert_eq!(
            change(Some("base"), Some("local"), Some("base")),
            Change::Push
        );
        assert_eq!(
            change(Some("base"), Some("local"), Some("remote")),
            Change::Conflict
        );
        assert_eq!(change(Some("base"), None, Some("remote")), Change::Conflict);
        assert_eq!(change(Some("base"), Some("local"), None), Change::Conflict);
        assert_eq!(change(Some("base"), None, Some("base")), Change::Push);
        assert_eq!(change(Some("base"), Some("base"), None), Change::Pull);
        assert_eq!(change(None, Some("new"), None), Change::Push);
        assert_eq!(change(None, None, Some("new")), Change::Pull);
        assert_eq!(
            change(None, Some("local"), Some("remote")),
            Change::Conflict
        );
        assert_eq!(
            change(Some("base"), Some("same"), Some("same")),
            Change::Same
        );
    }
    #[test]
    fn sync_paths_and_stale_local_writes_are_rejected() {
        let root = tempfile::tempdir().unwrap();
        for path in [
            "../escape",
            "/absolute",
            "C:/file",
            "a\\b",
            "a/../b",
            "CON",
            "a.",
            ".yougori-sync-temp",
        ] {
            assert!(destination(root.path(), path).is_err(), "{path}");
        }
        fs::write(root.path().join("file.txt"), "first").unwrap();
        let hash = local_hash(root.path(), "file.txt").unwrap().unwrap();
        fs::write(root.path().join("file.txt"), "second").unwrap();
        assert!(expect_local(root.path(), "file.txt", Some(&hash)).is_err());
        assert_eq!(
            fs::read_to_string(root.path().join("file.txt")).unwrap(),
            "second"
        );
        assert!(expect_local(root.path(), "file.txt", None).is_err());
    }
}
