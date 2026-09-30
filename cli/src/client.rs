use crate::wire::{self, Request, Response};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

/// Interactive screens show their own progress; notes on stderr would break their layout.
pub static QUIET: AtomicBool = AtomicBool::new(false);

pub async fn call_at(endpoint: &str, request: &Request) -> Result<Value, String> {
    #[cfg(windows)]
    let mut stream = {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match tokio::net::windows::named_pipe::ClientOptions::new().open(endpoint) {
                Ok(stream)=>break stream,
                Err(error) if error.raw_os_error()==Some(231) && Instant::now()<deadline => tokio::time::sleep(Duration::from_millis(50)).await,
                Err(error)=>return Err(format!("Cannot reach the Yougori engine: {error}. Start/update Yougori, or run yougori app start.")),
            }
        }
    };
    #[cfg(unix)]
    let mut stream = tokio::net::UnixStream::connect(endpoint)
        .await
        .map_err(|error| {
            format!("Cannot reach the Yougori engine: {error}. Run yougori app start.")
        })?;
    #[cfg(windows)]
    wire::verify_pipe_server(&stream).map_err(|error| {
        format!("Cannot verify Yougori engine ownership: {error}. No request was sent.")
    })?;
    #[cfg(unix)]
    if stream
        .peer_cred()
        .map_err(|error| format!("Cannot verify Yougori engine ownership: {error}"))?
        .uid()
        != unsafe { libc::geteuid() }
    {
        return Err(
            "Yougori control endpoint belongs to another user. No request was sent.".into(),
        );
    }
    let bytes = serde_json::to_vec(request).map_err(|e| e.to_string())?;
    let reply=tokio::time::timeout(request_timeout(request), async {
        wire::write_frame(&mut stream,&bytes,wire::MAX_REQUEST).await?;
        wire::read_frame(&mut stream,wire::MAX_RESPONSE).await
    }).await.map_err(|_|"Yougori control request timed out. The outcome may be unknown; inspect jobs and state before repeating a mutation.")?.map_err(|e|format!("Yougori connection ended: {e}. Inspect jobs/state before repeating a mutation."))?;
    let response: Response =
        serde_json::from_slice(&reply).map_err(|_| "Invalid Yougori control response")?;
    if response.version != wire::VERSION {
        return Err("CLI/engine protocol mismatch. Update both Yougori and its CLI.".into());
    }
    if response.ok {
        Ok(response.result.unwrap_or(Value::Null))
    } else {
        Err(engine_error(&request.method, response
            .error
            .unwrap_or_else(|| "Yougori operation failed".into())))
    }
}

fn engine_error(method: &str, error: String) -> String {
    if error.starts_with(&format!("Unknown method '{method}'.")) && crate::catalog::find(method).is_ok() {
        return format!("The running Yougori engine does not support {method}, but this CLI does. Restart Yougori to load the updated engine: `yougori app quit --yes`, then `yougori app start`. Quitting stops running environments and disconnects sessions. If this continues after restarting, update both the CLI and engine.");
    }
    error
}
fn request_timeout(request: &Request) -> Duration {
    // Terminal calls can include guest startup plus a 25-second guest request.
    // Keep their I/O independent of the mutation queue without timing out mid-write.
    match request.method.as_str() {
        "terminal_action" => Duration::from_secs(180),
        "list_environment_services"
        | "model_status"
        | "model_api_status"
        | "get_storage_allocation"
        | "vault_summary" => Duration::from_secs(180),
        "jobs_get" => {
            Duration::from_millis(20_000 + request.params["wait"].as_u64().unwrap_or(0).min(30_000))
        }
        _ => Duration::from_secs(20),
    }
}
pub async fn call(request: &Request) -> Result<Value, String> {
    call_at(&wire::endpoint().map_err(|e| e.to_string())?, request).await
}
pub fn request(method: &str, params: Value) -> Request {
    Request {
        version: wire::VERSION,
        method: method.into(),
        params,
        confirmed: false,
        dry_run: false,
    }
}

