use crate::{commands, models::{AppSettings, EnvironmentStatus, ThemePreference}, runtime::RuntimeManager, store::PlatformStore};
use tauri::{Manager, State};
use crate::AppHandle;

pub fn validate(settings: &AppSettings, store: &PlatformStore) -> Result<(), String> {
    if matches!(settings.theme, ThemePreference::Custom) && settings.custom_theme_colors.is_none() {
        return Err("Choose four colors for the custom theme".into());
    }
    if let Some(colors) = &settings.custom_theme_colors {
        for value in [&colors.background, &colors.surface, &colors.accent, &colors.detail] {
            if value.len() != 7 || !value.starts_with('#') || !value.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit) {
                return Err("Custom theme colors must be six-digit hex values such as #315B8C".into());
            }
        }
    }
    let state = store.snapshot()?;
    if settings.auto_start_environment_ids.len() > state.environments.len() || settings.auto_start_environment_ids.iter().any(|id| !state.environments.iter().any(|e| &e.id == id)) {
        return Err("Automatic startup must reference existing environments".into());
    }
    Ok(())
}

/// `headless` starts the engine without the dashboard (for CLI-first use).
pub fn set_launch_at_startup(enabled: bool, headless: bool) -> Result<(), String> {
    let executable = std::env::current_exe().map_err(|e|e.to_string())?;
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    let flag = if headless { " --headless" } else { "" };
    #[cfg(target_os="windows")]
    {
        use std::os::windows::process::CommandExt;
        let reg = std::path::PathBuf::from(std::env::var_os("SystemRoot").ok_or("Windows system directory unavailable")?).join("System32/reg.exe");
        let mut command = std::process::Command::new(reg);
        command.creation_flags(0x08000000);
        command.args([if enabled {"add"} else {"delete"}, "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run", "/v", "Yougori", "/f"]);
        if enabled { command.args(["/t", "REG_SZ", "/d"]).arg(format!("\"{}\"{flag}", executable.display())); }
        let result = command.output().map_err(|e|e.to_string())?;
        if !result.status.success() {
            // Deleting an already absent value is idempotent; other registry errors
            // remain visible rather than claiming startup configuration succeeded.
            let check = std::process::Command::new(std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32/reg.exe")).creation_flags(0x08000000).args(["query","HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run","/v","Yougori"]).output().map_err(|e|e.to_string())?;
            if enabled || check.status.success() { return Err(format!("Could not update automatic launch: {}",String::from_utf8_lossy(&result.stderr))); }
        }
    }
    #[cfg(target_os="linux")]
    {
        let root = std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from).or_else(||std::env::var_os("HOME").map(|p|std::path::PathBuf::from(p).join(".config"))).ok_or("User configuration directory unavailable")?.join("autostart");
        std::fs::create_dir_all(&root).map_err(|e|e.to_string())?;
        let path=root.join("yougori.desktop");
        if enabled {
            let exe=executable.to_string_lossy().replace('\\',"\\\\").replace('"',"\\\"").replace('`',"\\`").replace('$',"\\$").replace('%',"%%");
            if exe.contains(['\n','\r']) { return Err("Executable path cannot be used for startup".into()); }
            std::fs::write(path,format!("[Desktop Entry]\nType=Application\nName=Yougori\nExec=\"{exe}\"{flag}\nTerminal=false\n")).map_err(|e|e.to_string())?;
        } else if path.exists() { std::fs::remove_file(path).map_err(|e|e.to_string())?; }
    }
    #[cfg(target_os="macos")]
    {
        let root=std::path::PathBuf::from(std::env::var_os("HOME").ok_or("User directory unavailable")?).join("Library/LaunchAgents");
        std::fs::create_dir_all(&root).map_err(|e|e.to_string())?;
        let path=root.join("app.yougori.startup.plist");
        if enabled { let exe=executable.to_string_lossy().replace('&',"&amp;").replace('<',"&lt;").replace('>',"&gt;"); std::fs::write(path,format!("<?xml version=\"1.0\"?><plist version=\"1.0\"><dict><key>Label</key><string>app.yougori.startup</string><key>ProgramArguments</key><array><string>{exe}</string>{}</array><key>RunAtLoad</key><true/></dict></plist>", if headless {"<string>--headless</string>"} else {""})).map_err(|e|e.to_string())?; } else if path.exists(){ std::fs::remove_file(path).map_err(|e|e.to_string())?; }
    }
    Ok(())
}

/// A previous run that was killed can leave guests holding their disks. Stop
/// them at launch, while no environment of this run is using them yet, so the
/// user never meets a busy runtime. Runtimes owned by another live Yougori
/// instance are left alone by the recovery check itself.
async fn reclaim_abandoned_runtimes(app: &AppHandle) {
    let store = app.state::<PlatformStore>();
    let runtime = app.state::<RuntimeManager>();
    let Ok(state) = store.snapshot() else { return };
    let stale: Vec<String> = state
        .environments
        .iter()
        .filter(|environment| matches!(environment.status, EnvironmentStatus::Running | EnvironmentStatus::Provisioning | EnvironmentStatus::Paused))
        .map(|environment| environment.id.clone())
        .collect();
    for id in stale {
        if let Err(error) = runtime.recover_orphaned_vm(&id).await {
            // Nothing to reclaim, or another live instance owns it: both are fine.
            if error.contains("YOUGORI_") || error.contains("OPENDOCK_") {
                eprintln!("Left {id} to its owning Yougori instance: {error}");
            }
        }
    }
    if let Err(error) = runtime.recover_orphaned_container_runtime().await {
        eprintln!("Container runtime reclaim at startup: {error}");
    }
}

