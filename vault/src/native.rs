//! Windows session state used by the vault broker. Vault entry and approvals live in Yougori Desktop.
use std::ptr::null_mut;
use windows_sys::Win32::{
    System::StationsAndDesktops::*,
};

pub(crate) fn workstation_locked() -> bool {
    use windows_sys::Win32::System::RemoteDesktop::*;
    unsafe {
        let mut buffer = null_mut();
        let mut size = 0;
        if WTSQuerySessionInformationW(
            WTS_CURRENT_SERVER_HANDLE,
            WTS_CURRENT_SESSION,
            WTSSessionInfoEx,
            &mut buffer,
            &mut size,
        ) == 0 {
            return true;
        }
        let unlocked = if size as usize >= std::mem::size_of::<WTSINFOEXW>() {
            let info = &*(buffer as *const WTSINFOEXW);
            info.Level == 1
                && info.Data.WTSInfoExLevel1.SessionFlags == WTS_SESSIONSTATE_UNLOCK as i32
                && info.Data.WTSInfoExLevel1.SessionState == WTSActive
        } else {
            false
        };
        WTSFreeMemory(buffer.cast());
        !unlocked
    }
}

pub(crate) fn unlocked_desktop() -> bool {
    !workstation_locked() && input_desktop_name().is_some_and(|s| s.eq_ignore_ascii_case("Default"))
}

fn input_desktop_name() -> Option<String> {
    unsafe {
        let desktop = OpenInputDesktop(0, 0, DESKTOP_READOBJECTS);
        if desktop.is_null() {
            return None;
        }
        let mut name = vec![0u16; 256];
        let mut length = 0;
        let okay = GetUserObjectInformationW(
            desktop,
            UOI_NAME,
            name.as_mut_ptr().cast(),
            (name.len() * 2) as u32,
            &mut length,
        );
        CloseDesktop(desktop);
        if okay == 0 {
            return None;
        }
        let end = name.iter().position(|n| *n == 0).unwrap_or(name.len());
        Some(String::from_utf16_lossy(&name[..end]))
    }
}