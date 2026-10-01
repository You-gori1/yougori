//! `yougori update` and `yougori uninstall`.
//!
//! The release manifest (served by yougori.com, written by scripts/release-manifest.mjs):
//! `{"version":"1.2.0","notes":"…","assets":{"windows-x86_64":{"url":"https://…/Yougori_1.2.0_x64-setup.exe","sha256":"…"},
//!   "windows-x86_64-engine":{"url":"https://…/yougori-engine-1.2.0-windows-x86_64.zip","sha256":"…"}}}`
//! A download must match its SHA-256 and, on Windows, carry a valid signature from the same
//! publisher as the installed copy, so a compromised website alone cannot ship an update.
//! `-engine` assets are archives of the standalone engine for installs without the desktop app.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::Write;

pub const DEFAULT_MANIFEST: &str = "https://yougori.com/releases/latest.json";

pub fn platform() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "aarch64") => "windows-aarch64",
        ("windows", _) => "windows-x86_64",
        ("macos", "aarch64") => "macos-aarch64",
        ("macos", _) => "macos-x86_64",
        (_, "aarch64") => "linux-aarch64",
        _ => "linux-x86_64",
    }
}

fn manifest_url() -> String {
    std::env::var("YOUGORI_RELEASES_URL").ok().filter(|u| u.starts_with("https://")).unwrap_or_else(|| DEFAULT_MANIFEST.into())
}

/// The release asset for this install: the desktop installer, or the standalone engine archive
/// when only the engine is installed.
fn asset_key(engine_only: bool) -> String {
    if engine_only { format!("{}-engine", platform()) } else { platform().to_owned() }
}

pub(crate) fn engine_only_install() -> bool {
    crate::client::desktop_executable(None).is_err() && crate::client::engine_executable().is_some()
}

/// Compares this CLI's version with a release manifest.
pub fn compare(manifest: &Value, current: &str) -> Result<Value, String> {
    compare_asset(manifest, current, &asset_key(false))
}

fn compare_asset(manifest: &Value, current: &str, key: &str) -> Result<Value, String> {
    let latest = manifest["version"].as_str().ok_or("The release manifest has no version")?;
    let newer = semver::Version::parse(latest).map_err(|_| "The release manifest has an invalid version")?
        > semver::Version::parse(current).map_err(|e| e.to_string())?;
    let asset = &manifest["assets"][key];
    Ok(json!({
        "current": current, "latest": latest, "updateAvailable": newer, "notes": manifest["notes"],
        "platform": key, "download": asset["url"], "sha256": asset["sha256"],
    }))
}

async fn manifest() -> Result<Value, String> {
    let response = reqwest::Client::new().get(manifest_url()).timeout(std::time::Duration::from_secs(20)).send().await
        .map_err(|e| format!("Cannot reach the release server: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("The release server answered {}", response.status()));
    }
    response.json().await.map_err(|_| "The release manifest is not valid JSON".into())
}

pub async fn check() -> Result<Value, String> {
    compare_asset(&manifest().await?, env!("CARGO_PKG_VERSION"), &asset_key(engine_only_install()))
}

/// Streams the installer to disk while hashing it; nothing runs unless the hash matches.
async fn download(url: &str, expected: &str, path: &std::path::Path) -> Result<(), String> {
    if !url.starts_with("https://") || expected.len() != 64 {
        return Err("The release manifest must give an HTTPS download and a SHA-256".into());
    }
    let mut response = reqwest::Client::new().get(url).send().await.map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("Download failed: {}", response.status()));
    }
    let mut file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut total = 0u64;
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        total += chunk.len() as u64;
        if total > 8 * 1024 * 1024 * 1024 {
            return Err("The installer is larger than 8 GB".into());
        }
        hash.update(&chunk);
        file.write_all(&chunk).map_err(|e| e.to_string())?;
    }
    file.sync_all().map_err(|e| e.to_string())?;
    let actual = hash.finalize().iter().map(|b| format!("{b:02x}")).collect::<String>();
    if !actual.eq_ignore_ascii_case(expected) {
        let _ = std::fs::remove_file(path);
        return Err("The download does not match the published SHA-256; nothing was installed".into());
    }
    Ok(())
}

