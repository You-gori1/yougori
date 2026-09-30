use std::{
    ffi::c_void,
    path::{Path, PathBuf},
    ptr::null_mut,
};
use windows_sys::Win32::Storage::FileSystem::{FILE_GENERIC_EXECUTE, FILE_GENERIC_READ};
use windows_sys::Win32::{
    Foundation::*,
    Security::{Authorization::*, *},
    System::{Pipes::*, Threading::*},
    UI::{Shell::*, WindowsAndMessaging::*},
};

pub(crate) fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
fn last() -> String {
    std::io::Error::last_os_error().to_string()
}
pub(crate) struct Handle(pub HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

fn known(id: &windows_sys::core::GUID) -> Result<PathBuf, String> {
    unsafe {
        let mut ptr = null_mut();
        if SHGetKnownFolderPath(id, 0, null_mut(), &mut ptr) < 0 {
            return Err("Cannot locate Windows protected folders".into());
        }
        let mut len = 0;
        while *ptr.add(len) != 0 {
            len += 1;
        }
        let result = PathBuf::from(String::from_utf16_lossy(std::slice::from_raw_parts(
            ptr, len,
        )));
        windows_sys::Win32::System::Com::CoTaskMemFree(ptr.cast());
        Ok(result)
    }
}
pub fn installed_binary() -> Result<PathBuf, String> {
    Ok(known(&FOLDERID_ProgramFiles)?
        .join("Yougori Vault")
        .join("yougori-vault.exe"))
}
pub(crate) fn data_directory() -> Result<PathBuf, String> {
    Ok(known(&FOLDERID_ProgramData)?
        .join("YougoriVault")
        .join(user_sid()?))
}

fn token(process: HANDLE) -> Result<Handle, String> {
    unsafe {
        let mut h = null_mut();
        if OpenProcessToken(process, TOKEN_QUERY, &mut h) == 0 {
            return Err(last());
        }
        Ok(Handle(h))
    }
}
fn sid_of_token(token: HANDLE) -> Result<String, String> {
    unsafe {
        let mut size = 0;
        GetTokenInformation(token, TokenUser, null_mut(), 0, &mut size);
        let mut buf = vec![0u8; size as usize];
        if GetTokenInformation(token, TokenUser, buf.as_mut_ptr().cast(), size, &mut size) == 0 {
            return Err(last());
        }
        let sid = (*(buf.as_ptr() as *const TOKEN_USER)).User.Sid;
        sid_text(sid)
    }
}
pub(crate) unsafe fn sid_text(sid: PSID) -> Result<String, String> {
    let mut value = null_mut();
    if ConvertSidToStringSidW(sid, &mut value) == 0 {
        return Err(last());
    }
    let mut len = 0;
    while *value.add(len) != 0 {
        len += 1;
    }
    let text = String::from_utf16_lossy(std::slice::from_raw_parts(value, len));
    LocalFree(value.cast());
    Ok(text)
}
pub fn user_sid() -> Result<String, String> {
    sid_of_token(token(unsafe { GetCurrentProcess() })?.0)
}
fn elevated(process: HANDLE) -> Result<bool, String> {
    unsafe {
        let token = token(process)?;
        let mut elevation = TOKEN_ELEVATION::default();
        let mut size = 0;
        if GetTokenInformation(
            token.0,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut size,
        ) == 0
        {
            return Err(last());
        }
        Ok(elevation.TokenIsElevated != 0)
    }
}
fn path_of_process(process: HANDLE) -> Result<PathBuf, String> {
    unsafe {
        let mut path = vec![0u16; 32768];
        let mut len = path.len() as u32;
        if QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &mut len) == 0 {
            return Err(last());
        }
        Ok(PathBuf::from(String::from_utf16_lossy(
            &path[..len as usize],
        )))
    }
}
pub fn endpoint() -> Result<String, String> {
    Ok(format!(r"\\.\pipe\Yougori.ProtectedVault.{}", user_sid()?))
}

