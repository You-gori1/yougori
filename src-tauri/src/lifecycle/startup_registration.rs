use crate::models::AppSettings;
use serde_json::{json, Value};

/// Reports the supported promise. User sign-in startup never enables lingering,
/// a privileged boot service, or an additional engine process in this session.
pub(super) fn describe(settings: &AppSettings) -> Value {
    #[cfg(target_os = "linux")]
    if settings.launch_at_startup && settings.startup_headless {
        return describe_linux(systemctl);
    }
    #[cfg(target_os = "windows")]
    if settings.launch_at_startup {
        let expected = std::env::current_exe().map(|executable|format!("\"{}\"{}",executable.display(),if settings.startup_headless {" --headless --startup-sign-in"} else {" --startup-sign-in"}));
        let status = match (read_windows_registration(),expected) {
            (Ok(Some(command)),Ok(expected)) if command == expected => "registered",
            (Ok(Some(_)),_) => "stale",
            (Ok(None),_) => "missing",
            (Err(_),_) => "unverified",
        };
        return json!({"status":status,"mechanism":"windowsUserRun","trigger":"signIn","startsBeforeSignIn":false,"recoveryAction":if status == "registered" {None} else {Some("Enable Yougori sign-in startup again to verify and restore the current executable and startup mode.")}});
    }
    #[cfg(target_os = "windows")]
    let mechanism = "windowsUserRun";
    #[cfg(target_os = "macos")]
    let mechanism = "launchAgent";
    #[cfg(target_os = "linux")]
    let mechanism = "xdgDesktop";
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    let mechanism = "unsupported";
    json!({"status":if settings.launch_at_startup {"registered"} else {"disabled"},"mechanism":mechanism,"trigger":"signIn","startsBeforeSignIn":false})
}

/// RegGetValue distinguishes a genuinely absent value from access/registry
/// failures without parsing localized reg.exe output or returning its content.
#[cfg(target_os="windows")]
pub(super) fn read_windows_registration() -> Result<Option<String>,String> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name="advapi32")]
    unsafe extern "system" {
        fn RegGetValueW(key:*mut std::ffi::c_void,subkey:*const u16,value:*const u16,flags:u32,kind:*mut u32,data:*mut std::ffi::c_void,size:*mut u32) -> i32;
    }
    let subkey = std::ffi::OsStr::new("Software\\Microsoft\\Windows\\CurrentVersion\\Run").encode_wide().chain(Some(0)).collect::<Vec<_>>();
    let name = std::ffi::OsStr::new("Yougori").encode_wide().chain(Some(0)).collect::<Vec<_>>();
    let mut buffer = vec![0u16;32768];
    let mut size = (buffer.len() * 2) as u32;
    let result = unsafe {RegGetValueW(0x80000001u32 as i32 as isize as *mut std::ffi::c_void,subkey.as_ptr(),name.as_ptr(),2,std::ptr::null_mut(),buffer.as_mut_ptr().cast(),&mut size)};
    match registry_query_outcome(result)? {
        false => Ok(None),
        true => {
            let count = (size as usize / 2).min(buffer.len());
            let mut value = &buffer[..count];
            if value.last() == Some(&0) {value = &value[..value.len()-1];}
            String::from_utf16(value).map(Some).map_err(|_|"Startup registration is not a valid UTF-16 command".into())
        }
    }
}
#[cfg(any(target_os="windows",test))]
fn registry_query_outcome(code:i32) -> Result<bool,String> {
    match code {0=>Ok(true),2|3=>Ok(false),_=>Err(format!("Could not verify Windows sign-in startup registration (OS code {code}); the entry was not assumed absent"))}
}

#[cfg(any(target_os = "linux", test))]
const UNIT: &str = "yougori-engine.service";
#[cfg(any(target_os = "linux", test))]
const MANAGED: &str = "# Managed by Yougori: user sign-in startup";

#[cfg(any(target_os = "linux", test))]
fn unit_text(executable: &std::path::Path) -> Result<String, String> {
    let executable = executable.to_str().ok_or("Startup executable path is not valid UTF-8")?;
    if executable.contains(['\n', '\r', '\0']) { return Err("Executable path cannot be used for startup".into()); }
    // systemd parses ExecStart itself: this is never a shell command. Escape
    // specifiers/variables so a literal installation path is used unchanged.
    let quoted = executable.replace('\\', "\\\\").replace('"', "\\\"").replace('%', "%%").replace('$', "$$");
    Ok(format!("{MANAGED}\n[Unit]\nDescription=Yougori engine after user sign-in\n\n[Service]\nType=simple\nExecStart=\"{quoted}\" --headless --startup-sign-in\nRestart=on-failure\nRestartSec=10\nTimeoutStopSec=45\n\n[Install]\nWantedBy=default.target\n"))
}

