use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    future::Future,
    path::{Component, Path, PathBuf},
    pin::Pin,
    sync::Arc,
    time::UNIX_EPOCH,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};

pub struct HostFolderServer {
    pub port: u16,
    pub token: String,
    task: JoinHandle<()>,
    relays: std::sync::Mutex<std::collections::HashMap<String, (String, JoinHandle<()>)>>,
}
pub type FileForward = Arc<dyn Fn(Value) -> Pin<Box<dyn Future<Output = Result<(u16, Vec<u8>), String>> + Send>> + Send + Sync>;
impl Drop for HostFolderServer {
    fn drop(&mut self) {
        self.task.abort();
        for (_, task) in self.relays.get_mut().unwrap().values() { task.abort(); }
    }
}
impl HostFolderServer {
    pub fn stop(&self) {
        self.task.abort();
        let mut relays=self.relays.lock().unwrap();
        for (_,task) in relays.values(){task.abort();}
        relays.clear();
    }
    pub fn relay_endpoint(&self, key: &str) -> Option<String> {
        self.relays.lock().unwrap().get(key).filter(|(_, task)| !task.is_finished()).map(|(url, _)|url.clone())
    }
    pub fn retain_relay(&self, key: String, endpoint: String, task: JoinHandle<()>) {
        if let Some((_, old))=self.relays.lock().unwrap().insert(key,(endpoint,task)){old.abort();}
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileRequest {
    operation: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    destination: String,
    #[serde(default)]
    offset: u64,
    #[serde(default)]
    length: u64,
    #[serde(default)]
    data: String,
    #[serde(default)]
    expected_data: Option<String>,
}

pub fn safe_path(root: &Path, relative: &str, create: bool) -> Result<PathBuf, String> {
    if relative.contains(['\\', ':', '\0'])
        || Path::new(relative)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("Invalid shared path".into());
    }
    let candidate = root.join(relative);
    if !create && !candidate.exists() {
        return Err("Not found".into());
    }
    let resolved = if create && !candidate.exists() {
        let parent = candidate
            .parent()
            .ok_or("Invalid shared path")?
            .canonicalize()
            .map_err(|e| e.to_string())?;
        if !parent.starts_with(root) {
            return Err("Path leaves the selected folder".into());
        }
        parent.join(candidate.file_name().ok_or("Invalid shared path")?)
    } else {
        candidate.canonicalize().map_err(|e| e.to_string())?
    };
    if !resolved.starts_with(root) {
        return Err("Path leaves the selected folder".into());
    }
    Ok(resolved)
}
fn info(path: &Path) -> Result<Value, String> {
    let m = std::fs::metadata(path).map_err(|e| e.to_string())?;
    Ok(
        json!({"name":path.file_name().unwrap_or_default().to_string_lossy(),"size":m.len(),"directory":m.is_dir(),"modified":m.modified().ok().and_then(|t|t.duration_since(UNIX_EPOCH).ok()).map(|d|d.as_secs()).unwrap_or_default()}),
    )
}
fn operation(root: &Path, read_only: bool, request: FileRequest) -> Result<Value, String> {
    let write = !matches!(request.operation.as_str(), "stat" | "list" | "read");
    if write && read_only {
        return Err("This folder is read-only".into());
    }
    if write && request.path.is_empty() {
        return Err("Cannot modify the share root".into());
    }
    let path = safe_path(
        root,
        &request.path,
        matches!(request.operation.as_str(), "create" | "mkdir"),
    )?;
    match request.operation.as_str() {
        "stat" => Ok(json!({"info":info(&path)?})),
        "list" => {
            let mut entries = Vec::new();
            for item in std::fs::read_dir(path)
                .map_err(|e| e.to_string())?
                .take(10_000)
            {
                let item = item.map_err(|e| e.to_string())?;
                if item
                    .path()
                    .canonicalize()
                    .is_ok_and(|p| p.starts_with(root))
                {
                    if let Ok(value) = info(&item.path()) {
                        entries.push(value);
                    }
                }
            }
            Ok(json!({"entries":entries}))
        }
        "read" => {
            use std::io::{Read, Seek};
            let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
            file.seek(std::io::SeekFrom::Start(request.offset))
                .map_err(|e| e.to_string())?;
            let mut bytes = vec![0; request.length.min(256 * 1024) as usize];
            let n = file.read(&mut bytes).map_err(|e| e.to_string())?;
            Ok(json!({"data":STANDARD.encode(&bytes[..n])}))
        }
        "replace" => {
            use std::io::Write;
            let bytes = STANDARD.decode(request.data).map_err(|e| e.to_string())?;
            let expected = STANDARD.decode(request.expected_data.ok_or("Reload this file before saving")?).map_err(|e| e.to_string())?;
            if bytes.len() > 256 * 1024 || expected.len() > 256 * 1024 { return Err("Text editing is limited to 256 KiB".into()); }
            let metadata = std::fs::metadata(&path).map_err(|e| e.to_string())?;
            if metadata.len() != expected.len() as u64 || std::fs::read(&path).map_err(|e|e.to_string())? != expected {
                return Err("The file changed since it was opened. Reload it before saving.".into());
            }
            let mut temporary = tempfile::NamedTempFile::new_in(path.parent().ok_or("Invalid file path")?).map_err(|e|e.to_string())?;
            temporary.write_all(&bytes).and_then(|_| temporary.as_file().sync_all()).map_err(|e|e.to_string())?;
            temporary.as_file().set_permissions(metadata.permissions()).map_err(|e|e.to_string())?;
            temporary.persist(&path).map_err(|e|e.to_string())?;
            Ok(json!({"count":bytes.len()}))
        }
        "write" => {
            use std::io::{Seek, Write};
            let bytes = STANDARD.decode(request.data).map_err(|e| e.to_string())?;
            if bytes.len() > 256 * 1024 {
                return Err("Write is too large".into());
            }
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .open(path)
                .map_err(|e| e.to_string())?;
            file.seek(std::io::SeekFrom::Start(request.offset))
                .map_err(|e| e.to_string())?;
            file.write_all(&bytes).map_err(|e| e.to_string())?;
            Ok(json!({"count":bytes.len()}))
        }
        "create" => {
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|e| e.to_string())?;
            Ok(json!({"info":info(&path)?}))
        }
        "mkdir" => {
            std::fs::create_dir(&path).map_err(|e| e.to_string())?;
            Ok(json!({"info":info(&path)?}))
        }
        "truncate" => {
            std::fs::OpenOptions::new()
                .write(true)
                .open(path)
                .and_then(|f| f.set_len(request.length))
                .map_err(|e| e.to_string())?;
            Ok(json!({}))
        }
        "remove" => {
            if path.is_dir() {
                std::fs::remove_dir(path)
            } else {
                std::fs::remove_file(path)
            }
            .map_err(|e| e.to_string())?;
            Ok(json!({}))
        }
        "rename" => {
            let destination = safe_path(root, &request.destination, true)?;
            if destination.exists() {
                return Err("Destination already exists".into());
            }
            std::fs::rename(path, destination).map_err(|e| e.to_string())?;
            Ok(json!({}))
        }
        _ => Err("Unsupported file operation".into()),
    }
}

