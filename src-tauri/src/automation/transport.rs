use std::io;

#[cfg(windows)]
pub type Listener = tokio::net::windows::named_pipe::NamedPipeServer;
#[cfg(unix)]
pub type Listener = tokio::net::UnixListener;

#[cfg(windows)]
pub fn bind(endpoint: &str, first: bool) -> io::Result<Listener> {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW,
            SECURITY_ATTRIBUTES,
        },
    };
    let sid = yougori_cli::wire::user_sid()?;
    // Explicit protected DACL: only this Windows user. No Everyone/Anonymous/
    // network clients; a guest or arbitrary website cannot open this pipe.
    let sddl = format!("D:P(A;;GA;;;{sid})\0")
        .encode_utf16()
        .collect::<Vec<_>>();
    unsafe {
        let mut descriptor = std::ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut descriptor,
            std::ptr::null_mut(),
        ) == 0
        {
            return Err(io::Error::last_os_error());
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
                endpoint,
                (&mut attributes as *mut SECURITY_ATTRIBUTES).cast(),
            );
        LocalFree(descriptor);
        result
    }
}

#[cfg(unix)]
pub fn bind(endpoint: &str, _first: bool) -> io::Result<Listener> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
    let path = std::path::Path::new(endpoint);
    let parent = yougori_cli::wire::private_socket_directory()?;
    if path.parent() != Some(parent.as_path()) {
        return Err(io::Error::other("Unexpected control socket directory"));
    }
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        if !meta.file_type().is_socket() || meta.uid() != unsafe { libc::geteuid() } {
            return Err(io::Error::other(
                "Unexpected file at the control socket path",
            ));
        }
        match std::os::unix::net::UnixStream::connect(path) {
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    "Yougori control socket is already active",
                ))
            }
            Err(e) if e.kind() == io::ErrorKind::ConnectionRefused => std::fs::remove_file(path)?,
            Err(e) => return Err(e),
        }
    }
    let socket = Listener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(socket)
}
