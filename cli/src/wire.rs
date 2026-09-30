use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const VERSION: u32 = 1;
pub const MAX_REQUEST: usize = 1024 * 1024;
pub const MAX_RESPONSE: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    pub version: u32,
    pub method: String,
    #[serde(default = "empty_params")]
    pub params: Value,
    #[serde(default)]
    pub confirmed: bool,
    #[serde(default)]
    pub dry_run: bool,
}
fn empty_params() -> Value {
    serde_json::json!({})
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Response {
    pub version: u32,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
impl Response {
    pub fn success(result: Value) -> Self {
        Self {
            version: VERSION,
            ok: true,
            result: Some(result),
            error: None,
        }
    }
    pub fn failure(error: impl Into<String>) -> Self {
        Self {
            version: VERSION,
            ok: false,
            result: None,
            error: Some(error.into()),
        }
    }
}

pub async fn read_frame(
    stream: &mut (impl AsyncRead + Unpin),
    limit: usize,
) -> io::Result<Vec<u8>> {
    let size = stream.read_u32().await? as usize;
    if size == 0 || size > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Yougori message exceeds its size limit",
        ));
    }
    let mut bytes = vec![0; size];
    stream.read_exact(&mut bytes).await?;
    Ok(bytes)
}
pub async fn write_frame(
    stream: &mut (impl AsyncWrite + Unpin),
    bytes: &[u8],
    limit: usize,
) -> io::Result<()> {
    if bytes.is_empty() || bytes.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Yougori message exceeds its size limit",
        ));
    }
    stream.write_u32(bytes.len() as u32).await?;
    stream.write_all(bytes).await?;
    stream.flush().await
}

#[cfg(windows)]
pub fn user_sid() -> io::Result<String> {
    process_user_sid(unsafe { windows_sys::Win32::System::Threading::GetCurrentProcess() })
}

#[cfg(windows)]
fn process_user_sid(process: windows_sys::Win32::Foundation::HANDLE) -> io::Result<String> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, LocalFree},
        Security::Authorization::ConvertSidToStringSidW,
        Security::{GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER},
        System::Threading::OpenProcessToken,
    };
    // The token buffer is usize-aligned; all pointers live until copied to a String.
    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(process, TOKEN_QUERY, &mut token) == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut needed = 0;
        GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut needed);
        let mut buffer = vec![0usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
        let result = GetTokenInformation(
            token,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            needed,
            &mut needed,
        );
        CloseHandle(token);
        if result == 0 {
            return Err(io::Error::last_os_error());
        }
        let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
        let mut text = std::ptr::null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut text) == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut len = 0;
        while *text.add(len) != 0 {
            len += 1;
        }
        let value = String::from_utf16_lossy(std::slice::from_raw_parts(text, len));
        LocalFree(text.cast());
        Ok(value)
    }
}

#[cfg(windows)]
pub fn verify_pipe_server(stream: &tokio::net::windows::named_pipe::NamedPipeClient) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::{Pipes::GetNamedPipeServerProcessId, Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION}},
    };
    // A protected server DACL controls its clients, but does not prevent an
    // unrelated user from squatting the predictable pipe name before startup.
    // Authenticate the OS owner before sending commands or credential payloads.
    unsafe {
        let mut pid = 0;
        if GetNamedPipeServerProcessId(stream.as_raw_handle(), &mut pid) == 0 {
            return Err(io::Error::last_os_error());
        }
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() { return Err(io::Error::last_os_error()); }
        let owner = process_user_sid(process);
        CloseHandle(process);
        if owner? != user_sid()? {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "Yougori control endpoint belongs to another user"));
        }
    }
    Ok(())
}

/// No network address override: the management API is never a TCP service and
/// cannot be reached through My PC, guest ports, or Cloudflare publications.
pub fn endpoint() -> io::Result<String> {
    #[cfg(windows)]
    {
        Ok(format!(r"\\.\pipe\Yougori.Control.v1.{}", user_sid()?))
    }
    #[cfg(unix)]
    {
        Ok(format!("/tmp/yougori-{}/control-v1.sock", unsafe {
            libc::geteuid()
        }))
    }
}

#[cfg(unix)]
pub fn private_socket_directory() -> io::Result<std::path::PathBuf> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt};
    let path = std::path::PathBuf::from(format!("/tmp/yougori-{}", unsafe { libc::geteuid() }));
    match std::fs::DirBuilder::new().mode(0o700).create(&path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    let meta = std::fs::symlink_metadata(&path)?;
    if !meta.is_dir() || meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o077 != 0 {
        return Err(io::Error::other(
            "Yougori socket directory must be owned by this user with mode 0700",
        ));
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_frames_roundtrip_and_reject_oversize() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let (mut a, mut b) = tokio::io::duplex(128);
                write_frame(&mut a, b"hello", 8).await.unwrap();
                assert_eq!(read_frame(&mut b, 8).await.unwrap(), b"hello");
                a.write_u32(1024).await.unwrap();
                assert!(read_frame(&mut b, 8).await.is_err());
                assert!(write_frame(&mut a, b"oversized", 2).await.is_err());
            });
    }
    #[test]
    fn requests_reject_unknown_envelope_fields() {
        assert!(serde_json::from_value::<Request>(
            serde_json::json!({"version":1,"method":"state","admin":true})
        )
        .is_err());
        assert!(
            !serde_json::from_value::<Request>(serde_json::json!({"version":1,"method":"state"}))
                .unwrap()
                .confirmed
        );
        assert!(!endpoint().unwrap().contains("127.0.0.1"));
    }

    #[cfg(windows)]
    #[test]
    fn local_pipe_server_owner_is_verified_before_request_data() {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
            let endpoint = format!(r"\\.\pipe\Yougori.OwnerTest.{}.{}", std::process::id(),
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
            let server = tokio::net::windows::named_pipe::ServerOptions::new()
                .first_pipe_instance(true).create(&endpoint).unwrap();
            let client = tokio::net::windows::named_pipe::ClientOptions::new().open(&endpoint).unwrap();
            server.connect().await.unwrap();
            verify_pipe_server(&client).unwrap();
            // No protocol bytes are needed to establish the owner's identity.
            let mut bytes = [0u8; 1];
            assert_eq!(server.try_read(&mut bytes).unwrap_err().kind(), io::ErrorKind::WouldBlock);
        });
    }
}
