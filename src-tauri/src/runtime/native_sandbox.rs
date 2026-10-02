use std::{
    fs,
    path::{Component, Path, PathBuf},
};
#[cfg(target_os = "windows")]
use std::process::Command;

use sysinfo::{Pid, ProcessesToUpdate};

use super::{NativeSandboxProcess, RuntimeManager};
use crate::models::{ResourcePolicy, SandboxFileAccess, SandboxPolicy, SandboxShare};

#[derive(Debug, Clone)]
pub struct SandboxProvisionResult {
    pub workspace_path: PathBuf,
    pub policy: SandboxPolicy,
}

#[derive(Debug, Clone, Default)]
pub struct SandboxStats {
    pub cpu_percent: f64,
    pub memory_bytes: u64,
}

fn validate_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || id.len() > 80
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err("invalid native sandbox identifier".into());
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn profile_name(id: &str) -> Result<String, String> {
    validate_id(id)?;
    Ok(format!("Yougori.{id}"))
}

fn canonical_directory(path: &Path, label: &str) -> Result<PathBuf, String> {
    if !path.is_dir() {
        return Err(format!(
            "{label} is not an existing folder: {}",
            path.display()
        ));
    }
    path.canonicalize()
        .map_err(|error| format!("resolve {label} {}: {error}", path.display()))
}

fn is_volume_root(path: &Path) -> bool {
    if path == Path::new("/") { return true; }
    let mut components = path.components();
    matches!(components.next(), Some(Component::Prefix(_)))
        && matches!(components.next(), Some(Component::RootDir))
        && components.next().is_none()
}

fn normalize_policy(policy: &SandboxPolicy, managed_root: &Path) -> Result<SandboxPolicy, String> {
    let executable = PathBuf::from(policy.executable.trim());
    if !executable.is_file() {
        return Err(format!(
            "Select an existing Windows application (.exe): {}",
            executable.display()
        ));
    }
    if !executable
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
    {
        return Err("Native application branches can launch .exe files only".into());
    }
    let executable = executable
        .canonicalize()
        .map_err(|error| format!("resolve application path: {error}"))?;
    let managed_root = managed_root
        .canonicalize()
        .unwrap_or_else(|_| managed_root.to_path_buf());

    let mut shares: Vec<SandboxShare> = Vec::new();
    for share in &policy.shares {
        let path = canonical_directory(Path::new(share.path.trim()), "shared path")?;
        if is_volume_root(&path) {
            return Err(format!(
                "A whole drive cannot be shared with a branch: {}",
                path.display()
            ));
        }
        if path.starts_with(&managed_root) || managed_root.starts_with(&path) {
            return Err("Yougori's managed data directory cannot be shared with a branch".into());
        }
        if let Some(existing) = shares
            .iter_mut()
            .find(|existing| Path::new(&existing.path) == path)
        {
            if share.access == SandboxFileAccess::ReadWrite {
                existing.access = SandboxFileAccess::ReadWrite;
            }
            continue;
        }
        shares.push(SandboxShare {
            path: path.to_string_lossy().into_owned(),
            access: share.access.clone(),
        });
    }
    shares.sort_by(|left, right| left.path.cmp(&right.path));

    Ok(SandboxPolicy {
        executable: executable.to_string_lossy().into_owned(),
        arguments: policy.arguments.trim().to_owned(),
        shares,
        network_access: policy.network_access,
    })
}

impl RuntimeManager {
    fn sandbox_root(&self, id: &str) -> Result<PathBuf, String> {
        validate_id(id)?;
        Ok(self.data_root.join("environments").join(id))
    }

    pub async fn provision_native_sandbox(
        &self,
        id: &str,
        policy: &SandboxPolicy,
    ) -> Result<SandboxProvisionResult, String> {
        let root = self.sandbox_root(id)?;
        let workspace = root.join("workspace");
        fs::create_dir_all(workspace.join("Temp"))
            .map_err(|error| format!("create native branch workspace: {error}"))?;
        let normalized = normalize_policy(policy, &self.data_root)?;
        platform::prepare_profile_and_access(id, &workspace, &normalized).await?;
        Ok(SandboxProvisionResult {
            workspace_path: workspace,
            policy: normalized,
        })
    }

