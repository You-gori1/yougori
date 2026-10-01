//! Same-user, local-only automation. No HTTP listener, bearer token in a URL,
//! alternate disk owner, or webview-IPC bridge is introduced by this interface.
pub(crate) mod dispatch;
mod transport;

use crate::{runtime::RuntimeManager, store::PlatformStore};
use yougori_cli::{
    catalog,
    wire::{self, Request, Response},
};
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    sync::Arc,
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager};
use crate::AppHandle;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::{Mutex, Semaphore},
};

const HISTORY: usize = 64;
const RESULT_BUDGET: usize = 64 * 1024 * 1024;
const RESULT_LIMIT: usize = 8 * 1024 * 1024;
struct Job {
    id: String,
    method: String,
    status: &'static str,
    created: String,
    completed: Option<Instant>,
    completed_at: Option<String>,
    result: Option<Value>,
    error: Option<String>,
    bytes: usize,
    progress: tokio::sync::watch::Receiver<Option<Value>>,
}
impl Job {
    fn value(&self, result: bool) -> Value {
        let mut value = json!({"jobId":self.id,"method":self.method,"status":self.status,"createdAt":self.created});
        value["completedAt"] = self
            .completed_at
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null);
        if let Some(progress) = self.progress.borrow().as_ref() {
            value["progress"] = progress.clone();
        }
        if result {
            value["result"] = self.result.clone().unwrap_or(Value::Null);
            value["error"] = self.error.clone().map(Value::String).unwrap_or(Value::Null);
        }
        value
    }
}
pub struct Control {
    endpoint: String,
    pub headless: bool,
    jobs: Mutex<VecDeque<Job>>,
    writes: Mutex<()>,
    operations: Arc<Semaphore>,
    clients: Arc<Semaphore>,
    /// Wakes `jobs_get` long-polls whenever any job finishes.
    finished: tokio::sync::Notify,
}
impl Control {
    fn prune(jobs: &mut VecDeque<Job>) {
        jobs.retain(|job| {
            job.completed
                .is_none_or(|time| time.elapsed() < Duration::from_secs(1800))
        });
        while jobs.len() >= HISTORY
            || jobs.iter().map(|job| job.bytes).sum::<usize>() > RESULT_BUDGET
        {
            let Some(index) = jobs.iter().position(|job| job.completed.is_some()) else {
                break;
            };
            jobs.remove(index);
        }
    }
    async fn handle(self: &Arc<Self>, app: AppHandle, request: Request) -> Response {
        match self.submit(app, request).await {
            Ok(result) => Response::success(result),
            Err(error) => Response::failure(error),
        }
    }
    async fn submit(self: &Arc<Self>, app: AppHandle, request: Request) -> Result<Value, String> {
        if request.version != wire::VERSION {
            return Err("CLI/engine protocol mismatch. Update both components.".into());
        }
        let method = catalog::find(&request.method)?;
        method.validate(&request.params)?;
        dispatch::validate(&request.method, &request.params)?;
        let confirmation = method.confirmation_for(&request.params);
        if request.dry_run {
            return Ok(
                json!({"dryRun":true,"method":method.name,"validSyntax":true,"confirmationRequired":confirmation,"runtimeChecked":false}),
            );
        }
        if !request.confirmed {
            if let Some(reason) = confirmation {
                return Err(format!("{reason} Explicit --yes confirmation is required."));
            }
        }
        match method.name {
            "app_status" => {
                return Ok(
                    json!({"running":true,"version":env!("CARGO_PKG_VERSION"),"protocolVersion":wire::VERSION,"headless":self.headless,"engineOnly":crate::ENGINE_ONLY,"endpoint":self.endpoint,"pid":std::process::id()}),
                )
            }
            "jobs_list" => {
                let mut jobs = self.jobs.lock().await;
                Self::prune(&mut jobs);
                return Ok(Value::Array(jobs.iter().map(|j| j.value(false)).collect()));
            }
            "jobs_get" => {
                // `wait` long-polls up to 30 s for completion, so callers learn the result
                // the moment it exists instead of sleeping between polls.
                let deadline = Instant::now() + Duration::from_millis(request.params["wait"].as_u64().unwrap_or(0).min(30_000));
                loop {
                    let finished = self.finished.notified();
                    tokio::pin!(finished);
                    finished.as_mut().enable();
                    {
                        let mut jobs = self.jobs.lock().await;
                        Self::prune(&mut jobs);
                        let job = jobs.iter().find(|j|Some(j.id.as_str())==request.params["jobId"].as_str()).ok_or("Job not found (completed jobs expire after 30 minutes or an engine restart). Inspect persisted state before retrying.")?;
                        if job.completed.is_some() || Instant::now() >= deadline {
                            return Ok(job.value(true));
                        }
                    }
                    let _ = tokio::time::timeout_at(deadline.into(), finished).await;
                }
            }
            _ => {}
        }
        // Read-only snapshots, terminal I/O and streamed chat replies are quick and never
        // queued behind long imports; terminals keep their own per-session locks in the guest,
        // and a reply streams in its own task.
        if matches!(
            method.name,
            "get_platform_state" | "list_environment_windows" | "terminal_action"
                | "list_environment_downloads" | "keep_environment_downloads_alive" | "stop_environment_download"
                | "list_environment_services" | "list_saved_domains" | "model_status" | "model_api_status"
                | "get_storage_allocation" | "vault_summary" | "model_chat_begin" | "model_chat_read"
        ) {
            return dispatch::dispatch(&app, method.name, &request.params).await;
        }
        let permit=self.operations.clone().try_acquire_owned().map_err(|_|"Eight CLI operations are already pending. Inspect jobs and wait before submitting more.")?;
        let mut request = request;
        if method.name == "start_environment_download" {
            let id = request.params["request"]["environmentId"].as_str().ok_or("Invalid environment ID")?;
            let generation = app.state::<crate::environment_download::Downloads>().queued_generation(id)?;
            request.params["_downloadGeneration"] = generation.into();
        }
        let id = format!("job-{}", uuid::Uuid::new_v4().simple());
        // Keep only the latest measurement; copy callbacks also run on blocking workers.
        let (progress, measurement) = tokio::sync::watch::channel(None);
        {
            let mut jobs = self.jobs.lock().await;
            Self::prune(&mut jobs);
            jobs.push_back(Job {
                id: id.clone(),
                method: request.method.clone(),
                status: "queued",
                created: chrono::Utc::now().to_rfc3339(),
                completed: None,
                completed_at: None,
                result: None,
                error: None,
                bytes: 0,
                progress: measurement,
            });
        }
        let control = self.clone();
        let job_id = id.clone();
        tauri::async_runtime::spawn(async move {
            let _permit = permit;
            // Only CLI mutations are serialized here. Native provider locks and
            // the shared store continue to coordinate with ordinary UI actions.
            let _write = if method.mutating {
                Some(control.writes.lock().await)
            } else {
                None
            };
            if let Some(job) = control
                .jobs
                .lock()
                .await
                .iter_mut()
                .find(|j| j.id == job_id)
            {
                job.status = "running";
            }
            let task_app = app.clone();
            let result = tokio::spawn(async move {
                dispatch::dispatch_with_progress(&task_app, &request.method, &request.params, Arc::new(move |value| {
                    progress.send_replace(Some(value));
                })).await
            })
            .await
            .unwrap_or_else(|_| {
                Err(
                    "The operation failed internally; inspect environment state before retrying."
                        .into(),
                )
            });
            let mut jobs = control.jobs.lock().await;
            if let Some(job) = jobs.iter_mut().find(|j| j.id == job_id) {
                match result {
                    Ok(value) => {
                        let bytes = serde_json::to_vec(&value)
                            .map(|v| v.len())
                            .unwrap_or(RESULT_LIMIT + 1);
                        if bytes <= RESULT_LIMIT {
                            job.result = Some(value);
                            job.bytes = bytes;
                            job.status = "complete";
                        } else {
                            job.error=Some("Operation completed, but its result exceeded 8 MB. Inspect state/console in smaller chunks; do not repeat the operation.".into());
                            job.status = "failed";
                        }
                    }
                    Err(error) => {
                        job.error = Some(error);
                        job.status = "failed";
                    }
                }
                job.completed = Some(Instant::now());
                job.completed_at = Some(chrono::Utc::now().to_rfc3339());
            }
            Self::prune(&mut jobs);
            drop(jobs);
            control.finished.notify_waiters();
            // CLI changes become visible in every existing desktop window.
            if method.mutating {
                if let Ok(state) = app.state::<PlatformStore>().snapshot() {
                    let _ = app.emit("yougori-platform-state", state);
                }
            }
        });
        Ok(json!({"accepted":true,"jobId":id,"status":"queued"}))
    }
}

