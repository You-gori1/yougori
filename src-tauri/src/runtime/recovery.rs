use super::RuntimeManager;

impl RuntimeManager {
    pub async fn recover_orphaned_vm(&self, id: &str) -> Result<(), String> {
        if let Some(engine) = self.storage_runtime(id)? { return Box::pin(engine.recover_orphaned_vm(id)).await; }

        let lock = self.vm_lifecycle_mutex(id).await;
        let _guard = lock.lock().await;
        if self.vm_is_running(id).await? {
            return Err("This VM is owned by the current app. Use normal Shut down.".into());
        }
        self.check_external_vm(id, true).await
    }

    pub(super) async fn check_external_vm(&self, id: &str, recover: bool) -> Result<(), String> {
        super::vm::validate_runtime_identifier("virtual machine", id)?;
        #[cfg(target_os = "windows")]
        {
            let disk = self.data_root.join("environments").join(id).join("system.qcow2");
            let executables = vec![self.layout.qemu_system.clone(), self.layout.root.join("qemu-secure/qemu-system-x86_64.exe")];
            let name = format!("Yougori {id}");
            tokio::task::spawn_blocking(move || windows::check_named(&disk, &executables, recover, &name))
                .await.map_err(|error| format!("inspect VM runtime: {error}"))?
        }
        #[cfg(target_os = "linux")]
        { let _ = recover; linux_disk_idle(&self.data_root.join("environments").join(id).join("system.qcow2")) }
        #[cfg(target_os = "macos")]
        { let _ = recover; macos_disk_idle(&self.data_root.join("environments").join(id).join("system.qcow2")).await }
        #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
        { let _ = recover; Ok(()) }
    }
    /// Only explicitly confirmed recovery may terminate an abandoned runtime.
    /// A live runtime owned by this instance is left alone; normal deletion uses its agent.
    pub async fn recover_orphaned_container_runtime(&self) -> Result<(), String> {
        let mut owned = self.appliance.lock().await;
        if let Some(process) = owned.as_mut() {
            if process
                .child
                .try_wait()
                .map_err(|e| e.to_string())?
                .is_none()
            {
                return Ok(());
            }
        }
        self.check_external_appliance(true).await
    }

    pub(super) async fn check_external_appliance(&self, recover: bool) -> Result<(), String> {
        #[cfg(target_os = "windows")]
        {
            let disk = self.data_root.join("appliance/system.qcow2");
            let mut executables = vec![self.layout.qemu_system.clone()];
            if let Ok(exe) = std::env::current_exe() {
                if let Some(parent) = exe.parent() {
                    executables.push(parent.join("runtime/qemu/qemu-system-x86_64.exe"));
                }
            }
            tokio::task::spawn_blocking(move || windows::check(&disk, &executables, recover))
                .await
                .map_err(|error| format!("inspect container runtime: {error}"))?
        }
        #[cfg(target_os = "linux")]
        { let _ = recover; linux_disk_idle(&self.data_root.join("appliance/system.qcow2")) }
        #[cfg(target_os = "macos")]
        { let _ = recover; macos_disk_idle(&self.data_root.join("appliance/system.qcow2")).await }
        #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
        {
            let _ = recover;
            Ok(())
        }
    }
}

#[cfg(target_os = "macos")]
async fn macos_disk_idle(disk: &std::path::Path) -> Result<(), String> {
    match std::fs::symlink_metadata(disk) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("Inspect VM disk: {e}")),
        Ok(meta) if !meta.is_file() || meta.file_type().is_symlink() => return Err("Runtime disk is not a regular file; nothing was deleted.".into()),
        Ok(_) => {}
    }
    // macOS permits unlinking an open disk. Never interpret an unknown lock
    // owner as safe, and never kill an arbitrary QEMU just because of its name.
    let mut command = tokio::process::Command::new("/usr/sbin/lsof");
    command.args(["-nP", "-F", "p", "--"]).arg(disk);
    super::configure_background_process(&mut command);
    let output = tokio::time::timeout(std::time::Duration::from_secs(8), command.output())
        .await.map_err(|_| "Disk ownership check timed out; no disk was deleted or replaced")?
        .map_err(|e| format!("Cannot inspect disk ownership with macOS lsof: {e}"))?;
    macos_disk_check_result(output.status.code(), &output.stdout, &output.stderr)
}