    pub async fn start_native_sandbox(
        &self,
        id: &str,
        workspace: &Path,
        policy: &SandboxPolicy,
        resources: &ResourcePolicy,
    ) -> Result<(), String> {
        if self.native_sandbox_is_running(id).await? {
            return Ok(());
        }
        let expected_workspace = self.sandbox_root(id)?.join("workspace");
        if workspace != expected_workspace {
            return Err("native branch workspace metadata is invalid".into());
        }
        let normalized = normalize_policy(policy, &self.data_root)?;
        platform::prepare_profile_and_access(id, workspace, &normalized).await?;
        let process = platform::launch(id, workspace, &normalized, resources)?;
        self.sandboxes.lock().await.insert(id.to_owned(), process);
        Ok(())
    }

    pub async fn stop_native_sandbox(&self, id: &str) -> Result<(), String> {
        let process = self.sandboxes.lock().await.remove(id);
        if let Some(mut process) = process {
            platform::stop(&mut process)?;
        }
        Ok(())
    }

    pub async fn native_sandbox_is_running(&self, id: &str) -> Result<bool, String> {
        let mut processes = self.sandboxes.lock().await;
        let Some(process) = processes.get(id) else {
            return Ok(false);
        };
        if platform::is_running(process)? {
            return Ok(true);
        }
        if let Some(mut process) = processes.remove(id) {
            platform::close(&mut process);
        }
        Ok(false)
    }

    pub async fn update_native_sandbox_resources(
        &self,
        id: &str,
        cpu_cores: f64,
        memory_gb: f64,
    ) -> Result<(), String> {
        let processes = self.sandboxes.lock().await;
        let process = processes
            .get(id)
            .ok_or_else(|| "native application branch is not running".to_string())?;
        platform::set_job_limits(process, cpu_cores, memory_gb)
    }

    pub async fn native_sandbox_process_stats(&self, id: &str) -> Result<SandboxStats, String> {
        let process_id = self
            .sandboxes
            .lock()
            .await
            .get(id)
            .map(|process| process.process_id)
            .ok_or_else(|| "native application branch is not running".to_string())?;
        let pid = Pid::from_u32(process_id);
        let mut system = self.process_metrics.lock().await;
        system.refresh_processes(ProcessesToUpdate::Some(&[pid]), false);
        let process = system
            .process(pid)
            .ok_or_else(|| "native application process metrics are unavailable".to_string())?;
        Ok(SandboxStats {
            cpu_percent: f64::from(process.cpu_usage()),
            memory_bytes: process.memory(),
        })
    }

    pub async fn delete_native_sandbox(
        &self,
        id: &str,
        policy: Option<&SandboxPolicy>,
    ) -> Result<(), String> {
        self.stop_native_sandbox(id).await?;
        platform::remove_profile_and_access(id, policy).await?;
        let root = self.sandbox_root(id)?;
        if root.exists() {
            fs::remove_dir_all(&root).map_err(|error| {
                format!("delete native branch data {}: {error}", root.display())
            })?;
        }
        Ok(())
    }

    pub(super) async fn shutdown_all_native_sandboxes_report(&self) -> Vec<serde_json::Value> {
        let processes: Vec<_> = self.sandboxes.lock().await.drain().collect();
        futures_util::future::join_all(processes.into_iter().map(|(id, mut process)| async move {
            let stopped = tokio::task::spawn_blocking(move || platform::stop(&mut process)).await.map_err(|error| error.to_string()).and_then(|result| result);
            serde_json::json!({"provider":"nativeSandbox","environmentId":id,"status":if stopped.is_ok(){"stopped"}else{"failed"},"scope":"ownedRuntimes","postconditionVerified":stopped.is_ok(),"ownershipReleased":stopped.is_ok(),"error":stopped.err().map(|error|crate::lifecycle::safe_diagnostic(&error))})
        })).await
    }
}

#[cfg(target_os = "windows")]
pub(super) fn terminate_native_process(process: &mut NativeSandboxProcess) {
    let _ = platform::stop(process);
}

#[cfg(not(target_os = "windows"))]
pub(super) fn terminate_native_process(_process: &mut NativeSandboxProcess) {}

#[cfg(target_os = "windows")]
mod platform {
    use std::{
        alloc::{alloc_zeroed, dealloc, Layout},
        ffi::OsStr,
        mem::{align_of, size_of},
        os::windows::ffi::OsStrExt,
        path::Path,
        process::Stdio,
        ptr::{null, null_mut},
    };