pub async fn wait_job_at(endpoint: &str, id: &str, seconds: u64) -> Result<Value, String> {
    wait_job_observed(endpoint, id, seconds, 5000, |_| {}).await
}

/// Observe measured job progress without changing the job result or timeout semantics.
/// Engines without measurements still work, with an indeterminate display.
pub async fn wait_job_with_progress(id: &str, seconds: u64, progress: impl FnMut(&Value)) -> Result<Value, String> {
    wait_job_observed(&wire::endpoint().map_err(|e| e.to_string())?, id, seconds, 250, progress).await
}

async fn wait_job_observed(endpoint: &str, id: &str, seconds: u64, wait: u64, mut progress: impl FnMut(&Value)) -> Result<Value, String> {
    let started = Instant::now();
    let mut last_progress = Instant::now();
    // The engine long-polls, answering the moment the job ends. An older engine rejects
    // `wait`; then poll quickly at first and back off, instead of a fixed half second.
    let mut long_poll = true;
    let mut pause = Duration::from_millis(10);
    loop {
        let job = if long_poll {
            match call_at(
                endpoint,
                &request("jobs_get", json!({"jobId":id,"wait":wait})),
            )
            .await
            {
                Err(error) if error.contains("Unknown parameter 'wait'") => {
                    long_poll = false;
                    continue;
                }
                other => other,
            }
        } else {
            call_at(endpoint, &request("jobs_get", json!({"jobId":id}))).await
        }
        .map_err(|e| format!("{e} Accepted job: {id}."))?;
        if let Some(measurement) = job.get("progress").filter(|v| !v.is_null()) {
            progress(measurement);
        }
        match job["status"].as_str() {
            Some("complete") => return Ok(job["result"].clone()),
            Some("failed") => {
                return Err(format!(
                    "{}: {} (job {id})",
                    job["method"].as_str().unwrap_or("Operation"),
                    job["error"].as_str().unwrap_or("Operation failed")
                ))
            }
            Some("running" | "queued") => {}
            _ => return Err("Invalid job status from Yougori".into()),
        }
        if started.elapsed() >= Duration::from_secs(seconds) {
            return Err(format!("Still running: {id}. The operation was NOT cancelled. Check 'yougori jobs get {id}' before retrying."));
        }
        if last_progress.elapsed() >= Duration::from_secs(5) && !QUIET.load(Ordering::Relaxed) {
            eprintln!(
                "Yougori: {} — {}s ({id})",
                job["method"].as_str().unwrap_or("working"),
                started.elapsed().as_secs()
            );
            last_progress = Instant::now();
        }
        if !long_poll {
            tokio::time::sleep(pause).await;
            pause = (pause * 2).min(Duration::from_millis(250));
        }
    }
}
pub async fn wait_job(id: &str, seconds: u64) -> Result<Value, String> {
    wait_job_at(&wire::endpoint().map_err(|e| e.to_string())?, id, seconds).await
}

/// The installed Yougori app executable (or the development build in a checkout).
pub fn desktop_executable(explicit: Option<&str>) -> Result<PathBuf, String> {
    desktop_path(explicit)
}
fn desktop_path(explicit: Option<&str>) -> Result<PathBuf, String> {
    if let Some(path) = explicit
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("YOUGORI_APP").map(PathBuf::from))
        .or_else(|| std::env::var_os("OPENDOCK_APP").map(PathBuf::from))
    {
        if path.is_absolute() && path.is_file() {
            return Ok(path);
        }
        return Err("The Yougori app path must be an existing absolute executable path".into());
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let name = if cfg!(windows) {
        "yougori.exe"
    } else {
        "yougori"
    };
    let parent = exe.parent().ok_or("Cannot locate the CLI directory")?;
    let mut candidates = Vec::new();
    #[cfg(target_os = "linux")]
    candidates.push(PathBuf::from("/usr/bin/yougori-desktop"));
    #[cfg(target_os = "macos")]
    {
        candidates.extend(macos_app_candidates(&exe));
        if let Some(home) = std::env::var_os("HOME") {
            candidates
                .push(PathBuf::from(home).join("Applications/Yougori.app/Contents/MacOS/yougori"));
        }
        candidates.push(PathBuf::from(
            "/Applications/Yougori.app/Contents/MacOS/yougori",
        ));
    }
    if let Some(parent) = parent.parent() {
        candidates.push(parent.join(name));
    }
    candidates.extend([
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../src-tauri/target/debug")
            .join(name),
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../src-tauri/target/release")
            .join(name),
    ]);
    candidates.into_iter().find(|path|path.is_file() && path.canonicalize().ok() != exe.canonicalize().ok()).ok_or_else(||"Cannot find the desktop engine. Use 'app start --app ABSOLUTE_PATH_TO_YOUGORI' or start 'npm run desktop:dev' in the source checkout.".into())
}