#[cfg(any(target_os = "macos", test))]
fn macos_disk_check_result(code: Option<i32>, stdout: &[u8], stderr: &[u8]) -> Result<(), String> {
    if stdout.split(|b| *b == b'\n').any(|line| line.first() == Some(&b'p') && line[1..].iter().any(u8::is_ascii_digit)) {
        return Err("[YOUGORI_RUNTIME_BUSY] Another process still holds this disk. Close the other Yougori instance normally. If it crashed, use macOS Activity Monitor to quit only its QEMU process, then retry. This preview does not force-stop unverified processes; your disk was preserved.".into());
    }
    if code == Some(1) && stdout.is_empty() && stderr.is_empty() { return Ok(()); }
    Err("Could not verify that the runtime disk is unused. No disk was deleted or replaced. Close other Yougori instances and retry.".into())
}

#[cfg(test)]
mod macos_tests {
    use super::*;
    #[test]
    fn lsof_only_accepts_an_unambiguous_unused_disk() {
        assert!(macos_disk_check_result(Some(1), b"", b"").is_ok());
        assert!(macos_disk_check_result(Some(0), b"p123\n", b"").unwrap_err().contains("YOUGORI_RUNTIME_BUSY"));
        for code in [None, Some(0), Some(2)] {
            assert!(macos_disk_check_result(code, b"", b"").is_err());
        }
        assert!(macos_disk_check_result(Some(1), b"", b"permission denied").is_err());
        assert!(macos_disk_check_result(Some(1), b"unexpected", b"").is_err());
    }
}

