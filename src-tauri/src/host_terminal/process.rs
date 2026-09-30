//! Keep terminal child processes tied to their terminal on Windows. No global
//! process enumeration, arbitrary PID termination, or administrator elevation.
#[cfg(windows)]
pub struct ProcessScope(isize);
#[cfg(unix)]
pub struct ProcessScope;

pub fn is_elevated() -> Result<bool, String> {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::{
            Foundation::CloseHandle,
            Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY},
            System::Threading::{GetCurrentProcess, OpenProcessToken},
        };
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err("Cannot verify host terminal privilege level".into());
        }
        let mut elevation: TOKEN_ELEVATION = std::mem::zeroed();
        let mut length = 0;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut length,
        );
        CloseHandle(token);
        if ok == 0 {
            return Err("Cannot verify host terminal privilege level".into());
        }
        Ok(elevation.TokenIsElevated != 0)
    }
    #[cfg(unix)]
    {
        Ok(unsafe { libc::geteuid() } == 0)
    }
}
impl ProcessScope {
    pub fn attach(child: &(dyn portable_pty::Child + Send + Sync)) -> Result<Self, String> {
        #[cfg(windows)]
        unsafe {
            use windows_sys::Win32::{
                Foundation::CloseHandle,
                System::JobObjects::{
                    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
                    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                },
            };
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if handle.is_null() {
                return Err("Cannot contain host terminal child processes".into());
            }
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let configured = SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&limits) as u32,
            ) != 0;
            let assigned = configured
                && child
                    .as_raw_handle()
                    .is_some_and(|process| AssignProcessToJobObject(handle, process) != 0);
            if !assigned {
                CloseHandle(handle);
                return Err(
                    "Cannot safely contain the host shell; no interactive terminal was started"
                        .into(),
                );
            }
            Ok(Self(handle as isize))
        }
        #[cfg(unix)]
        {
            let _ = child;
            Ok(Self)
        }
    }
}
#[cfg(windows)]
impl Drop for ProcessScope {
    fn drop(&mut self) {
        unsafe {
            // Explicit termination also handles another attached program holding
            // a job handle. Only this terminal's unnamed job is affected.
            windows_sys::Win32::System::JobObjects::TerminateJobObject(self.0 as _, 1);
            windows_sys::Win32::Foundation::CloseHandle(self.0 as _);
        }
    }
}
