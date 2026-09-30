use super::*;
use std::collections::BTreeSet;

impl Live {
    pub(super) fn find_folder_conflicts(&mut self, local: &std::collections::BTreeMap<String, String>, remote: &std::collections::BTreeMap<String, String>) -> Result<(), String> {
        self.folder_conflicts.clear();
        let local_dirs = sync::directories(&self.folder, &sync::Filter::load(&self.folder, self.env_files)?)?;
        for dir in &self.saved.directories {
            let (l,r) = (local_dirs.contains(dir), self.remote_directories.contains(dir));
            if l == r { continue; }
            let surviving = if l { local } else { remote };
            let dirs = if l { &local_dirs } else { &self.remote_directories };
            let prefix = format!("{dir}/");
            if surviving.iter().any(|(p,h)| p.starts_with(&prefix) && self.saved.common.get(p) != Some(h))
                || dirs.iter().any(|p| p.starts_with(&prefix) && !self.saved.directories.contains(p)) {
                self.folder_conflicts.insert(dir.clone());
            }
        }
        Ok(())
    }

    pub(super) fn folder_conflict(&self, path: &str) -> bool {
        self.folder_conflicts.iter().any(|p| path == p || path.starts_with(&format!("{p}/")))
    }

    pub(super) async fn sync_folders(&mut self, two_way: bool) -> Result<usize, String> {
        self.sync_folders_direction(two_way, if two_way { sync::Direction::Both } else { sync::Direction::ToContainer }).await
    }

    pub(super) async fn sync_folders_direction(&mut self, two_way: bool, direction: sync::Direction) -> Result<usize, String> {
        let local = sync::directories(&self.folder, &sync::Filter::load(&self.folder, self.env_files)?)?;
        let remote = if two_way { self.remote_directories.clone() } else { self.saved.directories.clone() };
        let paths: BTreeSet<_> = local.iter().chain(remote.iter()).chain(self.saved.directories.iter()).cloned().collect();
        let mut next = self.saved.directories.clone();
        let mut guest_create = Vec::new();
        let mut guest_remove = Vec::new();
        let mut host_create = Vec::new();
        let mut host_remove = Vec::new();
        for path in paths {
            let (l,r,b) = (local.contains(&path),remote.contains(&path),self.saved.directories.contains(&path));
            if l == r { if l { next.insert(path); } else { next.remove(&path); } continue; }
            if two_way && self.folder_conflict(&path) {
                match self.priority {
                    sync::Priority::Local if direction.sends() => { if l { guest_create.push(path); } else { guest_remove.push(path); } }
                    sync::Priority::Remote if direction.receives() => { if r { host_create.push(path); } else { host_remove.push(path); } }
                    sync::Priority::KeepBoth => { let message = format!("Folder conflict: {} — both versions kept", clean(&path)); if self.conflicts.insert(message.clone()) { ui::warn(&message); } }
                    _ => {}
                }
                continue;
            }
            if !two_way || r == b {
                if direction.sends() { if l { guest_create.push(path); } else { guest_remove.push(path); } }
            } else if l == b {
                if direction.receives() { if r { host_create.push(path); } else { host_remove.push(path); } }
            }
        }
        let mut changed = 0;
        // Parent creation is recursive; removal is always empty-directory-only.
        // A new file created during sync must never be erased by a stale folder scan.
        host_remove.sort_by_key(|p| std::cmp::Reverse(p.split('/').count()));
        guest_remove.sort_by_key(|p| std::cmp::Reverse(p.split('/').count()));
        for (create, paths) in [(true, host_create), (false, host_remove)] {
            for path in paths {
                let target = two_way::destination(&self.folder, &path)?;
                let result = if create { fs::create_dir_all(target) } else { fs::remove_dir(target) };
                match result {
                    Ok(()) => { if create { next.insert(path); } else { next.remove(&path); } changed += 1; }
                    Err(e) if !create && e.kind() == io::ErrorKind::NotFound => { next.remove(&path); }
                    Err(e) => { let message = format!("Folder sync kept {}: {e}", clean(&path)); if self.conflicts.insert(message.clone()) { ui::warn(&message); } }
                }
            }
        }
        for (create, paths) in [(true, guest_create), (false, guest_remove)] {
            for batch in paths.chunks(64) {
                let reply = self.receiver.rpc(json!({"op":"directories","create":create,"paths":batch})).await?;
                for path in batch {
                    if let Some(error) = reply["errors"][path].as_str() {
                        let message = format!("Folder sync kept {}: {}", clean(path), clean(error));
                        if self.conflicts.insert(message.clone()) { ui::warn(&message); }
                    } else { if create { next.insert(path.clone()); } else { next.remove(path); } changed += 1; }
                }
            }
        }
        if self.saved.directories != next { self.saved.directories = next; self.saved.save(&self.state_path)?; }
        Ok(changed)
    }
}
