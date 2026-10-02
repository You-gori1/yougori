use super::*;
use serde_json::json;
use std::sync::atomic::AtomicBool;
struct Fixture {
    confirmed: AtomicU64,
    cancel: AtomicBool,
    complete: bool,
}
async fn fixture(complete: bool) -> (String, Arc<Fixture>, tokio::task::JoinHandle<()>) {
    let state = Arc::new(Fixture {
        confirmed: AtomicU64::new(0),
        cancel: AtomicBool::new(false),
        complete,
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let shared = state.clone();
    let server = tokio::spawn(async move {
        let mut tasks = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                accepted=listener.accept()=>{let (mut socket,_)=accepted.unwrap();let state=shared.clone();tasks.spawn(async move {
                    let mut header=Vec::new();loop {let byte=socket.read_u8().await.ok()?;header.push(byte);if header.ends_with(b"\r\n\r\n"){break;}if header.len()>16384{return None;}}
                    let header=String::from_utf8(header).ok()?;let path=header.lines().next()?.split_whitespace().nth(1)?;
                    let length=header.lines().find_map(|line|line.to_lowercase().strip_prefix("content-length:").map(|v|v.trim().parse::<usize>().unwrap())).unwrap_or(0);
                    let mut body=vec![0;length];socket.read_exact(&mut body).await.ok()?;
                    let value=if path.starts_with("/v1/files/import/progress") {json!({"phase":"extracting","confirmedBytes":state.confirmed.load(Ordering::Relaxed)})}
                    else if path.starts_with("/v1/files/import/cancel") {state.cancel.store(true,Ordering::Relaxed);json!({"cancelRequested":true})}
                    else if state.complete {for i in 1..=8 {tokio::time::sleep(Duration::from_millis(35)).await;state.confirmed.store(i,Ordering::Relaxed);}json!({"destination":"/yougori-import-abc","bytes":8})}
                    else {let _=socket.read_u8().await;return None;};
                    let body=value.to_string();socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).as_bytes()).await.ok()?;Some(())
                });},
                _=tasks.join_next(),if !tasks.is_empty()=>{}
            }
        }
    });
    (base, state, server)
}
fn limits() -> Limits {
    Limits {
        connect: Duration::from_secs(1),
        inactivity: Duration::from_millis(100),
        total: Duration::from_secs(2),
        poll: Duration::from_millis(10),
    }
}
#[tokio::test]
async fn small_stalled_copy_fails_promptly_and_cancels_guest_without_touching_source() {
    let archive = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(archive.path(), b"original").unwrap();
    let (base, state, server) = fixture(false).await;
    let start = Instant::now();
    let result = transfer_archive(
        &base,
        "secret",
        "env-a",
        archive.path(),
        "abc",
        8,
        None,
        Arc::new(|_| {}),
        CancellationToken::new(),
        limits(),
    )
    .await;
    assert!(result.unwrap_err().starts_with("YOUGORI_TRANSFER_INACTIVE"));
    assert!(start.elapsed() < Duration::from_secs(2));
    assert!(state.cancel.load(Ordering::Relaxed));
    assert_eq!(std::fs::read(archive.path()).unwrap(), b"original");
    server.abort();
}
#[tokio::test]
async fn cancellation_closes_a_stalled_request_and_releases_the_reader() {
    let archive = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(archive.path(), b"original").unwrap();
    let (base, state, server) = fixture(false).await;
    let token = CancellationToken::new();
    let trigger = token.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        trigger.cancel();
    });
    let result = transfer_archive(
        &base,
        "secret",
        "env-a",
        archive.path(),
        "abc",
        8,
        None,
        Arc::new(|_| {}),
        token,
        limits(),
    )
    .await;
    assert!(result
        .unwrap_err()
        .starts_with("YOUGORI_OPERATION_CANCELLED"));
    assert!(state.cancel.load(Ordering::Relaxed));
    server.abort();
}
#[tokio::test]
async fn confirmed_guest_progress_keeps_slow_extraction_alive() {
    let archive = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(archive.path(), b"original").unwrap();
    let (base, state, server) = fixture(true).await;
    let result = transfer_archive(
        &base,
        "secret",
        "env-a",
        archive.path(),
        "abc",
        8,
        None,
        Arc::new(|_| {}),
        CancellationToken::new(),
        limits(),
    )
    .await
    .unwrap();
    assert_eq!(result, "/yougori-import-abc");
    assert!(!state.cancel.load(Ordering::Relaxed));
    server.abort();
}
