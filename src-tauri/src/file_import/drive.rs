use super::*;
use std::io::{Seek, SeekFrom, Write};

pub(super) fn capacity(plan: &CopyPlan) -> u64 {
    (plan
        .bytes
        .saturating_add(plan.bytes / 20)
        .saturating_add((plan.entry_count as u64).saturating_mul(16384))
        .saturating_add(64 * 1024 * 1024))
    .div_ceil(1024 * 1024)
        * 1024
        * 1024
}

pub(super) fn write_drive(
    plan: &CopyPlan,
    destination: &Path,
    progress: impl Fn(CopyProgress),
) -> Result<(), String> {
    // FAT is readable by Windows and Linux without installing a guest agent.
    // Reject incompatible names before writing anything, including case clashes.
    for entry in plan.entries()? {
        let entry = entry?;
        let path = entry.relative.to_string_lossy().replace('\\', "/");
        if path.split('/').any(|name| {
            name.ends_with(['.', ' '])
                || name.encode_utf16().count() > 255
                || name.chars().any(|c| c < ' ' || "<>:\"\\|?*".contains(c))
        }) {
            return Err("This folder contains names that cannot coexist on the VM's Windows-compatible import drive. Rename those files or copy them separately.".into());
        }
        if !entry.directory && entry.bytes > u32::MAX as u64 {
            return Err("The VM import drive supports files smaller than 4 GB. Split larger files before dropping them.".into());
        }
    }
    let mut disk = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|e| e.to_string())?;
    // Reserve space for FAT tables, long names, cluster rounding and later edits.
    disk.set_len(capacity(plan))
        .map_err(|e| format!("Not enough space for the imported-files drive: {e}"))?;
    fatfs::format_volume(
        &mut disk,
        fatfs::FormatVolumeOptions::new().volume_label(*b"YOUGORI    "),
    )
    .map_err(|e| format!("Prepare imported-files drive: {e}"))?;
    disk.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let filesystem =
        fatfs::FileSystem::new(disk, fatfs::FsOptions::new()).map_err(|e| e.to_string())?;
    {
        let root = filesystem.root_dir();
        let mut completed = 0;
        let mut last = Instant::now();
        for entry in plan.entries()? {
            let entry = entry?;
            let name = entry.relative.to_string_lossy().replace('\\', "/");
            if root.open_file(&name).is_ok() || root.open_dir(&name).is_ok() {
                return Err(format!(
                    "Two source names resolve to the same file on the imported drive: {name}"
                ));
            }
            if entry.directory {
                root.create_dir(&name)
                    .map_err(|e| format!("Copy folder {name}: {e}"))?;
            } else {
                let source = open_source(&entry)?;
                let mut output = root
                    .create_file(&name)
                    .map_err(|e| format!("Copy file {name}: {e}"))?;
                let copied = io::copy(
                    &mut CopyReader {
                        file: source,
                        completed: &mut completed,
                        last: &mut last,
                        total: plan.bytes,
                        progress: &progress,
                    }
                    .take(entry.bytes),
                    &mut output,
                )
                .map_err(|e| format!("Copy file {name}: {e}"))?;
                if copied != entry.bytes {
                    return Err(format!("{name} changed during the copy. Drop it again when it has finished saving."));
                }
                output.flush().map_err(|e| e.to_string())?;
            }
        }
    }
    filesystem.unmount().map_err(|e| e.to_string())
}
