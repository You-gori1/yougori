#[cfg(target_os = "windows")]
use windows_sys::Win32::{
    Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE},
    System::Threading::CreateMutexW,
};

/// Held for the entire desktop-process lifetime so two Yougori instances can
/// never mutate the same appliance overlay, VM disks, or state generations.
pub struct InstanceLock {
    #[cfg(target_os = "windows")]
    handle: isize,
    #[cfg(unix)]
    _file: std::fs::File,
    #[cfg(unix)]
    _previous_file: Option<std::fs::File>,
}

impl InstanceLock {
    pub fn acquire() -> Result<Self, String> {
        #[cfg(target_os = "windows")]
        {
            let name = "Local\\OpenDock.Runtime.Singleton.v1\0"
                .encode_utf16()
                .collect::<Vec<_>>();
            // SAFETY: `name` is NUL-terminated and remains alive for the call. A null
            // security descriptor requests the caller's default mutex ACL.
            let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
            if handle.is_null() {
                return Err(format!(
                    "create the Yougori single-instance lock: Windows error {}",
                    unsafe { GetLastError() }
                ));
            }
            // GetLastError must be read before any other Win32 call.
            let already_running = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
            if already_running {
                unsafe {
                    CloseHandle(handle);
                }
                return Err(
                    "Yougori is already running. Use the existing window before starting another instance."
                        .into(),
                );
            }
            Ok(Self {
                handle: handle as isize,
            })
        }

        #[cfg(unix)]
        {
            use std::os::{fd::AsRawFd, unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt}};
            // The previous release used a different socket directory. Lock its
            // existing inode as well so an older engine cannot own these disks.
            let previous_directory = std::path::PathBuf::from(format!("/tmp/opendock-{}", unsafe { libc::geteuid() }));
            match std::fs::DirBuilder::new().mode(0o700).create(&previous_directory) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.to_string()),
            }
            let parent = std::fs::symlink_metadata(&previous_directory).map_err(|e| e.to_string())?;
            if !parent.is_dir() || parent.file_type().is_symlink()
                || parent.uid() != unsafe { libc::geteuid() } || parent.mode() & 0o077 != 0 {
                return Err("The previous Yougori runtime directory is not private to this user.".into());
            }
            let previous_path = previous_directory.join("runtime.lock");
            let previous_file = std::fs::OpenOptions::new().read(true).write(true)
                .create(true).truncate(false).mode(0o600)
                .open(&previous_path).map_err(|e| e.to_string())?;
            let metadata = std::fs::symlink_metadata(&previous_path).map_err(|e| e.to_string())?;
            if !metadata.is_file() || metadata.file_type().is_symlink()
                || metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
                return Err("The previous Yougori runtime lock is not private to this user.".into());
            }
            if unsafe { libc::flock(previous_file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
                return Err("An older Yougori engine is still running. Close it before starting this version.".into());
            }
            let directory =
                yougori_cli::wire::private_socket_directory().map_err(|e| e.to_string())?;
            // Reserve the runtime before loading/migrating state, not only when
            // the control socket starts later. Keep this inode on disk so two
            // launchers can never lock different generations of the same file.
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .mode(0o600)
                .open(directory.join("runtime.lock"))
                .map_err(|e| e.to_string())?;
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
                return Err("Yougori is already running or its runtime lock is unavailable. Use the existing engine.".into());
            }
            Ok(Self { _file: file, _previous_file: Some(previous_file) })
        }
    }
}

#[cfg(target_os = "windows")]
impl Drop for InstanceLock {
    fn drop(&mut self) {
        if self.handle != 0 {
            unsafe {
                CloseHandle(self.handle as HANDLE);
            }
            self.handle = 0;
        }
    }
}
