//! WSL-owned VHDX access through its native provider. Compaction requires a
//! verified, cleanly stopped distribution and an exclusive runtime owner.
use std::{
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::Path,
};
use windows_sys::Win32::Storage::Vhd::*;

pub(super) fn compact(path: &Path) -> Result<(), String> {
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    if !path.is_absolute() || wide.contains(&0) { return Err("Invalid CUDA storage path".into()); }
    wide.push(0);
    let kind = VIRTUAL_STORAGE_TYPE { DeviceId: VIRTUAL_STORAGE_TYPE_DEVICE_VHDX, VendorId: VIRTUAL_STORAGE_TYPE_VENDOR_MICROSOFT };
    let parameters = OPEN_VIRTUAL_DISK_PARAMETERS {
        Version: OPEN_VIRTUAL_DISK_VERSION_1,
        Anonymous: OPEN_VIRTUAL_DISK_PARAMETERS_0 { Version1: OPEN_VIRTUAL_DISK_PARAMETERS_0_0 { RWDepth: 1 } },
    };
    let mut handle = std::ptr::null_mut();
    // SAFETY: initialized structures and a NUL-terminated path; METAOPS does
    // not attach the disk. A live WSL disk cannot be compacted by this handle.
    // WSL's utility VM has a roughly one-minute idle teardown even after its
    // distributions report Stopped. Never force a global WSL shutdown to hurry it.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(90);
    let status = loop {
        // WSL can return from --terminate before its VHD handle is released.
        // Retry only sharing/lock violations, keeping exclusive app ownership.
        let status = unsafe { OpenVirtualDisk(&kind, wide.as_ptr(), VIRTUAL_DISK_ACCESS_METAOPS, OPEN_VIRTUAL_DISK_FLAG_NONE, &parameters, &mut handle) };
        if !matches!(status, 32 | 33) || std::time::Instant::now() >= deadline { break status; }
        std::thread::sleep(std::time::Duration::from_millis(500));
    };
    if status != 0 { return Err(format!("Windows has not released the CUDA disk for compaction: {}. Data is safe. Close other WSL sessions normally (or restart your PC), then retry Reclaim space before starting GPU containers. No other WSL sessions were stopped.", std::io::Error::from_raw_os_error(status as i32))); }
    // SAFETY: the successful call returns a uniquely owned handle.
    let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
    // SAFETY: the handle remains live for this synchronous operation. NONE
    // performs filesystem-agnostic compaction; no mounting or formatting.
    let status = unsafe { CompactVirtualDisk(handle.as_raw_handle(), COMPACT_VIRTUAL_DISK_FLAG_NONE, std::ptr::null(), std::ptr::null()) };
    if status != 0 { return Err(format!("Compact CUDA disk: {}. Its data was kept; retry Reclaim space after stopping GPU containers.", std::io::Error::from_raw_os_error(status as i32))); }
    Ok(())
}

pub(super) fn sizes(path: &Path) -> Result<(u64, u64), String> {
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    if !path.is_absolute() || wide.contains(&0) {
        return Err("Invalid CUDA storage path".into());
    }
    wide.push(0);
    let kind = VIRTUAL_STORAGE_TYPE {
        DeviceId: VIRTUAL_STORAGE_TYPE_DEVICE_VHDX,
        VendorId: VIRTUAL_STORAGE_TYPE_VENDOR_MICROSOFT,
    };
    let parameters = OPEN_VIRTUAL_DISK_PARAMETERS {
        Version: OPEN_VIRTUAL_DISK_VERSION_2,
        Anonymous: OPEN_VIRTUAL_DISK_PARAMETERS_0 {
            Version2: OPEN_VIRTUAL_DISK_PARAMETERS_0_1 {
                GetInfoOnly: 1,
                ReadOnly: 1,
                ..Default::default()
            },
        },
    };
    let mut handle = std::ptr::null_mut();
    // SAFETY: all structures are initialized; the path is NUL-terminated and
    // all pointers outlive this synchronous call. The handle requests info only.
    let status = unsafe {
        OpenVirtualDisk(
            &kind,
            wide.as_ptr(),
            VIRTUAL_DISK_ACCESS_NONE,
            OPEN_VIRTUAL_DISK_FLAG_NONE,
            &parameters,
            &mut handle,
        )
    };
    if status != 0 {
        return Err(format!(
            "Read CUDA disk information: {}. The disk was not changed.",
            std::io::Error::from_raw_os_error(status as i32)
        ));
    }
    // SAFETY: a successful OpenVirtualDisk returned a unique owned HANDLE.
    let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
    let mut info = GET_VIRTUAL_DISK_INFO {
        Version: GET_VIRTUAL_DISK_INFO_SIZE,
        ..Default::default()
    };
    let mut size = std::mem::size_of_val(&info) as u32;
    // SAFETY: the buffer is valid for `size` bytes and HANDLE remains owned.
    let status = unsafe {
        GetVirtualDiskInformation(
            handle.as_raw_handle(),
            &mut size,
            &mut info,
            std::ptr::null_mut(),
        )
    };
    if status != 0 {
        return Err(format!(
            "Read CUDA disk size: {}",
            std::io::Error::from_raw_os_error(status as i32)
        ));
    }
    // SAFETY: successful GET_VIRTUAL_DISK_INFO_SIZE initializes the Size arm.
    let sizes = unsafe { info.Anonymous.Size };
    if sizes.VirtualSize == 0 {
        return Err("CUDA disk capacity is missing".into());
    }
    Ok((sizes.VirtualSize, sizes.PhysicalSize))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metadata_queries_reject_invalid_paths_without_creating_disks() {
        assert!(compact(Path::new("relative.vhdx")).is_err());
        assert!(compact(Path::new("C:\\bad\0.vhdx")).is_err());
        assert!(sizes(Path::new("relative.vhdx")).is_err());
        assert!(sizes(Path::new("C:\\bad\0.vhdx")).is_err());
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("absent.vhdx");
        assert!(sizes(&path).is_err());
        assert!(compact(&path).is_err());
        assert!(!path.exists());
        std::fs::write(&path, b"not a virtual disk; preserve me").unwrap();
        assert!(compact(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"not a virtual disk; preserve me");
    }
}