pub async fn read_http<S: tokio::io::AsyncRead + Unpin>(stream: &mut S) -> Result<(String, Vec<u8>), String> {
    let mut header = Vec::new();
    loop {
        let b = stream.read_u8().await.map_err(|e| e.to_string())?;
        header.push(b);
        if header.len() > 16 * 1024 {
            return Err("Header is too large".into());
        }
        if header.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    let header = String::from_utf8(header).map_err(|e| e.to_string())?;
    let length = http_body_length(&header)?;
    let mut body = vec![0; length];
    stream
        .read_exact(&mut body)
        .await
        .map_err(|e| e.to_string())?;
    Ok((header, body))
}

fn http_body_length(header: &str) -> Result<usize, String> {
    let mut lines = header.split("\r\n");
    let request: Vec<_> = lines.next().unwrap_or_default().split(' ').collect();
    if request.len() != 3
        || request[0].is_empty()
        || !request[0].bytes().all(|b| b.is_ascii_uppercase())
        || !request[1].starts_with('/')
        || request[1].bytes().any(|b| b.is_ascii_control())
        || !matches!(request[2], "HTTP/1.0" | "HTTP/1.1")
    {
        return Err("Invalid HTTP request line".into());
    }
    let mut length = None;
    let mut sensitive = std::collections::HashSet::new();
    for line in lines.take_while(|line| !line.is_empty()) {
        let (name, value) = line.split_once(':').ok_or("Invalid HTTP header")?;
        if name.is_empty()
            || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
            || value.bytes().any(|b| b.is_ascii_control() && b != b'\t')
        {
            return Err("Invalid HTTP header".into());
        }
        let name = name.to_ascii_lowercase();
        if name == "transfer-encoding" {
            return Err("Transfer-Encoding is not supported".into());
        }
        if matches!(name.as_str(), "content-length" | "host" | "authorization")
            && !sensitive.insert(name.clone())
        {
            return Err("Duplicate HTTP framing or authentication header".into());
        }
        if name == "content-length" {
            let value = value.trim();
            if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                return Err("Invalid Content-Length".into());
            }
            length = Some(value.parse::<usize>().map_err(|_| "Invalid Content-Length")?);
        }
    }
    let length = length.unwrap_or(0);
    if length > 1024 * 1024 {
        return Err("Request is too large".into());
    }
    Ok(length)
}
async fn respond(stream: &mut TcpStream, status: u16, kind: &str, body: Vec<u8>) {
    use sha2::Digest;
    let page = include_str!("host_files_browser.html");
    let script = page.split_once("<script>").unwrap().1.split_once("</script>").unwrap().0;
    let script = script.replace("\r\n", "\n").replace('\r', "\n");
    let hash = STANDARD.encode(sha2::Sha256::digest(script.as_bytes()));
    let header=format!("HTTP/1.1 {status} Response\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nX-Content-Type-Options: nosniff\r\nContent-Security-Policy: default-src 'none'; script-src 'sha256-{hash}'; style-src 'unsafe-inline'; connect-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'\r\nDAV: 1\r\nAllow: OPTIONS, GET, HEAD, PROPFIND\r\n\r\n",body.len());
    let _ = stream.write_all(header.as_bytes()).await;
    let _ = stream.write_all(&body).await;
}
fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
fn encode_path(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes())
        .collect::<String>()
        .replace('+', "%20")
}

