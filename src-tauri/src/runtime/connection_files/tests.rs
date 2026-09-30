use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};
pub(crate) fn environment(id: &str, kind: EnvironmentKind) -> Environment {
    serde_json::from_value(json!({"id":id,"name":id,"kind":kind,"status":"running","runtime":"test","description":"","createdAt":"test","cpuUsage":0,"memoryUsageGb":0,"storageDeltaGb":0,"networkRxMbps":0,"resourcePolicy":{"cpu":{"min":1,"preferred":1,"max":1,"current":1},"memoryGb":{"min":1,"preferred":1,"max":1,"current":1},"priority":"normal","dynamic":true}})).unwrap()
}

#[test]
fn selected_folder_must_be_inside_the_guest_filesystem() {
    for path in ["/", "//", "/.", "/workspace/..", "/workspace//data", "/workspace/"] {
        assert!(validate_selected_folder_path(path).is_err(), "accepted {path}");
    }
    for path in ["/workspace", "/home/exampleuser/project", "/root/data"] {
        assert!(validate_selected_folder_path(path).is_ok(), "rejected {path}");
    }
}
async fn request(files: &SharedFiles, env: &str, body: Value) -> (u16, Value) {
    request_at(files, env, "/api", body).await
}
async fn request_at(files: &SharedFiles, env: &str, path: &str, body: Value) -> (u16, Value) {
    let body = serde_json::to_vec(&body).unwrap();
    let mut request=format!("POST {path} HTTP/1.1\r\nHost: 10.192.0.1:7444\r\nX-Yougori-Files: 1\r\nContent-Length: {}\r\n\r\n",body.len()).into_bytes();
    request.extend(body);
    let reply = files.http(env, &request).await;
    let split = reply.windows(4).position(|b| b == b"\r\n\r\n").unwrap();
    (
        std::str::from_utf8(&reply[9..12]).unwrap().parse().unwrap(),
        serde_json::from_slice(&reply[split + 4..]).unwrap(),
    )
}
#[tokio::test]
async fn selected_folder_mount_proxy_requires_its_token_and_stops_on_disconnect() {
    let forward: FileForward = Arc::new(|body| Box::pin(async move {
        if body["operation"] != "list" { return Err("Denied".into()); }
        Ok((200, serde_json::to_vec(&json!({"entries":[{"name":"from-peer.txt","directory":false,"size":4}]})).unwrap()))
    }));
    let server = HostFolderServer::start_forward(forward).await.unwrap();
    let url = format!("http://127.0.0.1:{}/files", server.port);
    let client = reqwest::Client::new();
    assert_eq!(client.post(&url).json(&json!({"operation":"list","path":""})).send().await.unwrap().status().as_u16(), 403);
    let reply = client.post(&url).bearer_auth(&server.token).json(&json!({"operation":"list","path":""})).send().await.unwrap();
    assert_eq!(reply.status().as_u16(), 200);
    assert_eq!(reply.json::<Value>().await.unwrap()["entries"][0]["name"], "from-peer.txt");
    server.stop();
    assert!(client.post(&url).bearer_auth(&server.token).json(&json!({"operation":"list"})).send().await.is_err());
}
#[tokio::test]
async fn selected_guest_folder_and_peer_commands_obey_identity_direction_and_grants() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let agent = tokio::spawn(async move {
        let mut seen = Vec::new();
        for _ in 0..3 {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            loop {
                let mut chunk = [0u8; 8192];
                let size = stream.read(&mut chunk).await.unwrap();
                assert!(size > 0);
                bytes.extend_from_slice(&chunk[..size]);
                if let Some(end) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
                    let header = String::from_utf8_lossy(&bytes[..end]);
                    let length = header.lines().find_map(|line| line.to_ascii_lowercase().strip_prefix("content-length:").and_then(|value| value.trim().parse::<usize>().ok())).unwrap();
                    if bytes.len() >= end + 4 + length {
                        let body: Value = serde_json::from_slice(&bytes[end + 4..end + 4 + length]).unwrap();
                        seen.push(body.clone());
                        let response = if body.get("command").is_some() { json!({"exitCode":0,"stdout":"guest-ok\n","stderr":""}) }
                            else { json!({"entries":[],"readOnly":body["readOnly"]}) };
                        let encoded = serde_json::to_vec(&response).unwrap();
                        stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", encoded.len()).as_bytes()).await.unwrap();
                        stream.write_all(&encoded).await.unwrap();
                        break;
                    }
                }
            }
        }
        seen
    });
    let root = tempfile::tempdir().unwrap();
    let files = SharedFiles::default();
    let client = FolderClient::Agent { endpoint, token: "test-token".into(), runtime_id: "a".into(), exec_path: "/v1/containers/exec", micro_workload: false };
    let share = Arc::new(Share {
        environments: [environment("a", EnvironmentKind::Container), environment("b", EnvironmentKind::Container)],
        source: "a".into(), target: "b".into(), label: "test".into(), both: false,
        storage: true, commands: true, command_clients: [None, Some(client.clone())],
        source_server: HostFolderServer::start(root.path().into(), false).await.unwrap(),
        target_server: HostFolderServer::start(root.path().into(), true).await.unwrap(),
        selected: vec![SelectedAccess { owner_id: "a".into(), path: "/workspace".into(), client }],
        selected_mounts: Vec::new(),
    });
    files.0.lock().unwrap().insert("conn-checked".into(), share);
    let body = json!({"connectionId":"conn-checked","operation":"list","path":"_selected/0"});
    assert!(files.desktop_request("other", body.clone()).await.is_err());
    let listing = files.desktop_request("b", body).await.unwrap();
    assert_eq!(listing["readOnly"], true);
    let (status, result) = request_at(&files, "a", "/exec", json!({"connectionId":"conn-checked","command":"ls"})).await;
    assert_eq!(status, 200);
    assert_eq!(result["stdout"], "guest-ok\n");
    assert_eq!(request_at(&files, "b", "/exec", json!({"connectionId":"conn-checked","command":"rm -rf /tmp/test"})).await.0, 403);
    let (status, listing) = request(&files, "a", json!({"connectionId":"conn-checked","operation":"list","path":"_selected/0"})).await;
    assert_eq!(status, 200);
    assert_eq!(listing["readOnly"], false);
    let seen = tokio::time::timeout(std::time::Duration::from_secs(5), agent).await.unwrap().unwrap();
    assert_eq!(seen.len(), 3);
    assert_eq!(seen[0]["root"], "/workspace");
    assert_eq!(seen[0]["path"], "");
}
#[tokio::test]
async fn shared_file_permissions_work_for_all_sixteen_local_and_cloud_pairings() {
    for a in [
        EnvironmentKind::Container,
        EnvironmentKind::MicroVm,
        EnvironmentKind::FullVm,
        EnvironmentKind::Cloud,
    ] {
        for b in [
            EnvironmentKind::Container,
            EnvironmentKind::MicroVm,
            EnvironmentKind::FullVm,
            EnvironmentKind::Cloud,
        ] {
            let root = tempfile::tempdir().unwrap();
            let files = SharedFiles::default();
            for both in [false, true] {
                let share = Arc::new(Share {
                    environments: [environment("a", a.clone()), environment("b", b.clone())],
                    source: "a".into(),
                    target: "b".into(),
                    label: "test".into(),
                    both,
                    storage: true,
                    commands: false,
                    command_clients: [None, None],
                    source_server: HostFolderServer::start(root.path().into(), false)
                        .await
                        .unwrap(),
                    target_server: HostFolderServer::start(root.path().into(), !both)
                        .await
                        .unwrap(),
                    selected: Vec::new(),
                    selected_mounts: Vec::new(),
                });
                files.0.lock().unwrap().insert("conn-test".into(), share);
                let name = if both { "both.txt" } else { "one.txt" };
                assert_eq!(
                    request(
                        &files,
                        "a",
                        json!({"connectionId":"conn-test","operation":"create","path":name})
                    )
                    .await
                    .0,
                    200
                );
                assert_eq!(request(&files,"a",json!({"connectionId":"conn-test","operation":"write","path":name,"data":STANDARD.encode("hello")})).await.0,200);
                let read = request(
                    &files,
                    "b",
                    json!({"connectionId":"conn-test","operation":"read","path":name,"length":5}),
                )
                .await;
                assert_eq!(read.0, 200);
                assert_eq!(read.1["data"], STANDARD.encode("hello"));
                assert_eq!(request(&files,"b",json!({"connectionId":"conn-test","operation":"write","path":name,"data":STANDARD.encode("world")})).await.0,if both {200} else {403});
                assert_eq!(
                    request(
                        &files,
                        "other",
                        json!({"connectionId":"conn-test","operation":"list"})
                    )
                    .await
                    .0,
                    403
                );
                assert_eq!(request(&files,"a",json!({"connectionId":"conn-test","operation":"read","path":"../secret","length":1})).await.0,403);
                files.remove("conn-test");
                assert_eq!(
                    request(
                        &files,
                        "a",
                        json!({"connectionId":"conn-test","operation":"list"})
                    )
                    .await
                    .0,
                    403
                );
                assert!(
                    root.path().join(name).exists(),
                    "disconnect must preserve data"
                );
            }
        }
    }
}
#[tokio::test]
async fn shared_file_service_rejects_browser_cross_origin_and_rebinding() {
    let files = SharedFiles::default();
    for request in ["POST /api HTTP/1.1\r\nHost: 10.192.0.1:7444\r\n\r\n{}", "GET /connections HTTP/1.1\r\nHost: malicious.test\r\n\r\n", "POST /api HTTP/1.1\r\nHost: 10.192.0.1:7444\r\nOrigin: https://evil.test\r\nX-Yougori-Files: 1\r\n\r\n{}"] {
        assert!(files.http("a",request.as_bytes()).await.starts_with(b"HTTP/1.1 403"));
    }
    let response = String::from_utf8(
        files
            .http("a", b"GET / HTTP/1.1\r\nHost: 10.192.0.1:7444\r\n\r\n")
            .await,
    )
    .unwrap();
    assert!(response.starts_with("HTTP/1.1 200"));
    assert!(response.contains("frame-ancestors 'none'"));
    assert!(!response.contains("Access-Control-Allow-Origin"));
}
#[test]
fn inline_script_csp_hash_matches_html_newline_normalization() {
    let html = include_str!("../connection_files.html").replace("\r\n", "\n").replace('\r', "\n");
    let expected = html_script_hash(&html);
    assert_eq!(html_script_hash(&html.replace('\n', "\r\n")), expected);
    assert_eq!(html_script_hash(&html.replace('\n', "\r")), expected);
    assert_eq!(script_hash(), expected);
}
#[tokio::test]
async fn disconnect_closes_mount_servers_even_with_inflight_share_references() {
    let root = tempfile::tempdir().unwrap();
    let files = SharedFiles::default();
    let share = Arc::new(Share {
        environments: [
            environment("a", EnvironmentKind::Container),
            environment("b", EnvironmentKind::FullVm),
        ],
        source: "a".into(),
        target: "b".into(),
        label: "test".into(),
        both: true,
        storage: true,
        commands: false,
        command_clients: [None, None],
        source_server: HostFolderServer::start(root.path().into(), false)
            .await
            .unwrap(),
        target_server: HostFolderServer::start(root.path().into(), false)
            .await
            .unwrap(),
        selected: Vec::new(),
        selected_mounts: Vec::new(),
    });
    files
        .0
        .lock()
        .unwrap()
        .insert("conn-test".into(), share.clone());
    files.remove_environment("a");
    tokio::task::yield_now().await;
    assert!(reqwest::Client::new()
        .get(format!("http://127.0.0.1:{}/", share.source_server.port))
        .send()
        .await
        .is_err());
}