#[cfg(any(target_os = "linux", test))]
fn describe_linux<F>(mut run: F) -> Value where F: FnMut(&[&str]) -> Result<String, String> {
    match run(&["is-system-running"]) {
        Err(_) => json!({"status":"unsupported","mechanism":"systemdUser","trigger":"signIn","startsBeforeSignIn":false,"recoveryAction":"Enable a systemd user session at sign-in, then enable Yougori background startup again. Before-sign-in boot startup is not configured."}),
        Ok(_) => match run(&["is-enabled", UNIT]) {
            Ok(state) if state.trim() == "enabled" => json!({"status":"registered","mechanism":"systemdUser","trigger":"signIn","startsBeforeSignIn":false,"requires":"systemd user manager at sign-in"}),
            _ => json!({"status":"missing","mechanism":"systemdUser","trigger":"signIn","startsBeforeSignIn":false,"recoveryAction":"Enable Yougori background startup again to restore its user sign-in service registration."}),
        },
    }
}

#[cfg(any(target_os = "linux", test))]
fn desktop_text(executable: &std::path::Path) -> Result<String, String> {
    let executable = executable.to_str().ok_or("Startup executable path is not valid UTF-8")?;
    if executable.contains(['\n', '\r', '\0']) { return Err("Executable path cannot be used for startup".into()); }
    let quoted = executable.replace('\\', "\\\\").replace('"', "\\\"").replace('`', "\\`").replace('$', "\\$").replace('%', "%%");
    Ok(format!("{MANAGED}\n[Desktop Entry]\nType=Application\nName=Yougori\nExec=\"{quoted}\" --startup-sign-in\nTerminal=false\n"))
}

#[cfg(any(target_os = "linux", test))]
fn read_registration(path: &std::path::Path, legacy_desktop: bool) -> Result<Option<Vec<u8>>, String> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
        Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 64 * 1024 => return Err("Automatic startup registration must be a regular managed file".into()),
        Ok(_) => {}
    }
    let contents = std::fs::read(path).map_err(|error| error.to_string())?;
    let text = String::from_utf8_lossy(&contents);
    if !text.starts_with(MANAGED) && !(legacy_desktop && text.lines().any(|line| line == "Name=Yougori") && text.lines().any(|line| line.starts_with("Exec="))) {
        return Err(format!("Automatic startup would overwrite an unmanaged registration at {}", path.display()));
    }
    Ok(Some(contents))
}

#[cfg(any(target_os = "linux", test))]
fn write_registration(path: &std::path::Path, contents: Option<&[u8]>) -> Result<(), String> {
    if let Some(contents) = contents {
        std::fs::create_dir_all(path.parent().ok_or("Startup path has no parent")?).map_err(|error| error.to_string())?;
        std::fs::write(path, contents).map_err(|error| error.to_string())
    } else {
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }
}

