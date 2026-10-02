use std::io::IsTerminal;

fn success_output(result: serde_json::Value) -> String {
    serde_json::to_string_pretty(&crate::wire::Response::success(result)).unwrap()
}

fn identity_output(path: &str, fingerprint: &str, terminal: bool) -> String {
    if terminal {
        format!("Device identity created: {path}\nSHA-256: {fingerprint}\nCompare this fingerprint with the native connection popup. This key authenticates requests; it cannot approve operations.")
    } else {
        success_output(serde_json::json!({
            "path": path,
            "fingerprint": fingerprint,
            "canApprove": false,
        }))
    }
}

pub async fn run(args: &[String]) -> Result<i32, String> {
    match args {
        [arg] if arg == "mcp" => local().await,
        [arg] if matches!(arg.as_str(), "status" | "open" | "approve" | "add") => {
            crate::client::start(None).await?;
            let (method, params) = match arg.as_str() {
                "status" => ("vault_summary", serde_json::json!({})),
                "open" => ("open_personal_vault", serde_json::json!({"view":"home"})),
                "approve" => ("open_personal_vault", serde_json::json!({"view":"approvals"})),
                _ => ("open_personal_vault", serde_json::json!({"view":"add"})),
            };
            let result = crate::public::call(method, params).await?;
            println!("{}", success_output(result));
            Ok(0)
        }
        [command, output, path] if command == "identity" && output == "--output" => {
            let fingerprint=yougori_vault::transport::DeviceKey::create_file(std::path::Path::new(path))?;
            println!("{}", identity_output(path, &fingerprint, std::io::stdout().is_terminal()));
            Ok(0)
        }
        [command, remote, endpoint, pin_flag, pin, identity_flag, identity] if command == "mcp" && remote == "--remote" && pin_flag == "--pin" && identity_flag == "--identity" => {
            bridge(yougori_vault::transport::connect(endpoint,pin,std::path::Path::new(identity)).await?).await
        }
        [arg] if matches!(arg.as_str(), "help" | "--help" | "-h") => {
            println!("yougori vault status             Is the vault running; how many requests wait for approval\nyougori vault open | approve | add  Bring Yougori forward on Personal Vault; you decide there\nyougori vault mcp\nyougori vault identity --output DEVICE.json\nyougori vault mcp --remote HOST:49731 --pin SERVER_SHA256 --identity DEVICE.json\n\nOpen and unlock Personal Vault in Yougori Desktop first. Every connection and protected operation needs approval in Yougori Desktop. This command cannot add, read, or approve vault credentials.");
            Ok(0)
        }
        _ => Err("Usage: yougori vault status | open | approve | add | mcp | identity --output FILE | mcp --remote HOST:49731 --pin SHA256 --identity FILE".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_vault_output_uses_the_machine_contract() {
        let result = serde_json::json!({"locked": true, "pendingRequests": 2});
        let value: serde_json::Value = serde_json::from_str(&success_output(result.clone())).unwrap();
        assert_eq!(value["version"], crate::wire::VERSION);
        assert_eq!(value["ok"], true);
        assert_eq!(value["result"], result);
        assert!(value.get("error").is_none());
    }

    #[test]
    fn identity_has_structured_piped_output_and_terminal_guidance() {
        let value: serde_json::Value = serde_json::from_str(&identity_output("device.json", "abc123", false)).unwrap();
        assert_eq!(value["version"], crate::wire::VERSION);
        assert_eq!(value["result"]["path"], "device.json");
        assert_eq!(value["result"]["fingerprint"], "abc123");
        assert_eq!(value["result"]["canApprove"], false);
        let terminal = identity_output("device.json", "abc123", true);
        assert!(terminal.contains("SHA-256: abc123"));
        assert!(terminal.contains("cannot approve operations"));
    }
}
#[cfg(not(windows))]
async fn local() -> Result<i32, String> {
    Err("A local Personal Vault requires Windows. Use vault mcp --remote to connect to a protected Windows vault.".into())
}
#[cfg(windows)]
async fn local() -> Result<i32, String> {
    bridge(yougori_vault::client::connect().await?).await
}

async fn bridge<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static>(
    stream: S,
) -> Result<i32, String> {
    use std::io::{BufRead, Read, Write};
    use yougori_vault::protocol;
    let (mut reader, mut writer) = tokio::io::split(stream);
    let (input, mut incoming) = tokio::sync::mpsc::channel::<Result<Vec<u8>, String>>(2);
    let (closed, mut ended) = tokio::sync::watch::channel(false);
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut stdin = stdin.lock();
        loop {
            let mut bytes = vec![];
            match (&mut stdin)
                .take(protocol::MAX_FRAME as u64 + 1)
                .read_until(b'\n', &mut bytes)
            {
                Ok(0) => break,
                Ok(_) if bytes.len() <= protocol::MAX_FRAME => {
                    if input.blocking_send(Ok(bytes)).is_err() {
                        break;
                    }
                }
                _ => {
                    let _ = input
                        .blocking_send(Err("MCP input exceeds 256 KB or could not be read".into()));
                    break;
                }
            }
        }
        let _ = closed.send(true);
    });
    // A dedicated reader preserves framing when input arrives halfway through
    // a reply; read_exact is not cancellation-safe inside tokio::select!.
    let (responses, mut replies) = tokio::sync::mpsc::channel(2);
    let reading = tokio::spawn(async move {
        loop {
            let result = protocol::read(&mut reader).await;
            let stop = result.is_err();
            if responses.send(result).await.is_err() || stop {
                break;
            }
        }
    });
    let result=async { loop {
        tokio::select! {
            _ = ended.changed() => return Ok(0),
            Some(bytes) = incoming.recv() => {
                let bytes = bytes?;
                if bytes.iter().all(|b|b.is_ascii_whitespace()) { continue; }
                let message:serde_json::Value=serde_json::from_slice(&bytes).map_err(|_|"Invalid MCP JSON input")?;
                protocol::write(&mut writer,&serde_json::json!({"method":"mcp","message":message})).await?;
            }
            bytes = replies.recv() => {
                let message:serde_json::Value=serde_json::from_slice(&bytes.ok_or("Vault connection closed")??).map_err(|_|"Invalid vault reply")?;
                if message.is_null() { continue; }
                if message.get("jsonrpc").is_none() { return Err(message["error"].as_str().unwrap_or("Vault rejected the request").into()); }
                let mut stdout=std::io::stdout().lock();
                serde_json::to_writer(&mut stdout,&message).map_err(|_|"MCP output closed")?;
                stdout.write_all(b"\n").and_then(|_|stdout.flush()).map_err(|_|"MCP output closed")?;
            }
        }
    } }.await;
    reading.abort();
    result
}