#[cfg(any(target_os = "macos", test))]
fn macos_app_candidates(cli: &Path) -> Vec<PathBuf> {
    // Resolve from Resources/cli in a moved or renamed .app, without assuming
    // it was installed in /Applications or requiring a global PATH entry.
    cli.ancestors()
        .filter(|p| p.file_name().is_some_and(|name| name == "Contents"))
        .map(|contents| contents.join("MacOS/yougori"))
        .collect()
}

/// The standalone engine (`yougori-engine`, no desktop app), placed beside the CLI's folder by
/// the engine-only installers, or YOUGORI_ENGINE.
pub fn engine_executable() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("YOUGORI_ENGINE").map(PathBuf::from) {
        return (path.is_absolute() && path.is_file()).then_some(path);
    }
    let name = if cfg!(windows) {
        "yougori-engine.exe"
    } else {
        "yougori-engine"
    };
    // Resolve ~/.local/bin/yougori symlinks to the install folder (macOS reports the link path).
    let exe = std::env::current_exe().ok()?;
    let exe = exe.canonicalize().unwrap_or(exe);
    let mut candidates: Vec<PathBuf> = exe
        .ancestors()
        .skip(1)
        .take(2)
        .map(|folder| folder.join(name))
        .collect();
    candidates.extend(["debug", "release"].map(|profile| {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../engine/target")
            .join(profile)
            .join(name)
    }));
    candidates.into_iter().find(|path| path.is_file())
}

fn launch(executable: &Path, args: &[&str]) -> Result<std::process::Child, String> {
    let mut command = std::process::Command::new(executable);
    command
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
        // Windows passes every inheritable handle to the child. A shell pipe on this process's
        // stdout would then stay open for the engine's lifetime, so `yougori app start | cat`
        // would never finish. Stdio::inherit elsewhere duplicates its own inheritable copies.
        use windows_sys::Win32::{
            Foundation::{SetHandleInformation, HANDLE_FLAG_INHERIT},
            System::Console::{
                GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
            },
        };
        for id in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            // SAFETY: only clears the inherit flag of this process's own standard handles.
            unsafe {
                let handle = GetStdHandle(id);
                if !handle.is_null() && handle as isize != -1 {
                    SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0);
                }
            }
        }
    }
    command
        .spawn()
        .map_err(|e| format!("Cannot start Yougori: {e}"))
}