pub(crate) fn no_reparse(path: &Path) -> Result<(), String> {
    use std::os::windows::fs::MetadataExt;
    if std::fs::symlink_metadata(path)
        .map_err(|e| e.to_string())?
        .file_attributes()
        & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
        != 0
    {
        return Err("Vault paths cannot be junctions or symbolic links".into());
    }
    Ok(())
}
// Owner must be Administrators, not the filtered interactive user: an owner can
// rewrite a DACL even if ordinary file writes are denied.
pub(crate) fn secure(path: &Path, public_read: bool) -> Result<(), String> {
    no_reparse(path)?;
    let sddl = if public_read {
        "O:BAG:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;GRGX;;;BU)"
    } else {
        "O:BAG:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)"
    };
    unsafe {
        let mut descriptor = null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            wide(sddl).as_ptr(),
            1,
            &mut descriptor,
            null_mut(),
        ) == 0
        {
            return Err(last());
        }
        let result = SetFileSecurityW(
            wide(&path.to_string_lossy()).as_ptr(),
            OWNER_SECURITY_INFORMATION
                | GROUP_SECURITY_INFORMATION
                | DACL_SECURITY_INFORMATION
                | PROTECTED_DACL_SECURITY_INFORMATION,
            descriptor,
        );
        LocalFree(descriptor);
        if result == 0 {
            return Err(last());
        }
    }
    verify_protected(path, public_read)
}
pub(crate) fn verify_protected(path: &Path, public_read: bool) -> Result<(), String> {
    no_reparse(path)?;
    unsafe {
        let mut owner = null_mut();
        let mut dacl = null_mut();
        let mut descriptor = null_mut();
        let result = GetNamedSecurityInfoW(
            wide(&path.to_string_lossy()).as_ptr(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            &mut dacl,
            null_mut(),
            &mut descriptor,
        );
        if result != 0 {
            return Err("Cannot verify vault file protections".into());
        }
        let guard = HandleLocal(descriptor);
        let owner = sid_text(owner)?;
        if !matches!(owner.as_str(), "S-1-5-32-544" | "S-1-5-18") || dacl.is_null() {
            return Err("Vault requires administrator-owned files with a restrictive ACL".into());
        }
        for index in 0..(*dacl).AceCount {
            let mut ace: *mut c_void = null_mut();
            if GetAce(dacl, index as u32, &mut ace) == 0 {
                return Err(last());
            }
            let header = &*(ace as *const ACE_HEADER);
            if header.AceType != 0 {
                return Err("Unexpected vault file access rule".into());
            } // ACCESS_ALLOWED_ACE_TYPE
            let allowed = &*(ace as *const ACCESS_ALLOWED_ACE);
            let sid = sid_text((&allowed.SidStart as *const u32).cast_mut().cast())?;
            if matches!(sid.as_str(), "S-1-5-32-544" | "S-1-5-18") {
                continue;
            }
            let read_only =
                FILE_GENERIC_READ | FILE_GENERIC_EXECUTE | GENERIC_READ | GENERIC_EXECUTE;
            if !public_read || sid != "S-1-5-32-545" || allowed.Mask & !read_only != 0 {
                return Err("Vault path is writable outside its trusted broker".into());
            }
        }
        drop(guard);
        Ok(())
    }
}
struct HandleLocal(*mut c_void);
impl Drop for HandleLocal {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}

pub(crate) fn require_broker() -> Result<(), String> {
    unsafe {
        windows_sys::Win32::System::LibraryLoader::SetDefaultDllDirectories(
            windows_sys::Win32::System::LibraryLoader::LOAD_LIBRARY_SEARCH_SYSTEM32
                | windows_sys::Win32::System::LibraryLoader::LOAD_LIBRARY_SEARCH_APPLICATION_DIR,
        );
    }
    if !elevated(unsafe { GetCurrentProcess() })? {
        return Err(
            "The vault broker needs Windows administrator protection. Open it from Personal Vault."
                .into(),
        );
    }
    let expected = installed_binary()?;
    if std::env::current_exe().map_err(|e| e.to_string())? != expected {
        return Err("The vault can only run from its protected installation".into());
    }
    verify_protected(expected.parent().unwrap(), true)?;
    verify_protected(&expected, true)?;
    Ok(())
}
pub fn launch(bundled: &Path, allow_elevation: bool) -> Result<(), String> {
    let installed = installed_binary()?;
    if installed.exists() && !update_required(bundled)? {
        verify_protected(installed.parent().unwrap(), true)?;
        verify_protected(&installed, true)?;
        match crate::task::run(&installed, &user_sid()?) {
            Ok(()) => Ok(()),
            Err(error) if allow_elevation => {
                // A failed task must be repaired, not bypassed for this one
                // launch; otherwise the next Desktop start prompts again.
                elevate(&installed, &format!("--install --owner {}", user_sid()?))
                    .map_err(|e| format!("Protected vault startup task failed: {error}. {e}"))
            }
            Err(error) => Err(format!("Protected vault startup task failed: {error}. Open Personal Vault MCP and choose Repair startup to fix it.")),
        }
    } else {
        if !bundled.is_file() {
            return Err("The protected vault component is missing. Reinstall Yougori.".into());
        }
        if !allow_elevation {
            return Err("Personal Vault needs an update. Open Personal Vault MCP and choose Update vault.".into());
        }
        elevate(bundled, &format!("--install --owner {}", user_sid()?))
    }
}
pub fn update_required(bundled: &Path) -> Result<bool, String> {
    use sha2::{Digest, Sha256};
    let installed = installed_binary()?;
    if !installed.exists() || !bundled.is_file() {
        return Ok(false);
    }
    verify_protected(installed.parent().unwrap(), true)?;
    verify_protected(&installed, true)?;
    let digest = |path: &Path| -> Result<_, String> {
        let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let mut hash = Sha256::new();
        std::io::copy(&mut file, &mut hash).map_err(|e| e.to_string())?;
        Ok(hash.finalize())
    };
    Ok(digest(bundled)? != digest(&installed)?)
}
fn elevate(path: &Path, args: &str) -> Result<(), String> {
    unsafe {
        let directory = path.parent().ok_or("Invalid broker path")?;
        let result = ShellExecuteW(
            null_mut(),
            wide("runas").as_ptr(),
            wide(&path.to_string_lossy()).as_ptr(),
            wide(args).as_ptr(),
            wide(&directory.to_string_lossy()).as_ptr(),
            SW_HIDE,
        );
        if result as usize <= 32 {
            return Err(
                "Windows did not start the protected vault. No vault data was opened.".into(),
            );
        }
    }
    Ok(())
}
pub fn install() -> Result<(), String> {
    if !elevated(unsafe { GetCurrentProcess() })? {
        return Err("Windows approval is required to install Personal Vault".into());
    }
    let destination = installed_binary()?;
    let directory = destination.parent().unwrap();
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    secure(directory, true)?;
    // Stage and sync the whole new executable before stopping the old process.
    // An interrupted copy must not damage the existing protected installation.
    let staged = tempfile::NamedTempFile::new_in(directory).map_err(|e| e.to_string())?;
    std::fs::copy(
        std::env::current_exe().map_err(|e| e.to_string())?,
        staged.path(),
    )
    .map_err(|e| e.to_string())?;
    secure(staged.path(), true)?;
    staged.as_file().sync_all().map_err(|e| e.to_string())?;
    if destination.exists() {
        verify_protected(&destination, true)?;
        stop_installed_broker(&destination)?;
    }
    staged.persist(&destination).map_err(|e| e.to_string())?;
    verify_protected(&destination, true)?;
    let data = data_directory()?;
    std::fs::create_dir_all(data.parent().unwrap()).map_err(|e| e.to_string())?;
    secure(data.parent().unwrap(), false)?;
    std::fs::create_dir_all(&data).map_err(|e| e.to_string())?;
    secure(&data, false)?;
    crate::task::install(&destination, &user_sid()?)?;
    use std::os::windows::process::CommandExt;
    std::process::Command::new(&destination)
        .arg("--serve")
        .arg("--owner")
        .arg(user_sid()?)
        .current_dir(directory)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}
// Only the UAC-elevated installer can replace the protected broker. Stop only
// this account's verified installed executable; never stop desktop/workloads.
fn stop_installed_broker(destination: &Path) -> Result<(), String> {
    use windows_sys::Win32::System::Diagnostics::ToolHelp::*;
    unsafe {
        let snapshot = Handle(CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0));
        if snapshot.0 == INVALID_HANDLE_VALUE {
            return Err(last());
        }
        let owner = user_sid()?;
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut found = Process32FirstW(snapshot.0, &mut entry);
        while found != 0 {
            let end = entry
                .szExeFile
                .iter()
                .position(|v| *v == 0)
                .unwrap_or(entry.szExeFile.len());
            if entry.th32ProcessID != GetCurrentProcessId()
                && String::from_utf16_lossy(&entry.szExeFile[..end])
                    .eq_ignore_ascii_case("yougori-vault.exe")
            {
                let process = Handle(OpenProcess(
                    PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | PROCESS_SYNCHRONIZE,
                    0,
                    entry.th32ProcessID,
                ));
                if !process.0.is_null()
                    && path_of_process(process.0).is_ok_and(|path| path == destination)
                    && sid_of_token(token(process.0)?.0)? == owner
                    && elevated(process.0)?
                {
                    if TerminateProcess(process.0, 0) == 0
                        || WaitForSingleObject(process.0, 5000) != WAIT_OBJECT_0
                    {
                        return Err(
                            "Close the Personal Vault broker before installing its update.".into(),
                        );
                    }
                }
            }
            found = Process32NextW(snapshot.0, &mut entry);
        }
    }
    Ok(())
}