/// Transactional file registration with injected bounded user-manager commands.
/// No --now, linger, sudo, system-wide unit, or process termination is used.
#[cfg(any(target_os = "linux", test))]
fn configure_linux_in<F>(config: &std::path::Path, executable: &std::path::Path, enabled: bool, headless: bool, mut run: F) -> Result<(), String>
where F: FnMut(&[&str]) -> Result<String, String> {
    let unit = config.join("systemd/user").join(UNIT);
    let desktop = config.join("autostart/yougori.desktop");
    let previous_unit = read_registration(&unit, false)?;
    let previous_desktop = read_registration(&desktop, true)?;
    let use_systemd = enabled && headless;
    let next_unit = use_systemd.then(|| unit_text(executable)).transpose()?;
    let next_desktop = (enabled && !headless).then(|| desktop_text(executable)).transpose()?;
    if use_systemd {
        run(&["is-system-running"]).map_err(|_| "[YOUGORI_STARTUP_UNSUPPORTED] Background sign-in startup requires a systemd user manager. Enable a user session, then retry; no before-sign-in boot service is installed.".to_string())?;
    }
    let result = (|| {
        if previous_unit.is_some() && !use_systemd { run(&["disable", UNIT])?; }
        write_registration(&unit, next_unit.as_ref().map(|text| text.as_bytes()))?;
        write_registration(&desktop, next_desktop.as_ref().map(|text| text.as_bytes()))?;
        if use_systemd || previous_unit.is_some() {
            run(&["daemon-reload"])?;
            if use_systemd { run(&["enable", UNIT])?; }
        }
        Ok(())
    })();
    if let Err(error) = result {
        // Restore prior registrations and best-effort enablement. Never turn a
        // failed settings write into a different saved startup configuration.
        let rollback = (|| {
            if use_systemd { let _ = run(&["disable", UNIT]); }
            write_registration(&unit, previous_unit.as_deref())?;
            write_registration(&desktop, previous_desktop.as_deref())?;
            if use_systemd || previous_unit.is_some() { run(&["daemon-reload"])?; }
            if previous_unit.is_some() { run(&["enable", UNIT])?; }
            Ok::<_, String>(())
        })();
        return Err(match rollback { Ok(()) => error, Err(rollback) => format!("{error}; startup registration rollback needs reconciliation: {rollback}") });
    }
    Ok(())
}

#[cfg(target_os = "linux")]
pub(super) fn configure_linux(executable: &std::path::Path, enabled: bool, headless: bool) -> Result<(), String> {
    let config = std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".config")))
        .ok_or("User configuration directory unavailable")?;
    configure_linux_in(&config, executable, enabled, headless, systemctl)
}

#[cfg(target_os = "linux")]
fn systemctl(arguments: &[&str]) -> Result<String, String> {
    let mut command = std::process::Command::new("systemctl");
    command.arg("--user").args(arguments);
    let output = bounded_output(&mut command)?;
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    // A degraded manager can still register services; absence/offline cannot.
    if output.status.success() || (arguments == ["is-system-running"] && matches!(text.as_str(), "degraded" | "starting" | "initializing")) { return Ok(text); }
    Err(format!("User startup manager rejected {}: {}", arguments.join(" "), super::safe_diagnostic(&String::from_utf8_lossy(&output.stderr))))
}

