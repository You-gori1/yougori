//! One application connection over pinned SSH. Dropping it terminates its SSH child.
use super::*;
use std::{pin::Pin, task::{Context, Poll}};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

pub(crate) struct ServiceStream {
    _child: tokio::process::Child,
    input: tokio::process::ChildStdin,
    output: tokio::process::ChildStdout,
}
impl AsyncRead for ServiceStream {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.output).poll_read(cx, buf)
    }
}
impl AsyncWrite for ServiceStream {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.input).poll_write(cx, buf)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.input).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.input).poll_shutdown(cx)
    }
}
impl Cloud {
    /// Project launches publish only private Docker bindings from labelled
    /// Yougori project containers, never an arbitrary service on the server.
    pub(crate) async fn project_service(&self, id: &str, port: u16) -> Result<bool, String> {
        let reply = self.session(id).await?.request("exec", json!({"command": "docker ps -q --filter label=com.yougori.project | xargs -r docker inspect --format '{{json .NetworkSettings.Ports}}'"})).await?;
        if reply["exitCode"] != 0 { return Ok(false); }
        Ok(reply["stdout"].as_str().unwrap_or("").lines().filter_map(|line| serde_json::from_str::<Value>(line).ok()).any(|bindings| project_binding(&bindings, port)))
    }
    pub(crate) async fn service_target(&self, id: &str, port: u16) -> Result<ServiceTarget, String> {
        self.session(id).await?;
        if port == 0 || port == 7443 { return Err("Invalid application port".into()); }
        let profile = self.profile(id)?;
        validate(&profile, true)?;
        Ok(ServiceTarget { profile, known: self.directory(id)?.join("known_hosts"), port })
    }
    pub(crate) async fn service_stream(&self, id: &str, port: u16) -> Result<ServiceStream, String> {
        self.service_target(id, port).await?.connect()
    }
}

fn project_binding(bindings: &Value, port: u16) -> bool {
    port != 0 && port != 7443 && bindings.as_object().into_iter().flat_map(|ports| ports.iter())
        .filter(|(name, _)| name.ends_with("/tcp")).map(|(_, value)| value)
        .filter_map(Value::as_array).flatten()
        .any(|binding| binding["HostIp"] == "127.0.0.1" && binding["HostPort"].as_str().and_then(|p| p.parse::<u16>().ok()) == Some(port))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn project_publications_require_the_exact_private_docker_binding() {
        let private = json!({"43119/tcp":[{"HostIp":"127.0.0.1","HostPort":"49152"}]});
        assert!(project_binding(&private, 49152));
        assert!(!project_binding(&private, 43119));
        assert!(!project_binding(&json!({"43119/tcp":[{"HostIp":"0.0.0.0","HostPort":"49152"}]}), 49152));
        assert!(!project_binding(&json!({"43119/tcp":null}), 49152));
        assert!(!project_binding(&json!({"43119/udp":[{"HostIp":"127.0.0.1","HostPort":"49152"}]}), 49152));
    }
}
#[derive(Clone)]
pub(crate) struct ServiceTarget { profile: Profile, known: PathBuf, port: u16 }
impl ServiceTarget {
    pub(crate) fn connect(&self) -> Result<ServiceStream, String> {
        let mut cmd = ssh_service_command(&self.profile, &self.known, Some(self.port))?;
        let mut child = cmd.stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null()).spawn().map_err(|e| format!("Open model SSH tunnel: {e}"))?;
        Ok(ServiceStream { input: child.stdin.take().unwrap(), output: child.stdout.take().unwrap(), _child: child })
    }
}
