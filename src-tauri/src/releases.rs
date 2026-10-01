use yougori_cli::release_notice::{self, ReleaseStatus};

#[tauri::command]
pub async fn check_release_update(force: bool) -> Result<Option<ReleaseStatus>, String> {
    if !force && !release_notice::automatic_allowed() {
        return Ok(None);
    }
    release_notice::check(env!("CARGO_PKG_VERSION"), false, force)
        .await
        .map(Some)
}

#[tauri::command]
pub async fn remind_release_update_later(version: String) -> Result<(), String> {
    let status = release_notice::check(env!("CARGO_PKG_VERSION"), false, false).await?;
    if status.latest == version {
        release_notice::remind_later(&status)?;
    }
    Ok(())
}

#[tauri::command]
pub fn open_release_downloads() -> Result<(), String> {
    release_notice::open_downloads(false)
}