pub(super) fn bounded_output(command:&mut std::process::Command) -> Result<std::process::Output,String> {
    bounded_output_with_limits(command,std::time::Duration::from_secs(10),std::time::Duration::from_secs(3))
}
fn bounded_output_with_limits(command:&mut std::process::Command,timeout:std::time::Duration,cleanup:std::time::Duration) -> Result<std::process::Output,String> {
    use std::process::Stdio;
    let mut child = command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|error|format!("Startup registration command is unavailable: {error}"))?;
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if child.try_wait().map_err(|error| error.to_string())?.is_some() { break; }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let deadline = std::time::Instant::now() + cleanup;
            while child.try_wait().map_err(|error|error.to_string())?.is_none() {
                if std::time::Instant::now() >= deadline { return Err("YOUGORI_OPERATION_INTERRUPTED: startup registration command did not confirm termination; inspect its OS registration before retrying".into()); }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            return Err("Startup registration command did not respond before its deadline; its owned process was stopped".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    child.wait_with_output().map_err(|error|error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registry_absence_is_distinct_from_access_failure_or_oversized_values() {
        assert_eq!(registry_query_outcome(0).unwrap(),true);
        for code in [2,3] {assert_eq!(registry_query_outcome(code).unwrap(),false);}
        for code in [5,234,1630] {assert!(registry_query_outcome(code).unwrap_err().contains("not assumed absent"));}
    }
    #[test]
    fn an_unresponsive_os_registration_command_is_bounded_and_only_its_child_is_stopped() {
        let root = tempfile::tempdir().unwrap();
        let pid_file = root.path().join("owned.pid");
        let mut command = std::process::Command::new(if cfg!(windows) {"python"} else {"python3"});
        command.args(["-c","import os,sys,time;open(sys.argv[1],'w').write(str(os.getpid()));time.sleep(60)"]).arg(&pid_file);
        #[cfg(windows)] {use std::os::windows::process::CommandExt; command.creation_flags(0x08000000);}
        let error = bounded_output_with_limits(&mut command,std::time::Duration::from_millis(350),std::time::Duration::from_secs(1)).unwrap_err();
        assert!(error.contains("owned process was stopped"),"{error}");
        let pid:u32 = std::fs::read_to_string(pid_file).unwrap().parse().unwrap();
        assert!(sysinfo::System::new_all().process(sysinfo::Pid::from_u32(pid)).is_none());
    }

    #[test]
    fn headless_sign_in_registers_a_user_service_without_starting_another_engine() {
        let temporary = tempfile::tempdir().unwrap();
        let mut commands = Vec::new();
        configure_linux_in(temporary.path(), std::path::Path::new("/home/user/Yougori $100%/engine"), true, true, |args| { commands.push(args.iter().map(|s| s.to_string()).collect::<Vec<_>>()); Ok("running".into()) }).unwrap();
        let unit = std::fs::read_to_string(temporary.path().join("systemd/user").join(UNIT)).unwrap();
        assert!(unit.contains("WantedBy=default.target"));
        assert!(unit.contains("$$100%%"));
        assert!(unit.contains("--headless --startup-sign-in"));
        assert!(!unit.contains("After=default.target"), "default.target also waits for this unit; do not create an ordering cycle");
        assert!(!temporary.path().join("autostart/yougori.desktop").exists());
        assert_eq!(commands, vec![vec!["is-system-running"], vec!["daemon-reload"], vec!["enable", UNIT]]);
    }

    #[test]
    fn unsupported_user_manager_and_failed_enable_leave_previous_startup_untouched() {
        let temporary = tempfile::tempdir().unwrap();
        let executable = std::path::Path::new("/opt/yougori/engine");
        configure_linux_in(temporary.path(), executable, true, false, |_| panic!("desktop startup does not need systemd")).unwrap();
        let desktop = temporary.path().join("autostart/yougori.desktop");
        let previous = std::fs::read(&desktop).unwrap();
        let unsupported = configure_linux_in(temporary.path(), executable, true, true, |_| Err("No user manager".into())).unwrap_err();
        assert!(unsupported.contains("YOUGORI_STARTUP_UNSUPPORTED"));
        assert_eq!(std::fs::read(&desktop).unwrap(), previous);
        let failed = configure_linux_in(temporary.path(), executable, true, true, |args| if args == ["enable", UNIT] { Err("Cannot enable unit".into()) } else { Ok("running".into()) }).unwrap_err();
        assert!(failed.contains("Cannot enable"));
        assert_eq!(std::fs::read(&desktop).unwrap(), previous);
        assert!(!temporary.path().join("systemd/user").join(UNIT).exists());
    }

    #[test]
    fn disabling_and_switching_to_desktop_only_remove_managed_registrations() {
        let temporary = tempfile::tempdir().unwrap();
        let executable = std::path::Path::new("/opt/yougori/engine");
        let unit = temporary.path().join("systemd/user").join(UNIT);
        configure_linux_in(temporary.path(), executable, true, true, |_| Ok("running".into())).unwrap();
        let mut commands = Vec::new();
        configure_linux_in(temporary.path(), executable, true, false, |args| { commands.push(args.iter().map(|s| s.to_string()).collect::<Vec<_>>()); Ok("running".into()) }).unwrap();
        assert_eq!(commands, vec![vec!["disable", UNIT], vec!["daemon-reload"]]);
        assert!(!unit.exists());
        assert!(temporary.path().join("autostart/yougori.desktop").exists());
        std::fs::write(&unit, "[Unit]\nDescription=Personal engine\n").unwrap();
        assert!(configure_linux_in(temporary.path(), executable, true, true, |_| panic!("unmanaged file must be rejected before manager calls")).is_err());
        assert!(std::fs::read_to_string(unit).unwrap().contains("Personal engine"));
    }

    #[test]
    fn registration_report_requires_a_user_manager_and_an_enabled_unit() {
        let unavailable = describe_linux(|_| Err("No manager".into()));
        assert_eq!(unavailable["status"], "unsupported");
        assert_eq!(unavailable["startsBeforeSignIn"], false);
        assert!(unavailable["recoveryAction"].as_str().unwrap().contains("sign-in"));
        let missing = describe_linux(|args| if args == ["is-system-running"] { Ok("running".into()) } else { Err("Unit missing".into()) });
        assert_eq!(missing["status"], "missing");
        let registered = describe_linux(|args| Ok(if args == ["is-system-running"] { "running" } else { "enabled" }.into()));
        assert_eq!(registered["status"], "registered");
        assert_eq!(registered["trigger"], "signIn");
    }
}