#[cfg(windows)]
fn signer(path: &std::path::Path) -> Result<String, String> {
    // Authenticode: the status must be Valid; the subject identifies the publisher.
    let script = "$s = Get-AuthenticodeSignature -LiteralPath $args[0]; if ($s.Status -ne 'Valid') { exit 3 }; $s.SignerCertificate.Subject";
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .arg(path)
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(format!("{} is not validly signed", path.display()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub async fn install(yes: bool) -> Result<Value, String> {
    let status = check().await?;
    if status["updateAvailable"] != true {
        return Ok(json!({"upToDate": true, "version": status["current"]}));
    }
    let url = status["download"].as_str().ok_or_else(|| format!("No {} download in this release yet", status["platform"].as_str().unwrap_or("")))?.to_owned();
    if !yes {
        return Ok(json!({"updateAvailable": true, "latest": status["latest"], "notes": status["notes"], "next": "Run `yougori update --yes` to download and install it. Yougori stops its workloads while it updates."}));
    }
    let folder = tempfile::Builder::new().prefix("yougori-update-").tempdir().map_err(|e| e.to_string())?.keep();
    let name = url.rsplit('/').next().filter(|n| !n.is_empty() && !n.contains(['\\', ':'])).unwrap_or("Yougori-setup");
    let installer = folder.join(name);
    download(&url, status["sha256"].as_str().unwrap_or(""), &installer).await?;
    if engine_only_install() {
        return replace_engine(&installer, &folder, &status).await;
    }
    #[cfg(windows)]
    {
        let current = crate::client::desktop_executable(None)?;
        let expected = signer(&current).map_err(|_| "This Yougori is a development build without a signature; update it from source instead".to_string())?;
        if signer(&installer)? != expected {
            return Err("The installer is signed by a different publisher than your Yougori; nothing was installed".into());
        }
        // Workloads stop gracefully before files are replaced.
        let mut quit = crate::client::request("app_quit", json!({}));
        quit.confirmed = true;
        let _ = crate::client::call(&quit).await;
        tokio::time::sleep(std::time::Duration::from_secs(4)).await;
        std::process::Command::new(&installer).arg("/S").spawn().map_err(|e| format!("Cannot start the installer: {e}"))?;
        return Ok(json!({"installing": status["latest"], "installer": installer, "next": "The installer runs in the background. Run `yougori doctor` when it finishes."}));
    }
    #[cfg(not(windows))]
    Ok(json!({"downloaded": installer, "version": status["latest"], "next": "Open the downloaded installer to finish updating."}))
}

async fn quit_engine() {
    let mut quit = crate::client::request("app_quit", json!({}));
    quit.confirmed = true;
    if crate::client::call(&quit).await.is_ok() {
        // Workloads stop gracefully before files are replaced.
        for _ in 0..120 {
            if crate::client::call(&crate::client::request("app_status", json!({}))).await.is_err() { break; }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }
}

/// Unpacks a verified engine archive over the engine's folder. Files in use are renamed aside
/// first (Windows cannot overwrite a running executable, such as this CLI).
async fn replace_engine(archive: &std::path::Path, folder: &std::path::Path, status: &Value) -> Result<Value, String> {
    let current = crate::client::engine_executable().ok_or("The standalone engine was not found")?;
    let target = current.parent().ok_or("Cannot locate the engine folder")?.to_path_buf();
    let staged = folder.join("unpacked");
    std::fs::create_dir_all(&staged).map_err(|e| e.to_string())?;
    // Windows' own bsdtar reads zip archives; a GNU tar earlier on PATH would not.
    let tar = if cfg!(windows) { std::path::Path::new(&std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into())).join("System32").join("tar.exe") } else { "tar".into() };
    let unpacked = std::process::Command::new(tar).arg("-xf").arg(archive).arg("-C").arg(&staged).status().map_err(|e| format!("Cannot unpack the update: {e}"))?;
    if !unpacked.success() {
        return Err("Cannot unpack the update; nothing was changed".into());
    }
    let engine_name = current.file_name().ok_or("Invalid engine path")?;
    let new_engine = staged.join(engine_name);
    if !new_engine.is_file() {
        return Err("The update does not contain the engine; nothing was changed".into());
    }
    #[cfg(windows)]
    {
        let expected = signer(&current).map_err(|_| "This engine is a development build without a signature; update it from source instead".to_string())?;
        for binary in [new_engine.clone(), staged.join("cli").join("yougori.exe")] {
            if binary.is_file() && signer(&binary)? != expected {
                return Err("The update is signed by a different publisher than your Yougori; nothing was changed".into());
            }
        }
    }
    quit_engine().await;
    copy_over(&staged, &target)?;
    Ok(json!({"updated": status["latest"], "folder": target, "next": "Run `yougori doctor` to start the updated engine and check it."}))
}

fn copy_over(from: &std::path::Path, to: &std::path::Path) -> Result<(), String> {
    for entry in std::fs::read_dir(from).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let destination = to.join(entry.file_name());
        if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            std::fs::create_dir_all(&destination).map_err(|e| e.to_string())?;
            copy_over(&entry.path(), &destination)?;
        } else {
            if destination.exists() && std::fs::copy(entry.path(), &destination).is_ok() { continue; }
            if destination.exists() {
                let aside = destination.with_extension(format!("old-{}", std::process::id()));
                std::fs::rename(&destination, &aside).map_err(|e| format!("{} is in use: {e}", destination.display()))?;
            }
            std::fs::copy(entry.path(), &destination).map_err(|e| format!("Cannot write {}: {e}", destination.display()))?;
        }
    }
    Ok(())
}

/// Removes the app but keeps environments and data unless the user deletes the data folder themselves.
pub async fn uninstall(yes: bool) -> Result<Value, String> {
    if !yes {
        return Ok(json!({"next": "Run `yougori uninstall --yes` to remove Yougori. Environments and their data stay on disk; the result shows where."}));
    }
    let data = crate::public::call("get_storage_location", json!({})).await.ok();
    if let Ok(mut settings) = crate::public::call("get_platform_state", json!({})).await.map(|s| s["settings"].clone()) {
        settings["launchAtStartup"] = false.into();
        let _ = crate::public::call("update_settings", json!({"settings": settings})).await;
    }
    let mut quit = crate::client::request("app_quit", json!({}));
    quit.confirmed = true;
    let _ = crate::client::call(&quit).await;
    #[cfg(windows)]
    {
        tokio::time::sleep(std::time::Duration::from_secs(4)).await;
        let script = "Get-ChildItem HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall, HKLM:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall -ErrorAction SilentlyContinue | Get-ItemProperty | Where-Object { $_.DisplayName -eq 'Yougori' } | Select-Object -First 1 -ExpandProperty UninstallString";
        let output = std::process::Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command", script]).output().map_err(|e| e.to_string())?;
        let command = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        let executable = command.trim_matches('"').split("\" ").next().unwrap_or("").trim_matches('"').to_owned();
        if executable.is_empty() || !std::path::Path::new(&executable).is_file() {
            return Err("Yougori's uninstaller was not found. Remove it from Windows Settings → Apps.".into());
        }
        std::process::Command::new(&executable).arg("/S").spawn().map_err(|e| e.to_string())?;
        return Ok(json!({"uninstalling": true, "dataKept": data, "next": "Environments and their data were kept. Delete that folder yourself only if you no longer need them."}));
    }
    #[cfg(not(windows))]
    Ok(json!({"stopped": true, "dataKept": data, "next": "Remove the Yougori app (drag it to the Trash on macOS, or remove the yougori package using your package manager on Debian/Ubuntu). Environments and their data were kept."}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn newer_releases_are_detected_for_this_platform() {
        let manifest = json!({"version":"9.0.0","notes":"n","assets":{platform():{"url":"https://example.com/setup.exe","sha256":"ab"},asset_key(true):{"url":"https://example.com/engine.zip","sha256":"cd"}}});
        let status = compare(&manifest, "1.0.0").unwrap();
        assert_eq!((status["updateAvailable"].as_bool(), status["download"].as_str()), (Some(true), Some("https://example.com/setup.exe")));
        let engine = compare_asset(&manifest, "1.0.0", &asset_key(true)).unwrap();
        assert_eq!((engine["download"].as_str(), engine["platform"].as_str()), (Some("https://example.com/engine.zip"), Some(asset_key(true).as_str())));
        assert_eq!(compare(&json!({"version":"1.0.0"}), "1.0.0").unwrap()["updateAvailable"], false);
        assert!(compare(&json!({"version":"latest"}), "1.0.0").is_err());
    }
    #[test]
    fn engine_updates_replace_files_and_keep_other_ones() {
        let from = tempfile::tempdir().unwrap();
        let to = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(from.path().join("runtime/appliance")).unwrap();
        std::fs::write(from.path().join("runtime/appliance/initramfs"), b"new").unwrap();
        std::fs::write(from.path().join("yougori-engine"), b"engine 2").unwrap();
        std::fs::create_dir_all(to.path().join("runtime/appliance")).unwrap();
        std::fs::write(to.path().join("runtime/appliance/initramfs"), b"old").unwrap();
        std::fs::write(to.path().join("keep.txt"), b"mine").unwrap();
        copy_over(from.path(), to.path()).unwrap();
        assert_eq!(std::fs::read(to.path().join("runtime/appliance/initramfs")).unwrap(), b"new");
        assert_eq!(std::fs::read(to.path().join("yougori-engine")).unwrap(), b"engine 2");
        assert_eq!(std::fs::read(to.path().join("keep.txt")).unwrap(), b"mine");
    }
    #[tokio::test]
    async fn downloads_must_be_https_with_a_hash() {
        let path = std::env::temp_dir().join("yougori-update-test");
        assert!(download("http://example.com/x", &"0".repeat(64), &path).await.is_err());
        assert!(download("https://example.com/x", "short", &path).await.is_err());
    }
}