    use windows_sys::Win32::{
        Foundation::{
            CloseHandle, GetLastError, LocalFree, ERROR_ALREADY_EXISTS, HANDLE, STILL_ACTIVE,
        },
        Security::{
            Authorization::ConvertSidToStringSidW,
            DeriveCapabilitySidsFromName,
            Isolation::{
                CreateAppContainerProfile, DeleteAppContainerProfile,
                DeriveAppContainerSidFromAppContainerName,
            },
            PSID, SECURITY_CAPABILITIES, SID_AND_ATTRIBUTES,
        },
        System::{
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JobObjectCpuRateControlInformation,
                JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
                JOBOBJECT_CPU_RATE_CONTROL_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                JOB_OBJECT_CPU_RATE_CONTROL_ENABLE, JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP,
                JOB_OBJECT_LIMIT_JOB_MEMORY, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            },
            Threading::{
                CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess,
                InitializeProcThreadAttributeList, ResumeThread, UpdateProcThreadAttribute,
                WaitForSingleObject, CREATE_SUSPENDED, EXTENDED_STARTUPINFO_PRESENT,
                PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES, STARTUPINFOEXW,
            },
        },
    };

    use super::{
        profile_name, Command, NativeSandboxProcess, ResourcePolicy, SandboxFileAccess,
        SandboxPolicy,
    };

    const HRESULT_ALREADY_EXISTS: i32 = 0x8007_00B7_u32 as i32;
    const WAIT_TIMEOUT: u32 = 258;
    const SE_GROUP_ENABLED: u32 = 4;

    fn wide(value: &OsStr) -> Vec<u16> {
        value.encode_wide().chain(Some(0)).collect()
    }

    fn windows_error(operation: &str) -> String {
        let code = unsafe { GetLastError() };
        format!("{operation} failed with Windows error {code}")
    }

    unsafe fn profile_sid(id: &str) -> Result<PSID, String> {
        let name = profile_name(id)?;
        let name_wide = wide(OsStr::new(&name));
        let display = wide(OsStr::new(&format!("Yougori branch {id}")));
        let description = wide(OsStr::new(
            "Isolated native application branch managed by Yougori",
        ));
        let mut sid: PSID = null_mut();
        let result = unsafe {
            CreateAppContainerProfile(
                name_wide.as_ptr(),
                display.as_ptr(),
                description.as_ptr(),
                null(),
                0,
                &mut sid,
            )
        };
        if result == HRESULT_ALREADY_EXISTS || result == ERROR_ALREADY_EXISTS as i32 {
            let result =
                unsafe { DeriveAppContainerSidFromAppContainerName(name_wide.as_ptr(), &mut sid) };
            if result < 0 {
                return Err(format!(
                    "open native sandbox profile failed with HRESULT 0x{result:08x}"
                ));
            }
        } else if result < 0 {
            return Err(format!(
                "create native sandbox profile failed with HRESULT 0x{result:08x}"
            ));
        }
        if sid.is_null() {
            return Err("Windows returned an empty AppContainer identity".into());
        }
        Ok(sid)
    }

    unsafe fn sid_string(sid: PSID) -> Result<String, String> {
        let mut pointer = null_mut();
        if unsafe { ConvertSidToStringSidW(sid, &mut pointer) } == 0 {
            return Err(windows_error("convert AppContainer identity"));
        }
        let mut length = 0;
        while unsafe { *pointer.add(length) } != 0 {
            length += 1;
        }
        let value =
            String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(pointer, length) });
        unsafe { LocalFree(pointer.cast()) };
        Ok(value)
    }

    fn run_icacls(path: &Path, sid: &str, permission: &str, remove: bool) -> Result<(), String> {
        let mut command = Command::new("icacls.exe");
        command.arg(path);
        if remove {
            command.args(["/remove:g", &format!("*{sid}")]);
        } else {
            command.args(["/grant:r", &format!("*{sid}:{permission}")]);
        }
        // An inheritable ACE on the selected directory covers its normal
        // descendants without rewriting every file ACL. This keeps setup fast
        // and avoids touching an entire application tree or shared workspace.
        command.args(["/C", "/Q"]);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let output = command
            .output()
            .map_err(|error| format!("configure branch access for {}: {error}", path.display()))?;
        if !output.status.success() {
            let details = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            return Err(format!(
                "Windows could not configure branch access for {}{}",
                path.display(),
                if details.is_empty() {
                    String::new()
                } else {
                    format!(": {details}")
                }
            ));
        }
        Ok(())
    }

    pub async fn prepare_profile_and_access(
        id: &str,
        workspace: &Path,
        policy: &SandboxPolicy,
    ) -> Result<(), String> {
        let sid = unsafe { profile_sid(id)? };
        let identity = unsafe { sid_string(sid) };
        unsafe { LocalFree(sid.cast()) };
        let identity = identity?;

        run_icacls(workspace, &identity, "(OI)(CI)(M)", false)?;
        let executable = Path::new(&policy.executable);
        let application_directory = executable
            .parent()
            .ok_or("the selected application has no parent folder")?;
        run_icacls(application_directory, &identity, "(OI)(CI)(RX)", false)?;
        for share in &policy.shares {
            let permission = match share.access {
                SandboxFileAccess::ReadOnly => "(OI)(CI)(RX)",
                SandboxFileAccess::ReadWrite => "(OI)(CI)(M)",
            };
            run_icacls(Path::new(&share.path), &identity, permission, false)?;
        }
        Ok(())
    }

    pub async fn remove_profile_and_access(
        id: &str,
        policy: Option<&SandboxPolicy>,
    ) -> Result<(), String> {
        let sid = unsafe { profile_sid(id)? };
        let identity = unsafe { sid_string(sid) };
        unsafe { LocalFree(sid.cast()) };
        let identity = identity?;
        let mut errors = Vec::new();
        if let Some(policy) = policy {
            if let Some(parent) = Path::new(&policy.executable).parent() {
                if parent.exists() {
                    if let Err(error) = run_icacls(parent, &identity, "", true) {
                        errors.push(error);
                    }
                }
            }
            for share in &policy.shares {
                let path = Path::new(&share.path);
                if path.exists() {
                    if let Err(error) = run_icacls(path, &identity, "", true) {
                        errors.push(error);
                    }
                }
            }
        }
        let name = wide(OsStr::new(&profile_name(id)?));
        let result = unsafe { DeleteAppContainerProfile(name.as_ptr()) };
        if result < 0 {
            errors.push(format!(
                "delete native sandbox profile failed with HRESULT 0x{result:08x}"
            ));
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }

    struct CapabilitySid {
        sid: PSID,
    }

    impl Drop for CapabilitySid {
        fn drop(&mut self) {
            unsafe { LocalFree(self.sid.cast()) };
        }
    }

    fn internet_capability() -> Result<CapabilitySid, String> {
        let name = wide(OsStr::new("internetClient"));
        let mut group_sids: *mut PSID = null_mut();
        let mut group_count = 0_u32;
        let mut capability_sids: *mut PSID = null_mut();
        let mut capability_count = 0_u32;
        if unsafe {
            DeriveCapabilitySidsFromName(
                name.as_ptr(),
                &mut group_sids,
                &mut group_count,
                &mut capability_sids,
                &mut capability_count,
            )
        } == 0
        {
            return Err(windows_error("derive Internet capability"));
        }
        unsafe {
            for index in 0..group_count as usize {
                LocalFree((*group_sids.add(index)).cast());
            }
            LocalFree(group_sids.cast());
        }
        if capability_count == 0 {
            unsafe { LocalFree(capability_sids.cast()) };
            return Err("Windows returned no Internet capability identity".into());
        }
        let sid = unsafe { *capability_sids };
        unsafe {
            for index in 1..capability_count as usize {
                LocalFree((*capability_sids.add(index)).cast());
            }
            LocalFree(capability_sids.cast());
        }
        Ok(CapabilitySid { sid })
    }

    fn quote_argument(value: &str) -> String {
        if !value.is_empty()
            && !value
                .chars()
                .any(|character| character.is_whitespace() || character == '"')
        {
            return value.to_owned();
        }
        let mut result = String::from("\"");
        let mut backslashes = 0;
        for character in value.chars() {
            if character == '\\' {
                backslashes += 1;
            } else if character == '"' {
                result.push_str(&"\\".repeat(backslashes * 2 + 1));
                result.push('"');
                backslashes = 0;
            } else {
                result.push_str(&"\\".repeat(backslashes));
                backslashes = 0;
                result.push(character);
            }
        }
        result.push_str(&"\\".repeat(backslashes * 2));
        result.push('"');
        result
    }

    pub fn launch(
        id: &str,
        workspace: &Path,
        policy: &SandboxPolicy,
        resources: &ResourcePolicy,
    ) -> Result<NativeSandboxProcess, String> {
        unsafe {
            let package_sid = profile_sid(id)?;
            let internet = if policy.network_access {
                Some(internet_capability()?)
            } else {
                None
            };
            let mut capability_attributes =
                internet.as_ref().map(|capability| SID_AND_ATTRIBUTES {
                    Sid: capability.sid,
                    Attributes: SE_GROUP_ENABLED,
                });
            let capabilities = SECURITY_CAPABILITIES {
                AppContainerSid: package_sid,
                Capabilities: capability_attributes
                    .as_mut()
                    .map_or(null_mut(), |value| value as *mut SID_AND_ATTRIBUTES),
                CapabilityCount: u32::from(capability_attributes.is_some()),
                Reserved: 0,
            };

            let mut attribute_size = 0_usize;
            InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut attribute_size);
            if attribute_size == 0 {
                LocalFree(package_sid.cast());
                return Err(windows_error("size native sandbox process attributes"));
            }
            let layout = Layout::from_size_align(attribute_size, align_of::<usize>())
                .map_err(|_| "invalid native sandbox attribute allocation".to_string())?;
            let attribute_list: *mut core::ffi::c_void = alloc_zeroed(layout).cast();
            if attribute_list.is_null() {
                LocalFree(package_sid.cast());
                return Err("allocate native sandbox process attributes".into());
            }
            let cleanup_attributes = || {
                DeleteProcThreadAttributeList(attribute_list);
                dealloc(attribute_list.cast(), layout);
            };
            if InitializeProcThreadAttributeList(attribute_list, 1, 0, &mut attribute_size) == 0 {
                dealloc(attribute_list.cast(), layout);
                LocalFree(package_sid.cast());
                return Err(windows_error(
                    "initialize native sandbox process attributes",
                ));
            }
            if UpdateProcThreadAttribute(
                attribute_list,
                0,
                PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as usize,
                (&capabilities as *const SECURITY_CAPABILITIES).cast(),
                size_of::<SECURITY_CAPABILITIES>(),
                null_mut(),
                null(),
            ) == 0
            {
                cleanup_attributes();
                LocalFree(package_sid.cast());
                return Err(windows_error("set native sandbox security identity"));
            }

            let job = CreateJobObjectW(null(), null());
            if job.is_null() {
                cleanup_attributes();
                LocalFree(package_sid.cast());
                return Err(windows_error("create native sandbox process group"));
            }
            let placeholder = NativeSandboxProcess {
                process_handle: 0,
                job_handle: job as usize,
                process_id: 0,
            };
            if let Err(error) = set_job_limits(
                &placeholder,
                resources.cpu.preferred,
                resources.memory_gb.preferred,
            ) {
                CloseHandle(job);
                cleanup_attributes();
                LocalFree(package_sid.cast());
                return Err(error);
            }

            let executable = wide(Path::new(&policy.executable).as_os_str());
            let mut command_line = quote_argument(&policy.executable);
            if !policy.arguments.is_empty() {
                command_line.push(' ');
                command_line.push_str(&policy.arguments);
            }
            let mut command_line = wide(OsStr::new(&command_line));
            let current_directory = wide(workspace.as_os_str());
            let mut startup = STARTUPINFOEXW::default();
            startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
            startup.lpAttributeList = attribute_list;
            let mut process_info = PROCESS_INFORMATION::default();
            let created = CreateProcessW(
                executable.as_ptr(),
                command_line.as_mut_ptr(),
                null(),
                null(),
                0,
                CREATE_SUSPENDED | EXTENDED_STARTUPINFO_PRESENT,
                null(),
                current_directory.as_ptr(),
                &startup.StartupInfo,
                &mut process_info,
            );
            cleanup_attributes();
            LocalFree(package_sid.cast());
            if created == 0 {
                CloseHandle(job);
                return Err(windows_error("launch application in native sandbox"));
            }
            if AssignProcessToJobObject(job, process_info.hProcess) == 0 {
                TerminateJobObject(job, 1);
                CloseHandle(process_info.hThread);
                CloseHandle(process_info.hProcess);
                CloseHandle(job);
                return Err(windows_error("contain native sandbox process tree"));
            }
            if ResumeThread(process_info.hThread) == u32::MAX {
                TerminateJobObject(job, 1);
                CloseHandle(process_info.hThread);
                CloseHandle(process_info.hProcess);
                CloseHandle(job);
                return Err(windows_error("start native sandbox application"));
            }
            CloseHandle(process_info.hThread);
            Ok(NativeSandboxProcess {
                process_handle: process_info.hProcess as usize,
                job_handle: job as usize,
                process_id: process_info.dwProcessId,
            })
        }
    }

    #[allow(clippy::field_reassign_with_default)]
    pub fn set_job_limits(
        process: &NativeSandboxProcess,
        cpu_cores: f64,
        memory_gb: f64,
    ) -> Result<(), String> {
        if process.job_handle == 0 {
            return Err("native sandbox process group is unavailable".into());
        }
        let job = process.job_handle as HANDLE;
        let mut extended = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        extended.BasicLimitInformation.LimitFlags =
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_JOB_MEMORY;
        extended.JobMemoryLimit = (memory_gb.max(0.25) * 1_073_741_824.0) as usize;
        if unsafe {
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&extended as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        } == 0
        {
            return Err(windows_error("apply native sandbox memory limit"));
        }

        let host_cpus = std::thread::available_parallelism()
            .map(|count| count.get())
            .unwrap_or(1) as f64;
        let rate = ((cpu_cores.max(0.1) / host_cpus) * 10_000.0)
            .round()
            .clamp(1.0, 10_000.0) as u32;
        let mut cpu = JOBOBJECT_CPU_RATE_CONTROL_INFORMATION::default();
        cpu.ControlFlags =
            JOB_OBJECT_CPU_RATE_CONTROL_ENABLE | JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP;
        cpu.Anonymous.CpuRate = rate;
        if unsafe {
            SetInformationJobObject(
                job,
                JobObjectCpuRateControlInformation,
                (&cpu as *const JOBOBJECT_CPU_RATE_CONTROL_INFORMATION).cast(),
                size_of::<JOBOBJECT_CPU_RATE_CONTROL_INFORMATION>() as u32,
            )
        } == 0
        {
            return Err(windows_error("apply native sandbox CPU limit"));
        }
        Ok(())
    }

    pub fn is_running(process: &NativeSandboxProcess) -> Result<bool, String> {
        if process.process_handle == 0 {
            return Ok(false);
        }
        let mut exit_code = 0_u32;
        if unsafe { GetExitCodeProcess(process.process_handle as HANDLE, &mut exit_code) } == 0 {
            return Err(windows_error("inspect native sandbox application"));
        }
        Ok(exit_code == STILL_ACTIVE as u32)
    }

    pub fn stop(process: &mut NativeSandboxProcess) -> Result<(), String> {
        if process.job_handle == 0 {
            close(process);
            return Ok(());
        }
        let job = process.job_handle as HANDLE;
        if unsafe { TerminateJobObject(job, 0) } == 0 {
            let error = windows_error("stop native sandbox process tree");
            close(process);
            return Err(error);
        }
        if process.process_handle != 0 {
            let wait = unsafe { WaitForSingleObject(process.process_handle as HANDLE, 5_000) };
            if wait == WAIT_TIMEOUT {
                close(process);
                return Err("native sandbox application did not stop within five seconds".into());
            }
            if wait != 0 {
                let error = windows_error("verify native sandbox process stopped");
                close(process);
                return Err(error);
            }
        }
        close(process);
        Ok(())
    }

    pub fn close(process: &mut NativeSandboxProcess) {
        unsafe {
            if process.process_handle != 0 {
                CloseHandle(process.process_handle as HANDLE);
                process.process_handle = 0;
            }
            if process.job_handle != 0 {
                CloseHandle(process.job_handle as HANDLE);
                process.job_handle = 0;
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod platform {
    use std::path::Path;

    use super::{NativeSandboxProcess, ResourcePolicy, SandboxPolicy};

    fn unsupported() -> String {
        "Native application branches require Windows 10 or Windows 11".into()
    }

    pub async fn prepare_profile_and_access(
        _id: &str,
        _workspace: &Path,
        _policy: &SandboxPolicy,
    ) -> Result<(), String> {
        Err(unsupported())
    }

    pub async fn remove_profile_and_access(
        _id: &str,
        _policy: Option<&SandboxPolicy>,
    ) -> Result<(), String> {
        Ok(())
    }

    pub fn launch(
        _id: &str,
        _workspace: &Path,
        _policy: &SandboxPolicy,
        _resources: &ResourcePolicy,
    ) -> Result<NativeSandboxProcess, String> {
        Err(unsupported())
    }

    pub fn set_job_limits(
        _process: &NativeSandboxProcess,
        _cpu_cores: f64,
        _memory_gb: f64,
    ) -> Result<(), String> {
        Err(unsupported())
    }

    pub fn is_running(_process: &NativeSandboxProcess) -> Result<bool, String> {
        Ok(false)
    }

    pub fn stop(_process: &mut NativeSandboxProcess) -> Result<(), String> {
        Ok(())
    }

    pub fn close(_process: &mut NativeSandboxProcess) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "windows")]
    use crate::models::{Priority, ResourceRange};

    #[test]
    fn rejects_volume_root_shares() {
        #[cfg(windows)]
        {
        assert!(is_volume_root(Path::new("C:\\")));
        assert!(!is_volume_root(Path::new("C:\\Users")));
        }
        #[cfg(unix)]
        {
            assert!(is_volume_root(Path::new("/")));
            assert!(!is_volume_root(Path::new("/tmp")));
        }
    }

    #[test]
    fn validates_managed_identifiers() {
        assert!(validate_id("env-1234_abcd").is_ok());
        assert!(validate_id("../outside").is_err());
        assert!(validate_id("").is_err());
    }

    #[test]
    fn normalizes_and_deduplicates_explicit_folder_access() {
        let temp = tempfile::tempdir().unwrap();
        let managed = temp.path().join("managed");
        let application = temp.path().join("application.exe");
        let shared = temp.path().join("shared");
        fs::create_dir_all(&managed).unwrap();
        fs::create_dir_all(&shared).unwrap();
        fs::write(&application, b"test executable placeholder").unwrap();
        let policy = SandboxPolicy {
            executable: application.to_string_lossy().into_owned(),
            arguments: "  --safe-mode  ".into(),
            shares: vec![
                SandboxShare {
                    path: shared.to_string_lossy().into_owned(),
                    access: SandboxFileAccess::ReadOnly,
                },
                SandboxShare {
                    path: shared.to_string_lossy().into_owned(),
                    access: SandboxFileAccess::ReadWrite,
                },
            ],
            network_access: false,
        };

        let normalized = normalize_policy(&policy, &managed).unwrap();
        assert_eq!(normalized.arguments, "--safe-mode");
        assert_eq!(normalized.shares.len(), 1);
        assert_eq!(normalized.shares[0].access, SandboxFileAccess::ReadWrite);
    }

    #[test]
    fn rejects_access_to_managed_runtime_data() {
        let temp = tempfile::tempdir().unwrap();
        let managed = temp.path().join("managed");
        let application = temp.path().join("application.exe");
        fs::create_dir_all(&managed).unwrap();
        fs::write(&application, b"test executable placeholder").unwrap();
        let policy = SandboxPolicy {
            executable: application.to_string_lossy().into_owned(),
            arguments: String::new(),
            shares: vec![SandboxShare {
                path: managed.to_string_lossy().into_owned(),
                access: SandboxFileAccess::ReadWrite,
            }],
            network_access: false,
        };

        assert!(normalize_policy(&policy, &managed).is_err());
    }

    #[cfg(target_os = "windows")]
    #[tokio::test]
    #[ignore = "creates a real Windows AppContainer profile and launches a process"]
    async fn launches_a_real_appcontainer_process() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        let executable = std::env::current_exe().unwrap();
        let policy = SandboxPolicy {
            executable: executable.to_string_lossy().into_owned(),
            arguments: "--help".into(),
            shares: Vec::new(),
            network_access: false,
        };
        let resources = ResourcePolicy {
            cpu: ResourceRange {
                min: 0.5,
                preferred: 1.0,
                max: 1.0,
                current: 1.0,
            },
            memory_gb: ResourceRange {
                min: 0.25,
                preferred: 0.5,
                max: 1.0,
                current: 0.5,
            },
            priority: Priority::Normal,
            dynamic: true,
        };
        let id = format!("smoke-{}", uuid::Uuid::new_v4());
        platform::prepare_profile_and_access(&id, &workspace, &policy)
            .await
            .unwrap();
        let mut process = platform::launch(&id, &workspace, &policy, &resources).unwrap();
        for _ in 0..50 {
            if !platform::is_running(&process).unwrap() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        if platform::is_running(&process).unwrap() {
            platform::stop(&mut process).unwrap();
        } else {
            platform::close(&mut process);
        }
        platform::remove_profile_and_access(&id, Some(&policy))
            .await
            .unwrap();
    }
}