pub(crate) fn verify_server(handle: HANDLE) -> Result<(), String> {
    unsafe {
        let mut pid = 0;
        if GetNamedPipeServerProcessId(handle, &mut pid) == 0 {
            return Err(last());
        }
        let process = Handle(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid));
        if process.0.is_null() {
            return Err(last());
        }
        let expected = installed_binary()?;
        if path_of_process(process.0)? != expected
            || !elevated(process.0)?
            || sid_of_token(token(process.0)?.0)? != user_sid()?
        {
            return Err("Cannot authenticate the protected vault broker".into());
        }
        verify_protected(expected.parent().unwrap(), true)?;
        verify_protected(&expected, true)
    }
}
pub(crate) fn peer(handle: HANDLE) -> Result<crate::policy::Identity, String> {
    use sha2::{Digest, Sha256};
    unsafe {
        let mut pid = 0;
        if GetNamedPipeClientProcessId(handle, &mut pid) == 0 {
            return Err(last());
        }
        let process = Handle(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid));
        if process.0.is_null() {
            return Err(last());
        }
        let sid = sid_of_token(token(process.0)?.0)?;
        if sid != user_sid()? {
            return Err("A different Windows user cannot access this vault".into());
        }
        let path = path_of_process(process.0)?;
        if !crate::policy::safe_text(&path.to_string_lossy(), 2048) {
            return Err("Client executable path cannot be displayed safely".into());
        }
        let mut file =
            std::fs::File::open(&path).map_err(|_| "Cannot authenticate client executable")?;
        let mut digest = Sha256::new();
        std::io::copy(&mut file, &mut digest)
            .map_err(|_| "Cannot fingerprint client executable")?;
        let mut name = vec![0u16; 256];
        let mut len = name.len() as u32;
        if windows_sys::Win32::System::WindowsProgramming::GetComputerNameW(
            name.as_mut_ptr(),
            &mut len,
        ) == 0
        {
            return Err(last());
        }
        let mut identity = crate::policy::Identity::local(
            path.to_string_lossy().into(),
            hex::encode(digest.finalize()),
            String::from_utf16_lossy(&name[..len as usize]),
            sid.clone(),
            pid,
        );
        if let Some((launcher, hash)) = launcher(pid, process.0, &sid) {
            identity.environment =
                format!("Host computer; verified launcher: {launcher}\nLauncher SHA-256: {hash}");
            identity.id = hex::encode(Sha256::digest(format!(
                "{}\0{launcher}\0{hash}",
                identity.id
            )));
        }
        Ok(identity)
    }
}