impl HostFolderServer {
    pub async fn start_forward(forward: FileForward) -> Result<Self, String> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.map_err(|e| e.to_string())?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let token = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
        let secret = token.clone();
        let task = tokio::spawn(async move {
            let mut clients = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    result = listener.accept() => {
                        let Ok((mut socket, _)) = result else { break };
                        if clients.len() >= 64 { continue }
                        let secret = secret.clone();
                        let forward = forward.clone();
                        clients.spawn(async move {
                            let Ok(Ok((header, body))) = tokio::time::timeout(std::time::Duration::from_secs(15), read_http(&mut socket)).await else { return };
                            if !header.starts_with("POST /files HTTP/1.") || !header.lines().any(|line| line.split_once(':').is_some_and(|(key, value)| key.eq_ignore_ascii_case("authorization") && value.trim() == format!("Bearer {secret}"))) {
                                respond(&mut socket, 403, "application/json", b"{\"error\":\"Forbidden\"}".to_vec()).await;
                                return;
                            }
                            let result = match serde_json::from_slice::<Value>(&body) {
                                Ok(request) => forward(request).await,
                                Err(_) => Err("Invalid file request".into()),
                            };
                            let (status, reply) = match result {
                                Ok((status, reply)) => (status, reply),
                                Err(error) => (403, serde_json::to_vec(&json!({"error": error})).unwrap_or_default()),
                            };
                            respond(&mut socket, status, "application/json", reply).await;
                        });
                    }
                    Some(_) = clients.join_next() => {}
                }
            }
        });
        Ok(Self { port, token, task, relays: std::sync::Mutex::new(std::collections::HashMap::new()) })
    }
    pub async fn start(root: PathBuf, read_only: bool) -> Result<Self, String> {
        let root = Arc::new(root.canonicalize().map_err(|e| e.to_string())?);
        if !root.is_dir() {
            return Err("Choose a folder".into());
        }
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .map_err(|e| e.to_string())?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let token = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let secret = token.clone();
        let operation_lock = Arc::new(std::sync::Mutex::new(()));
        let task = tokio::spawn(async move {
            let mut clients = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    result=listener.accept()=>{let Ok((mut socket,_))=result else{break};let root=root.clone();let secret=secret.clone();let operation_lock=operation_lock.clone();if clients.len()>=64{continue}clients.spawn(async move{let Ok(Ok((header,body)))=tokio::time::timeout(std::time::Duration::from_secs(15),read_http(&mut socket)).await else{return};
                        let mut first=header.lines().next().unwrap_or("").split_whitespace();let method=first.next().unwrap_or("");let uri=first.next().unwrap_or("");
                        if method=="POST" && uri=="/files"{
                            if !header.lines().any(|line|line.split_once(':').is_some_and(|(k,v)|k.eq_ignore_ascii_case("authorization")&&v.trim()==format!("Bearer {secret}"))){respond(&mut socket,403,"text/plain",b"Forbidden".to_vec()).await;return}
                            let result=match serde_json::from_slice::<FileRequest>(&body){Ok(request)=>tokio::task::spawn_blocking(move||{let _guard=operation_lock.lock().map_err(|_|"File service unavailable")?;operation(&root,read_only,request)}).await.unwrap_or_else(|e|Err(e.to_string())),Err(e)=>Err(e.to_string())};
                            let (status,reply)=match result{Ok(value)=>(200,value),Err(error)=>(if error=="Not found"{404}else if error.contains("already exists"){409}else{403},json!({"error":error}))};respond(&mut socket,status,"application/json",serde_json::to_vec(&reply).unwrap_or_default()).await;return
                        }
                        // A token-scoped read-only WebDAV/download view also works in guests without FUSE.
                        let prefix=format!("/{secret}/");let Some(relative)=uri.strip_prefix(&prefix) else{respond(&mut socket,403,"text/plain",b"Forbidden".to_vec()).await;return};
                        if method=="GET" && relative.is_empty(){respond(&mut socket,200,"text/html; charset=utf-8",include_str!("host_files_browser.html").replace("__READ_ONLY__",if read_only{"true"}else{"false"}).into_bytes()).await;return}
                        let encoded=format!("path={}",relative.replace('+',"%2B"));let relative=url::form_urlencoded::parse(encoded.as_bytes()).next().map(|(_,v)|v.into_owned()).unwrap_or_default();let relative=relative.trim_end_matches('/');let Ok(path)=safe_path(&root,relative,false) else{respond(&mut socket,404,"text/plain",b"Not found".to_vec()).await;return};
                        if method=="OPTIONS"{respond(&mut socket,200,"text/plain",Vec::new()).await;return}
                        if method=="PROPFIND"{
                            let mut paths=vec![path.clone()];if path.is_dir() && !header.lines().any(|line|line.eq_ignore_ascii_case("Depth: 0")){if let Ok(entries)=std::fs::read_dir(&path){paths.extend(entries.take(10000).filter_map(Result::ok).map(|e|e.path()).filter(|p|p.canonicalize().is_ok_and(|p|p.starts_with(root.as_path()))));}}
                            let mut xml=String::from("<?xml version=\"1.0\"?><D:multistatus xmlns:D=\"DAV:\">");for path in paths{if let Ok(m)=std::fs::metadata(&path){let suffix=path.strip_prefix(root.as_path()).unwrap_or(Path::new("")).iter().map(|p|encode_path(&p.to_string_lossy())).collect::<Vec<_>>().join("/");xml.push_str(&format!("<D:response><D:href>{}{}</D:href><D:propstat><D:prop><D:displayname>{}</D:displayname><D:resourcetype>{}</D:resourcetype><D:getcontentlength>{}</D:getcontentlength></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response>",prefix,escape(&suffix),escape(&path.file_name().unwrap_or_default().to_string_lossy()),if m.is_dir(){"<D:collection/>"}else{""},m.len()));}}xml.push_str("</D:multistatus>");respond(&mut socket,207,"application/xml",xml.into_bytes()).await;return
                        }
                        if !matches!(method,"GET"|"HEAD"){respond(&mut socket,405,"text/plain",b"Use the mounted share for writes".to_vec()).await;return}
                        if path.is_dir(){let mut html=String::from("<!doctype html><meta charset=utf-8><title>Yougori shared folder</title><h1>Shared folder</h1><ul>");if let Ok(entries)=std::fs::read_dir(path){for entry in entries.take(10000).filter_map(Result::ok){if entry.path().canonicalize().is_ok_and(|p|p.starts_with(root.as_path())){let name=entry.file_name().to_string_lossy().into_owned();let href=format!("{}{}/{}",prefix,relative.split('/').filter(|p|!p.is_empty()).map(encode_path).collect::<Vec<_>>().join("/"),encode_path(&name));html.push_str(&format!("<li><a href=\"{}\">{}</a></li>",escape(&href.replace("//","/")),escape(&name)));}}}html.push_str("</ul>");respond(&mut socket,200,"text/html; charset=utf-8",html.into_bytes()).await;
                        }else if let Ok(mut file)=tokio::fs::File::open(path).await{let length=file.metadata().await.map(|m|m.len()).unwrap_or(0);let header=format!("HTTP/1.1 200 OK\r\nContent-Length: {length}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n");let _=socket.write_all(header.as_bytes()).await;if method!="HEAD"{let _=tokio::io::copy(&mut file,&mut socket).await;}}
                    });},
                    _=clients.join_next(),if !clients.is_empty()=>{}
                }
            }
        });
        Ok(Self { port, token, task, relays: Default::default() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn http_framing_rejects_ambiguous_and_malformed_requests() {
        assert_eq!(http_body_length("GET / HTTP/1.1\r\nHost: localhost\r\n\r\n"), Ok(0));
        assert_eq!(http_body_length("POST /files HTTP/1.1\r\nContent-Length: 12\r\n\r\n"), Ok(12));
        for header in [
            "Content-Length: nope", "Content-Length: -1", "Content-Length: +1",
            "Content-Length: 1, 1", "Content-Length: 1048577",
            "Content-Length: 1\r\ncontent-length: 1",
            "Transfer-Encoding: chunked", "Content-Length : 1",
            "Host: localhost\r\nhost: different", "Authorization: no\r\nAuthorization: yes",
            "Folded: value\r\n continuation", "Header: value\nInjected: yes",
        ] {
            assert!(http_body_length(&format!("POST /files HTTP/1.1\r\n{header}\r\n\r\n")).is_err(), "{header}");
        }
    }
    #[test]
    fn folder_paths_cannot_escape() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path().canonicalize().unwrap();
        assert!(safe_path(&root, "../secret", false).is_err());
        assert!(safe_path(&root, "C:/secret", false).is_err());
        assert!(safe_path(&root, "/etc/passwd", false).is_err());
        assert!(safe_path(&root, "safe.txt", true)
            .unwrap()
            .starts_with(&root));
    }
    #[test]
    fn readonly_rejects_mutations() {
        let root = tempfile::tempdir().unwrap();
        let request: FileRequest =
            serde_json::from_value(json!({"operation":"create","path":"test"})).unwrap();
        assert!(operation(root.path(), true, request)
            .unwrap_err()
            .contains("read-only"));
    }
    #[tokio::test]
    async fn browser_editor_checks_permissions_conflicts_and_revocation() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("code.txt"), "before").unwrap();
        let client = reqwest::Client::new();
        for read_only in [true, false] {
            let server = HostFolderServer::start(directory.path().into(), read_only).await.unwrap();
            let base = format!("http://127.0.0.1:{}", server.port);
            let page = client.get(format!("{base}/{}/", server.token)).send().await.unwrap();
            assert!(page.headers()["content-security-policy"].to_str().unwrap().contains("script-src 'sha256-"));
            assert!(page.text().await.unwrap().contains(&format!("data-read-only=\"{read_only}\"")));
            let replace = json!({"operation":"replace","path":"code.txt","data":STANDARD.encode("after"),"expectedData":STANDARD.encode("before")});
            let result = client.post(format!("{base}/files")).bearer_auth(&server.token).json(&replace).send().await.unwrap();
            assert_eq!(result.status().as_u16(), if read_only {403} else {200});
            if !read_only {
                assert_eq!(std::fs::read_to_string(directory.path().join("code.txt")).unwrap(), "after");
                let conflict = client.post(format!("{base}/files")).bearer_auth(&server.token).json(&replace).send().await.unwrap();
                assert!(!conflict.status().is_success());
                assert!(conflict.text().await.unwrap().contains("changed since"));
            }
            server.stop();
            tokio::task::yield_now().await;
            assert!(client.post(format!("{base}/files")).bearer_auth(&server.token).json(&replace).send().await.is_err());
        }
    }
    #[tokio::test]
    async fn host_folder_http_is_authenticated_scoped_and_revocable() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("hello & world.txt"), "hello").unwrap();
        let server = HostFolderServer::start(directory.path().into(), true)
            .await
            .unwrap();
        let base = format!("http://127.0.0.1:{}", server.port);
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .unwrap();
        assert_eq!(
            client
                .post(format!("{base}/files"))
                .json(&json!({"operation":"list"}))
                .send()
                .await
                .unwrap()
                .status(),
            403
        );
        assert_eq!(
            client
                .get(format!("{base}/wrong-token/hello.txt"))
                .send()
                .await
                .unwrap()
                .status(),
            403
        );
        let list = client
            .post(format!("{base}/files"))
            .bearer_auth(&server.token)
            .json(&json!({"operation":"list"}))
            .send()
            .await
            .unwrap()
            .json::<Value>()
            .await
            .unwrap();
        assert_eq!(list["entries"][0]["name"], "hello & world.txt");
        for path in [
            "../private",
            "sub/../../private",
            "C:/private",
            "hello.txt:secret",
            "\\\\server\\share",
        ] {
            assert!(!client
                .post(format!("{base}/files"))
                .bearer_auth(&server.token)
                .json(&json!({"operation":"read","path":path,"length":10}))
                .send()
                .await
                .unwrap()
                .status()
                .is_success());
        }
        assert_eq!(
            client
                .post(format!("{base}/files"))
                .bearer_auth(&server.token)
                .json(&json!({"operation":"create","path":"new.txt"}))
                .send()
                .await
                .unwrap()
                .status(),
            403
        );
        let file_url = format!("{base}/{}/hello%20%26%20world.txt", server.token);
        assert_eq!(
            client
                .get(&file_url)
                .send()
                .await
                .unwrap()
                .text()
                .await
                .unwrap(),
            "hello"
        );
        let listing = client
            .request(
                reqwest::Method::from_bytes(b"PROPFIND").unwrap(),
                format!("{base}/{}/", server.token),
            )
            .header("Depth", "1")
            .send()
            .await
            .unwrap();
        assert_eq!(listing.status(), 207);
        assert!(listing
            .text()
            .await
            .unwrap()
            .contains("hello%20%26%20world.txt"));
        drop(server);
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        assert!(client.get(&file_url).send().await.is_err());
    }
}