async fn wait_for_engine(mut child: std::process::Child) -> Result<Value, String> {
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        if let Ok(status) = call(&request("app_status", json!({}))).await {
            return Ok(status);
        }
        if let Some(exit) = child.try_wait().map_err(|e| e.to_string())? {
            return Err(format!("Yougori exited during startup ({exit}). If an older desktop version is already open, close it normally and restart the updated build."));
        }
        if Instant::now() >= deadline {
            return Err("Yougori has not exposed its local control endpoint yet. The process was left running; check the desktop before starting another instance.".into());
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

const NOT_INSTALLED: &str = "Cannot find Yougori on this computer. Install it with `irm https://yougori.com/install.ps1 | iex` (Windows) or `curl -fsSL https://yougori.com/install.sh | sh`, or pass --app ABSOLUTE_PATH.";

/// Starts the background engine unless one already runs. The desktop app (in its headless mode)
/// is preferred so its dashboard can open later; the standalone engine is used when the app is
/// not installed, or first with `prefer_engine` / YOUGORI_ENGINE_ONLY=1.
pub async fn start(explicit: Option<&str>) -> Result<Value, String> {
    start_with(
        explicit,
        std::env::var("YOUGORI_ENGINE_ONLY").is_ok_and(|v| v == "1"),
    )
    .await
}

pub async fn start_with(explicit: Option<&str>, prefer_engine: bool) -> Result<Value, String> {
    if let Ok(status) = call(&request("app_status", json!({}))).await {
        return Ok(status);
    }
    let executable = if explicit.is_some() {
        desktop_path(explicit)?
    } else if prefer_engine {
        engine_executable().ok_or("The standalone engine (yougori-engine) is not installed. Reinstall with YOUGORI_ENGINE_ONLY=1, or set YOUGORI_ENGINE.")?
    } else {
        desktop_path(None)
            .ok()
            .or_else(engine_executable)
            .ok_or(NOT_INSTALLED)?
    };
    wait_for_engine(launch(&executable, &["--headless"])?).await
}

/// Opens the dashboard. A standalone engine has none: with nothing running it steps aside and
/// the desktop app starts in its place, keeping every environment and setting.
pub async fn show() -> Result<Value, String> {
    let mut show = request("app_show", json!({}));
    show.confirmed = true;
    let desktop = desktop_path(None).ok();
    let no_app = "This computer runs the Yougori engine without the desktop app, so there is no dashboard. Install the app for one, or keep using the CLI.";
    match call(&request("app_status", json!({}))).await {
        Ok(status) if status["engineOnly"] != true => call(&show).await,
        Ok(_) => {
            let desktop = desktop.ok_or(no_app)?;
            let result = call(&show).await?;
            if result["handover"] != true {
                return Ok(result);
            }
            let deadline = Instant::now() + Duration::from_secs(90);
            while call(&request("app_status", json!({}))).await.is_ok() {
                if Instant::now() >= deadline {
                    return Err("The background engine did not stop in time; run `yougori app quit`, then `yougori app show`.".into());
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            wait_for_engine(launch(&desktop, &[])?).await?;
            Ok(json!({"visible": true, "handover": true}))
        }
        Err(_) => {
            wait_for_engine(launch(&desktop.ok_or(no_app)?, &[])?).await?;
            Ok(json!({"visible": true}))
        }
    }
}

/// Requests a graceful shutdown and waits until the control endpoint is gone.
/// Returning on the acknowledgement alone lets an immediate desktop launch
/// attach to the process that is still shutting down.
/// Only called after the command line was confirmed with --yes.
pub async fn quit() -> Result<Value, String> {
    let mut quit = request("app_quit", json!({}));
    quit.confirmed = true;
    let result = call(&quit).await?;
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        tokio::time::sleep(Duration::from_millis(500)).await;
        if call(&request("app_status", json!({}))).await.is_err() {
            return Ok(result);
        }
        if Instant::now() >= deadline {
            return Err("Yougori accepted the shutdown request but is still running. Check the desktop before starting another instance.".into());
        }
    }
}

#[cfg(test)]
mod macos_path_tests {
    use super::*;
    #[cfg(windows)]
    #[tokio::test]
    async fn copy_progress_crosses_job_transport_and_older_engines_still_work() {
        use tokio::net::windows::named_pipe::ServerOptions;
        use crate::wire::Response;
        for mode in ["measured", "old", "failed"] {
            let endpoint = format!("{}-copy-progress-{}-{mode}", wire::endpoint().unwrap(), std::process::id());
            let mut server = ServerOptions::new().first_pipe_instance(true).create(&endpoint).unwrap();
            let responses = match mode {
                "measured" => vec![
                    Response::success(json!({"status":"queued"})),
                    Response::success(json!({"status":"running","progress":{"phase":"copying","completedBytes":45,"totalBytes":100}})),
                    Response::success(json!({"status":"complete","progress":{"phase":"finishing","completedBytes":100,"totalBytes":100},"result":{"destination":"/copied"}})),
                ],
                "old" => vec![
                    Response::failure("Unknown parameter 'wait'"),
                    Response::success(json!({"status":"running"})),
                    Response::success(json!({"status":"complete","result":{"destination":"/copied"}})),
                ],
                _ => vec![Response::success(json!({"status":"failed","method":"copy_files_to_environment","error":"Disk full"}))],
            };
            let count = responses.len();
            let address = endpoint.clone();
            let worker = tokio::spawn(async move {
                for (index, response) in responses.into_iter().enumerate() {
                    server.connect().await.unwrap();
                    let bytes = wire::read_frame(&mut server, wire::MAX_REQUEST).await.unwrap();
                    let req: Request = serde_json::from_slice(&bytes).unwrap();
                    assert_eq!(req.method, "jobs_get");
                    assert_eq!(req.params["jobId"], "copy-test");
                    if mode != "old" || index == 0 { assert_eq!(req.params["wait"], 250); }
                    else { assert!(req.params.get("wait").is_none()); }
                    // Reserve the next instance before the client can make its next poll.
                    let next = if index + 1 < count { Some(ServerOptions::new().create(&address).unwrap()) } else { None };
                    wire::write_frame(&mut server, &serde_json::to_vec(&response).unwrap(), wire::MAX_RESPONSE).await.unwrap();
                    let mut ack = [0];
                    let _ = tokio::io::AsyncReadExt::read(&mut server, &mut ack).await;
                    if let Some(next) = next { server = next; }
                }
            });
            let mut observed = Vec::new();
            let result = tokio::time::timeout(Duration::from_secs(10), wait_job_observed(&endpoint, "copy-test", 5, 250, |progress| observed.push(progress.clone()))).await.unwrap();
            worker.await.unwrap();
            if mode == "failed" {
                assert!(result.unwrap_err().contains("Disk full (job copy-test)"));
            } else { assert_eq!(result.unwrap()["destination"], "/copied"); }
            if mode == "measured" {
                assert_eq!(observed.len(), 2);
                assert_eq!(observed[0]["completedBytes"], 45);
                assert_eq!(observed[1]["phase"], "finishing");
            } else { assert!(observed.is_empty()); }
        }
    }
    #[test]
    fn outdated_engine_errors_explain_restart_without_retrying_requests() {
        let error = engine_error("runpod_status", "Unknown method 'runpod_status'. Run yougori schema.".into());
        assert!(error.contains("yougori app quit --yes"));
        assert!(error.contains("Quitting stops running environments"));
        let unknown = "Unknown method 'typo'. Run yougori schema.";
        assert_eq!(engine_error("typo", unknown.into()), unknown);
        assert_eq!(engine_error("runpod_status", "Account unavailable".into()), "Account unavailable");
    }
    #[test]
    fn timeout_covers_long_poll_and_guest_requests() {
        assert_eq!(
            request_timeout(&request("jobs_get", json!({"wait":30000}))),
            Duration::from_secs(50)
        );
        assert_eq!(
            request_timeout(&request("jobs_get", json!({"wait":u64::MAX}))),
            Duration::from_secs(50)
        );
        assert!(
            request_timeout(&request("terminal_action", json!({"action":"write"})))
                > Duration::from_secs(25)
        );
    }
    #[test]
    fn finds_engine_in_relocated_bundle_with_spaces() {
        assert_eq!(
            macos_app_candidates(Path::new(
                "/Users/test/My Apps/Renamed.app/Contents/Resources/cli/yougori-cli"
            )),
            vec![PathBuf::from(
                "/Users/test/My Apps/Renamed.app/Contents/MacOS/yougori"
            )]
        );
        assert!(macos_app_candidates(Path::new("/usr/bin/yougori-cli")).is_empty());
    }
}