pub(crate) fn desktop_process(identity: &crate::policy::Identity) -> bool {
    let executable = std::path::Path::new(&identity.executable);
    executable.file_name().is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("yougori.exe"))
}

pub(crate) fn desktop_foreground(identity: &crate::policy::Identity) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    if !desktop_process(identity) { return false; }
    unsafe {
        let window = GetForegroundWindow();
        if window.is_null() { return false; }
        let mut process = 0;
        GetWindowThreadProcessId(window, &mut process);
        process == identity.process
    }
}
unsafe fn launcher(pid: u32, child: HANDLE, sid: &str) -> Option<(String, String)> {
    use sha2::{Digest, Sha256};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::*;
    let snapshot = Handle(CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0));
    if snapshot.0 == INVALID_HANDLE_VALUE {
        return None;
    }
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut found = Process32FirstW(snapshot.0, &mut entry);
    while found != 0 {
        if entry.th32ProcessID == pid {
            let parent = Handle(OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION,
                0,
                entry.th32ParentProcessID,
            ));
            if parent.0.is_null() {
                return None;
            }
            if sid_of_token(token(parent.0).ok()?.0).ok()? != sid {
                return None;
            }
            let creation = |process| {
                let (mut start, mut end, mut kernel, mut user) = (
                    FILETIME::default(),
                    FILETIME::default(),
                    FILETIME::default(),
                    FILETIME::default(),
                );
                if GetProcessTimes(process, &mut start, &mut end, &mut kernel, &mut user) == 0 {
                    None
                } else {
                    Some(((start.dwHighDateTime as u64) << 32) | start.dwLowDateTime as u64)
                }
            };
            if creation(parent.0)? > creation(child)? {
                return None;
            }
            let path = path_of_process(parent.0).ok()?;
            let name = path.to_string_lossy().into_owned();
            if !crate::policy::safe_text(&name, 2048) {
                return None;
            }
            let mut file = std::fs::File::open(path).ok()?;
            let mut hash = Sha256::new();
            std::io::copy(&mut file, &mut hash).ok()?;
            return Some((name, hex::encode(hash.finalize())));
        }
        found = Process32NextW(snapshot.0, &mut entry);
    }
    None
}
pub(crate) fn bind(
    first: bool,
) -> Result<tokio::net::windows::named_pipe::NamedPipeServer, String> {
    unsafe {
        let sddl = wide(&format!(
            "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;{})S:(ML;;NW;;;ME)",
            user_sid()?
        ));
        let mut descriptor = null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut descriptor,
            null_mut(),
        ) == 0
        {
            return Err(last());
        }
        let mut attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        let result = tokio::net::windows::named_pipe::ServerOptions::new()
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .max_instances(32)
            .create_with_security_attributes_raw(
                endpoint()?,
                (&mut attributes as *mut SECURITY_ATTRIBUTES).cast(),
            );
        LocalFree(descriptor);
        result.map_err(|_| {
            "Cannot open the protected vault endpoint. Another broker may already be running."
                .into()
        })
    }
}