async fn connection(
    mut stream: impl AsyncRead + AsyncWrite + Unpin,
    control: Arc<Control>,
    app: AppHandle,
) {
    let response = match tokio::time::timeout(
        Duration::from_secs(5),
        wire::read_frame(&mut stream, wire::MAX_REQUEST),
    )
    .await
    {
        Ok(Ok(bytes)) => match serde_json::from_slice::<Request>(&bytes) {
            Ok(request) => control.handle(app, request).await,
            Err(_) => Response::failure("Malformed Yougori control request"),
        },
        _ => return,
    };
    if let Ok(bytes) = serde_json::to_vec(&response) {
        let _ = tokio::time::timeout(
            Duration::from_secs(5),
            wire::write_frame(&mut stream, &bytes, wire::MAX_RESPONSE),
        )
        .await;
        // A Windows pipe handle must outlive the client's response read; closing
        // a buffered server immediately can discard unread reply bytes.
        let mut ack = [0u8];
        let _ = tokio::time::timeout(
            Duration::from_secs(2),
            tokio::io::AsyncReadExt::read(&mut stream, &mut ack),
        )
        .await;
    }
}

pub fn start(app: &AppHandle, headless: bool) -> Result<(), String> {
    let endpoint = wire::endpoint().map_err(|e| e.to_string())?;
    start_at(app, headless, endpoint)
}
fn start_at(app: &AppHandle, headless: bool, endpoint: String) -> Result<(), String> {
    let control = Arc::new(Control {
        endpoint: endpoint.clone(),
        headless,
        jobs: Mutex::new(VecDeque::new()),
        writes: Mutex::new(()),
        operations: Arc::new(Semaphore::new(8)),
        clients: Arc::new(Semaphore::new(16)),
        finished: tokio::sync::Notify::new(),
    });
    // Creation runs inside the already-owned Tokio runtime; failing to reserve
    // the endpoint aborts startup instead of exposing an unprotected fallback.
    let listener = tauri::async_runtime::block_on(async { transport::bind(&endpoint, true) })
        .map_err(|e| format!("Cannot reserve the same-user CLI endpoint: {e}"))?;
    app.manage(control.clone());
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        #[cfg(windows)]
        {
            let mut listener = listener;
            loop {
                if listener.connect().await.is_err() {
                    break;
                }
                // Keep a replacement instance alive before dropping a client,
                // preventing another process from taking over the pipe name.
                let next = match transport::bind(&endpoint, false) {
                    Ok(next) => next,
                    Err(error) => {
                        eprintln!("CLI listener stopped: {error}");
                        break;
                    }
                };
                let stream = std::mem::replace(&mut listener, next);
                if let Ok(permit) = control.clients.clone().try_acquire_owned() {
                    let control = control.clone();
                    let app = app.clone();
                    tokio::spawn(async move {
                        let _permit = permit;
                        connection(stream, control, app).await;
                    });
                }
            }
        }
        #[cfg(unix)]
        {
            while let Ok((stream, _)) = listener.accept().await {
                if let Ok(permit) = control.clients.clone().try_acquire_owned() {
                    let control = control.clone();
                    let app = app.clone();
                    tokio::spawn(async move {
                        let _permit = permit;
                        connection(stream, control, app).await;
                    });
                }
            }
        }
    });
    Ok(())
}

/// No dashboard means no React metric polling. Keep resource scheduling alive
/// for headless workloads, without booting any stopped runtime just to poll it.
pub async fn headless_tick(app: &AppHandle) {
    if !app.state::<Arc<Control>>().headless {
        return;
    }
    if !app.state::<PlatformStore>().snapshot().is_ok_and(|state| {
        state
            .environments
            .iter()
            .any(|env| env.status == crate::models::EnvironmentStatus::Running)
    }) {
        return;
    }
    let _ = crate::commands::refresh_host_metrics(
        app.state::<PlatformStore>(),
        app.state::<RuntimeManager>(),
    )
    .await;
}

#[cfg(test)]
mod tests;