#[cfg(target_os = "linux")]
fn linux_disk_idle(disk: &std::path::Path) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    let meta = match std::fs::metadata(disk) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("Inspect runtime disk: {e}")),
    };
    for process in std::fs::read_dir("/proc").map_err(|e| e.to_string())? {
        let process = process.map_err(|e| e.to_string())?;
        let Ok(pid) = process.file_name().to_string_lossy().parse::<u32>() else { continue };
        if pid == std::process::id() { continue; }
        let Ok(files) = std::fs::read_dir(process.path().join("fd")) else { continue };
        for fd in files.flatten() {
            if std::fs::metadata(fd.path()).is_ok_and(|m| m.dev() == meta.dev() && m.ino() == meta.ino()) {
                return Err("[YOUGORI_RUNTIME_BUSY] Another process still holds this environment's disk. Close the other Yougori instance normally, wait for its runtime to exit, then retry. No disk was deleted or replaced.".into());
            }
        }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
mod windows {
    use std::{
        ffi::OsString,
        path::{Path, PathBuf},
    };
    use sysinfo::{Pid, Process, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0},
        System::Threading::{
            OpenProcess, TerminateProcess, WaitForSingleObject, PROCESS_QUERY_LIMITED_INFORMATION,
            PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
        },
    };

    fn same_file(a: &Path, b: &Path) -> bool {
        match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
            (Ok(a), Ok(b)) => a
                .to_string_lossy()
                .eq_ignore_ascii_case(&b.to_string_lossy()),
            _ => false,
        }
    }

    #[cfg(test)]
    fn appliance_args(args: &[OsString], disk: &Path) -> bool {
        runtime_args(args, disk, "Yougori Internal OCI Runtime")
    }

    fn runtime_args(args: &[OsString], disk: &Path, name: &str) -> bool {
        let previous = name.strip_prefix("Yougori ").map(|suffix| format!("OpenDock {suffix}"));
        let microvm = name.strip_prefix("Yougori ").or_else(|| name.strip_prefix("OpenDock "))
            .filter(|_| name != "Yougori Internal OCI Runtime" && name != "OpenDock Internal OCI Runtime")
            .map(|id| [format!("Yougori microVM {id}"), format!("OpenDock microVM {id}")]);
        let named = args.windows(2).any(|pair| {
            pair[0] == "-name" && (pair[1] == name
                || previous.as_ref().is_some_and(|value| pair[1] == value.as_str())
                || microvm.as_ref().is_some_and(|names| names.iter().any(|value| pair[1] == value.as_str())))
        });
        named
            && args.windows(2).any(|pair| {
                if pair[0] != "-blockdev" {
                    return false;
                }
                let Ok(value) =
                    serde_json::from_str::<serde_json::Value>(&pair[1].to_string_lossy())
                else {
                    return false;
                };
                value["driver"] == "file"
                    && value["filename"]
                        .as_str()
                        .is_some_and(|file| same_file(Path::new(file), disk))
            })
    }

    fn matches(process: &Process, disk: &Path, executables: &[PathBuf], name: &str) -> bool {
        process
            .exe()
            .is_some_and(|exe| executables.iter().any(|allowed| same_file(exe, allowed)))
            && runtime_args(process.cmd(), disk, name)
    }

    fn refresh(system: &mut System) {
        system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing()
                .with_exe(UpdateKind::Always)
                .with_cmd(UpdateKind::Always),
        );
    }

    struct Handle(HANDLE);
    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    pub(super) fn check(disk: &Path, executables: &[PathBuf], recover: bool) -> Result<(), String> {
        check_named(disk, executables, recover, "Yougori Internal OCI Runtime")
    }

    // Use only a loopback QMP endpoint read from the verified process arguments.
    fn request_power(args: &[OsString], command: &str) -> Result<(), String> {
        use std::io::{BufRead, BufReader, Write};
        use std::net::{SocketAddr, TcpStream};
        use std::time::Duration;
        let address = args.windows(2).find(|p| p[0] == "-qmp")
            .and_then(|p| p[1].to_str()).and_then(|s| s.strip_prefix("tcp:127.0.0.1:"))
            .and_then(|s| s.split(',').next()).and_then(|s| s.parse::<u16>().ok())
            .ok_or("No verified QMP endpoint")?;
        let mut stream = TcpStream::connect_timeout(&SocketAddr::from(([127,0,0,1],address)), Duration::from_secs(2)).map_err(|e| e.to_string())?;
        stream.set_read_timeout(Some(Duration::from_secs(2))).map_err(|e| e.to_string())?;
        stream.set_write_timeout(Some(Duration::from_secs(2))).map_err(|e| e.to_string())?;
        let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
        let mut line = String::new();
        reader.read_line(&mut line).map_err(|e| e.to_string())?;
        for (id, command) in [("hello", "qmp_capabilities"), ("power", command)] {
            writeln!(stream, "{}", serde_json::json!({"execute":command,"id":id})).map_err(|e| e.to_string())?;
            let mut matched = false;
            for _ in 0..32 {
                line.clear();
                if reader.read_line(&mut line).map_err(|e| e.to_string())? == 0 { break; }
                let reply: serde_json::Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
                if reply["id"] == id {
                    if reply.get("error").is_some() { return Err("QMP power request failed".into()); }
                    matched = true;
                    break;
                }
            }
            if !matched { return Err("QMP did not acknowledge the power request".into()); }
        }
        Ok(())
    }

    /// True when no live process owns this runtime, so stopping it cannot
    /// interrupt another running Yougori instance.
    fn abandoned(system: &mut System, pid: Pid) -> bool {
        refresh(system);
        match system.process(pid).and_then(|process| process.parent()) {
            Some(parent) => system.process(parent).is_none(),
            None => false,
        }
    }

    pub(super) fn check_named(disk: &Path, executables: &[PathBuf], recover: bool, name: &str) -> Result<(), String> {
        let mut system = System::new();
        refresh(&mut system);
        let candidates: Vec<Pid> = system
            .processes()
            .iter()
            .filter(|(_, process)| matches(process, disk, executables, name))
            .map(|(pid, _)| *pid)
            .collect();
        for pid in candidates {
            if !recover {
                // A runtime whose owning Yougori is gone is nobody's: reclaim it
                // here instead of asking the user to recover it by hand. Only a
                // runtime still owned by a live instance is reported as busy.
                if abandoned(&mut system, pid) {
                    eprintln!("Reclaiming an abandoned {name} runtime left by a previous Yougori instance.");
                } else if name != "Yougori Internal OCI Runtime" {
                    return Err("[YOUGORI_VM_RUNTIME_BUSY] This VM is still running in a previous Yougori runtime. Press Stop (Shut down) on this VM to recover it without deleting its disk. Close any other Yougori instance normally first.".into());
                } else {
                    return Err("[YOUGORI_RUNTIME_BUSY] An older Yougori runtime still holds the container disk. Press Stop (Shut down) on this environment and confirm stopping the abandoned runtime, then retry Start. This does not delete your containers. Close any other Yougori instance normally first.".into());
                }
            }
            // Pin the process before rechecking its identity. Never use taskkill by PID:
            // a recycled PID must not cause an unrelated process to be terminated.
            let handle = Handle(unsafe {
                OpenProcess(
                    PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                    0,
                    pid.as_u32(),
                )
            });
            if handle.0.is_null() {
                return Err("Cannot access the old container runtime. Close its owning Yougori instance, then retry recovery.".into());
            }
            if unsafe { WaitForSingleObject(handle.0, 0) } == WAIT_OBJECT_0 {
                continue;
            }
            refresh(&mut system);
            let Some(process) = system.process(pid) else {
                continue;
            };
            if !matches(process, disk, executables, name) {
                return Err("Runtime identity changed during recovery; no process was stopped. Retry recovery.".into());
            }
            let Some(parent) = process.parent() else {
                return Err(
                    "Cannot verify the old runtime's owner; no process was stopped.".into(),
                );
            };
            if system.process(parent).is_some() {
                return Err("Another live process owns this runtime. Close its Yougori instance normally before recovery; recovery will not stop it.".into());
            }
            let shutdown_wait_ms = if name == "Yougori Internal OCI Runtime" { 10000 } else { 30000 };
            if request_power(process.cmd(), "system_powerdown").is_ok()
                && unsafe { WaitForSingleObject(handle.0, shutdown_wait_ms) } == WAIT_OBJECT_0 { continue; }
            // Even when the guest cannot respond, QMP quit lets QEMU close its
            // QCOW2 metadata before the last-resort process termination.
            if request_power(process.cmd(), "quit").is_ok()
                && unsafe { WaitForSingleObject(handle.0, 5000) } == WAIT_OBJECT_0 { continue; }
            if unsafe { TerminateProcess(handle.0, 1) } == 0 {
                return Err(
                    "Windows could not stop the orphaned runtime. No environment data was removed."
                        .into(),
                );
            }
            if unsafe { WaitForSingleObject(handle.0, 5000) } != WAIT_OBJECT_0 {
                return Err("The orphaned runtime is still shutting down. Wait a moment, then retry recovery.".into());
            }
        }
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn fixture_command(exe: &Path, disk: &Path) -> std::process::Command {
            fixture_command_named(exe, disk, "OpenDock Internal OCI Runtime")
        }

        fn fixture_command_named(exe: &Path, disk: &Path, name: &str) -> std::process::Command {
            use std::os::windows::process::CommandExt;
            let mut command = std::process::Command::new(exe);
            command
                .creation_flags(0x0800_0000)
                .args([
                    "-machine",
                    "none",
                    "-m",
                    "32M",
                    "-S",
                    "-nodefaults",
                    "-no-user-config",
                    "-display",
                    "none",
                    "-monitor",
                    "none",
                    "-serial",
                    "none",
                    "-name",
                    name,
                    "-blockdev",
                ])
                .arg(
                    serde_json::json!({"driver":"file", "filename":disk,"node-name":"test-disk"})
                        .to_string(),
                )
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
            command
        }

        #[test]
        #[ignore = "internal subprocess helper for the isolated orphan recovery test"]
        fn recovery_fixture_parent() {
            let Some(directory) = std::env::var_os("OPENDOCK_RECOVERY_FIXTURE_DIR") else {
                return;
            };
            let directory = PathBuf::from(directory);
            let exe = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("resources/runtime/qemu/qemu-system-x86_64.exe");
            let name = std::env::var("OPENDOCK_RECOVERY_FIXTURE_NAME").unwrap_or_else(|_| "OpenDock Internal OCI Runtime".into());
            let child = fixture_command_named(&exe, &directory.join("system.qcow2"), &name)
                .spawn()
                .unwrap();
            std::fs::write(directory.join("pid"), child.id().to_string()).unwrap();
            // std::process::Child deliberately survives this short-lived parent.
        }

        #[test]
        #[ignore = "launches isolated disk-only QEMU processes to verify orphan recovery safety"]
        fn recovery_stops_only_an_orphan_with_the_exact_disk() {
            use std::os::windows::process::CommandExt;
            let temp = tempfile::tempdir().unwrap();
            let disk = temp.path().join("system.qcow2");
            std::fs::write(&disk, vec![0u8; 4096]).unwrap();
            let exe = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("resources/runtime/qemu/qemu-system-x86_64.exe");
            let allowed = vec![exe.clone()];
            let mut owned = fixture_command(&exe, &disk).spawn().unwrap();
            // Check both the startup guard and refusal to kill a live owner's runtime.
            let busy = check(&disk, &allowed, false);
            let refusal = check(&disk, &allowed, true);
            let still_running = owned.try_wait().unwrap().is_none();
            let _ = owned.kill();
            let _ = owned.wait();
            assert!(busy.unwrap_err().contains("YOUGORI_RUNTIME_BUSY"));
            assert!(refusal.unwrap_err().contains("Another live process"));
            assert!(still_running);

            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .creation_flags(0x0800_0000)
                .args([
                    "--exact",
                    "runtime::recovery::windows::tests::recovery_fixture_parent",
                    "--ignored",
                ])
                .env("OPENDOCK_RECOVERY_FIXTURE_DIR", temp.path())
                .status()
                .unwrap();
            assert!(status.success());
            let pid: u32 = std::fs::read_to_string(temp.path().join("pid"))
                .unwrap()
                .parse()
                .unwrap();
            let handle =
                Handle(unsafe { OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, 0, pid) });
            assert!(!handle.0.is_null());
            // Always clean up this test-owned process, including when an assertion fails.
            struct FixtureGuard(Handle);
            impl Drop for FixtureGuard {
                fn drop(&mut self) {
                    unsafe {
                        TerminateProcess(self.0 .0, 1);
                        WaitForSingleObject(self.0 .0, 5000);
                    }
                }
            }
            let guard = FixtureGuard(handle);
            let other_disk = temp.path().join("unrelated.qcow2");
            std::fs::write(&other_disk, b"keep me").unwrap();
            check(&other_disk, &allowed, true).unwrap();
            assert_ne!(unsafe { WaitForSingleObject(guard.0 .0, 0) }, WAIT_OBJECT_0);
            // An abandoned runtime is reclaimed by an ordinary start, without
            // asking the user to recover it first.
            check(&disk, &allowed, false).unwrap();
            assert_eq!(unsafe { WaitForSingleObject(guard.0 .0, 0) }, WAIT_OBJECT_0);
            check(&disk, &allowed, true).unwrap(); // Repeat recovery is harmless.
            assert_eq!(std::fs::read(&disk).unwrap(), vec![0u8; 4096]);
            assert_eq!(std::fs::read(&other_disk).unwrap(), b"keep me");
        }

        #[test]
        #[ignore = "launches isolated disk-only QEMU processes to verify orphan recovery safety"]
        fn vm_recovery_stops_only_an_orphan_with_the_exact_disk() {
            use std::os::windows::process::CommandExt;
            let temp = tempfile::tempdir().unwrap();
            let disk = temp.path().join("system.qcow2");
            std::fs::write(&disk, vec![0u8; 4096]).unwrap();
            let exe = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("resources/runtime/qemu/qemu-system-x86_64.exe");
            let allowed = vec![exe.clone()];
            let name = "OpenDock env-test-vm";
            let mut owned = fixture_command_named(&exe, &disk, name).spawn().unwrap();
            // Check both the startup guard and refusal to kill a live owner's runtime.
            let busy = check_named(&disk, &allowed, false, name);
            let refusal = check_named(&disk, &allowed, true, name);
            let still_running = owned.try_wait().unwrap().is_none();
            let _ = owned.kill();
            let _ = owned.wait();
            assert!(busy.unwrap_err().contains("YOUGORI_VM_RUNTIME_BUSY"));
            assert!(refusal.unwrap_err().contains("Another live process"));
            assert!(still_running);

            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .creation_flags(0x0800_0000)
                .args([
                    "--exact",
                    "runtime::recovery::windows::tests::recovery_fixture_parent",
                    "--ignored",
                ])
                .env("OPENDOCK_RECOVERY_FIXTURE_DIR", temp.path())
                .env("OPENDOCK_RECOVERY_FIXTURE_NAME", name)
                .status()
                .unwrap();
            assert!(status.success());
            let pid: u32 = std::fs::read_to_string(temp.path().join("pid"))
                .unwrap()
                .parse()
                .unwrap();
            let handle =
                Handle(unsafe { OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, 0, pid) });
            assert!(!handle.0.is_null());
            // Always clean up this test-owned process, including when an assertion fails.
            struct FixtureGuard(Handle);
            impl Drop for FixtureGuard {
                fn drop(&mut self) {
                    unsafe {
                        TerminateProcess(self.0 .0, 1);
                        WaitForSingleObject(self.0 .0, 5000);
                    }
                }
            }
            let guard = FixtureGuard(handle);
            let other_disk = temp.path().join("unrelated.qcow2");
            std::fs::write(&other_disk, b"keep me").unwrap();
            check_named(&other_disk, &allowed, true, name).unwrap();
            assert_ne!(unsafe { WaitForSingleObject(guard.0 .0, 0) }, WAIT_OBJECT_0);
            check_named(&disk, &allowed, true, "OpenDock env-unrelated").unwrap();
            assert_ne!(unsafe { WaitForSingleObject(guard.0 .0, 0) }, WAIT_OBJECT_0);
            // An abandoned runtime is reclaimed by an ordinary start, without
            // asking the user to recover it first.
            check_named(&disk, &allowed, false, name).unwrap();
            assert_eq!(unsafe { WaitForSingleObject(guard.0 .0, 0) }, WAIT_OBJECT_0);
            check_named(&disk, &allowed, true, name).unwrap(); // Repeat recovery is harmless.
            assert_eq!(std::fs::read(&disk).unwrap(), vec![0u8; 4096]);
            assert_eq!(std::fs::read(&other_disk).unwrap(), b"keep me");
        }

        #[test]
        fn recovery_requires_exact_appliance_name_and_managed_disk() {
            let temp = tempfile::tempdir().unwrap();
            let disk = temp.path().join("system.qcow2");
            std::fs::write(&disk, b"fixture").unwrap();
            let block = serde_json::json!({"driver":"file", "filename":disk}).to_string();
            let mut args: Vec<OsString> = [
                "qemu",
                "-name",
                "OpenDock Internal OCI Runtime",
                "-blockdev",
                &block,
            ]
            .into_iter()
            .map(Into::into)
            .collect();
            assert!(appliance_args(&args, &disk));
            assert!(!appliance_args(&args, &temp.path().join("other.qcow2")));
            args[2] = "User VM".into();
            assert!(!appliance_args(&args, &disk));
            args[2] = "OpenDock Internal OCI Runtime".into();
            args[4] = "not JSON".into();
            assert!(!appliance_args(&args, &disk));
        }

        #[test]
        fn recovery_recognizes_only_the_target_vm_or_microvm() {
            let temp = tempfile::tempdir().unwrap();
            let disk = temp.path().join("system.qcow2");
            std::fs::write(&disk, b"fixture").unwrap();
            let block = serde_json::json!({"driver":"file", "filename":disk}).to_string();
            for name in ["OpenDock env-target", "OpenDock microVM env-target"] {
                let args: Vec<OsString> = ["qemu", "-name", name, "-blockdev", &block].into_iter().map(Into::into).collect();
                assert!(runtime_args(&args, &disk, "OpenDock env-target"));
                assert!(!runtime_args(&args, &disk, "OpenDock env-other"));
                assert!(!runtime_args(&args, &temp.path().join("other.qcow2"), "OpenDock env-target"));
                assert!(!appliance_args(&args, &disk));
            }
        }
    }
}