pub fn start(app: &AppHandle) {
    let app=app.clone();
    let startup=app.clone();
    tauri::async_runtime::spawn(async move {
        reclaim_abandoned_runtimes(&startup).await;
        let ids=startup.state::<PlatformStore>().snapshot().map(|s|s.settings.auto_start_environment_ids).unwrap_or_default();
        for id in ids {
            if let Err(error)=commands::set_environment_status(id.clone(),EnvironmentStatus::Running,startup.state::<PlatformStore>(),startup.state::<RuntimeManager>()).await {
                let _=startup.state::<PlatformStore>().mutate(|s|{if let Some(e)=s.environments.iter_mut().find(|e|e.id==id){e.last_error=Some(format!("Automatic startup failed: {error}"));} Ok(())});
            }
        }
    });
    std::thread::spawn(move || {
        #[cfg(unix)] let mut inhibitor:Option<std::process::Child>=None;
        let mut previous=false;
        loop {
            let awake=app.state::<PlatformStore>().snapshot().is_ok_and(|s|s.settings.keep_awake && s.environments.iter().any(|e|matches!(e.status,EnvironmentStatus::Running|EnvironmentStatus::Provisioning)));
            if awake != previous {
                #[cfg(target_os="windows")]
                unsafe { use windows_sys::Win32::System::Power::{SetThreadExecutionState,ES_CONTINUOUS,ES_SYSTEM_REQUIRED}; SetThreadExecutionState(ES_CONTINUOUS | if awake {ES_SYSTEM_REQUIRED}else{0}); }
                #[cfg(unix)] {
                    if let Some(mut child)=inhibitor.take(){let _=child.kill();let _=child.wait();}
                    if awake {
                        #[cfg(target_os="linux")] let mut command={let mut c=std::process::Command::new("systemd-inhibit");c.args(["--what=sleep","--mode=block","--why=Yougori environments are running","/bin/sh","-c","while kill -0 \"$1\" 2>/dev/null; do sleep 2; done","yougori-inhibit",&std::process::id().to_string()]);c};
                        #[cfg(target_os="macos")] let mut command={let mut c=std::process::Command::new("/usr/bin/caffeinate");c.args(["-i","-w",&std::process::id().to_string()]);c};
                        inhibitor=command.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().ok();
                    }
                }
                previous=awake;
            }
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
    });
}

#[tauri::command]
pub async fn finish_app_close(keep_running: bool, app: AppHandle, store: State<'_,PlatformStore>, runtime: State<'_,RuntimeManager>) -> Result<(),String> {
    if keep_running { return Err("Closing Yougori stops the application. Cancel to keep the window open.".into()); }
    let environments=store.snapshot()?.environments;
    if environments.iter().any(|e|e.status==EnvironmentStatus::Provisioning){return Err("Wait for environment creation to finish before quitting".into());}
    for e in environments.into_iter().filter(|e|!crate::peer_sharing::is_shared(e)&&matches!(e.status,EnvironmentStatus::Running|EnvironmentStatus::Paused)) {
        commands::set_environment_status(e.id,EnvironmentStatus::Stopped,store.clone(),runtime.clone()).await?;
    }
    // Stop the host CLI before scheduling app exit. In particular, an active
    // coding-agent process must release its conversation when the UI quits.
    let terminal_app = app.clone();
    tokio::task::spawn_blocking(move || terminal_app.state::<crate::host_terminal::HostTerminalManager>().shutdown())
        .await.map_err(|error| format!("Close host terminals: {error}"))?;
    tauri::async_runtime::spawn(async move { tokio::time::sleep(std::time::Duration::from_millis(200)).await;crate::exit_engine(&app, 0); });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn startup_cannot_reference_unknown_environments(){let dir=tempfile::tempdir().unwrap();let store=PlatformStore::load(dir.path().join("state.json")).unwrap();let mut settings=store.snapshot().unwrap().settings;settings.auto_start_environment_ids=vec!["missing".into()];assert!(validate(&settings,&store).is_err());}

    #[test]
    fn custom_theme_requires_safe_complete_hex_colors() {
        let dir = tempfile::tempdir().unwrap();
        let store = PlatformStore::load(dir.path().join("state.json")).unwrap();
        let mut settings = store.snapshot().unwrap().settings;
        settings.theme = ThemePreference::Custom;
        assert!(validate(&settings, &store).is_err());
        settings.custom_theme_colors = Some(crate::models::CustomThemeColors {
            background: "#F5EBDD".into(), surface: "#D8C4AD".into(),
            accent: "#315B8C".into(), detail: "#413333".into(),
        });
        assert!(validate(&settings, &store).is_ok());
        settings.custom_theme_colors.as_mut().unwrap().accent = "red; background: black".into();
        assert!(validate(&settings, &store).is_err());
    }
}
