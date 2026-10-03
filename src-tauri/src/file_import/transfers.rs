use super::CopyProgress;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

fn registry() -> &'static Mutex<HashMap<String, Arc<Transfer>>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, Arc<Transfer>>>> = OnceLock::new();
    REGISTRY.get_or_init(Default::default)
}
pub(crate) struct Transfer {
    pub id: String,
    pub environment_id: String,
    pub cancellation: CancellationToken,
    measurement: Mutex<(CopyProgress, Instant)>,
    started: Instant,
    partial_kind: std::sync::atomic::AtomicU8,
}
pub(crate) struct Lease {
    pub transfer: Arc<Transfer>,
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.transfer.cancellation.cancel();
        registry()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.transfer.id);
    }
}
impl Transfer {
    pub(crate) fn use_guest_import(&self) {
        self.partial_kind
            .store(0, std::sync::atomic::Ordering::Relaxed);
    }
    pub(crate) fn use_cloud_staging(&self) {
        self.partial_kind
            .store(1, std::sync::atomic::Ordering::Relaxed);
    }
    pub(crate) fn use_host_export(&self) {
        self.partial_kind
            .store(2, std::sync::atomic::Ordering::Relaxed);
    }
    pub(crate) fn use_host_archive_export(&self) {
        self.partial_kind
            .store(3, std::sync::atomic::Ordering::Relaxed);
    }
    pub(crate) fn use_host_staging_export(&self) {
        self.partial_kind
            .store(4, std::sync::atomic::Ordering::Relaxed);
    }
    fn partial_policy(&self) -> &'static str {
        match self.partial_kind.load(std::sync::atomic::Ordering::Relaxed) {
            1 => "remove_unpublished_stage_on_stream_close_preserve_published_copy",
            2 => "preserve_completed_unique_host_files_remove_incomplete_file",
            3 => "remove_unverified_host_archive_preserve_originals",
            4 => "remove_unpublished_host_staging_preserve_originals",
            _ => "preserve_unique_guest_directory",
        }
    }
    pub(crate) fn check(&self) -> Result<(), String> {
        if self.cancellation.is_cancelled() {
            return Err(format!("YOUGORI_OPERATION_CANCELLED: transfer {} cancelled; originals unchanged; partialCopyPolicy={}. Inspect the unique transfer destination before retrying.",self.id,self.partial_policy()));
        }
        if self.started.elapsed() > Duration::from_secs(12 * 60 * 60) {
            return Err(format!("YOUGORI_TRANSFER_DEADLINE: transfer exceeded its 12-hour total limit; partialCopyPolicy={}",self.partial_policy()));
        }
        Ok(())
    }
    pub(crate) fn idle_for(&self) -> Duration {
        self.measurement
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .1
            .elapsed()
    }
    pub(crate) fn report(&self, mut progress: CopyProgress) -> CopyProgress {
        let mut measurement = self.measurement.lock().unwrap_or_else(|e| e.into_inner());
        let changed = measurement.0.phase != progress.phase
            || measurement.0.completed_bytes != progress.completed_bytes
            || measurement.0.scanned_entries != progress.scanned_entries
            || measurement.0.sent_bytes != progress.sent_bytes
            || measurement.0.confirmed_bytes != progress.confirmed_bytes;
        if changed || measurement.0.last_progress_at.is_none() {
            measurement.1 = Instant::now();
            progress.last_progress_at = Some(chrono::Utc::now().to_rfc3339());
        } else {
            progress.last_progress_at = measurement.0.last_progress_at.clone();
        }
        progress.transfer_id = Some(self.id.clone());
        measurement.0 = progress.clone();
        progress
    }
}
pub(crate) fn begin(environment_id: &str) -> Result<Lease, String> {
    let mut registry = registry().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(existing) = registry
        .values()
        .find(|transfer| transfer.environment_id == environment_id)
    {
        return Err(format!("Transfer {} is already copying to this environment. Inspect or cancel that transfer before starting another.",existing.id));
    }
    let cancellation = crate::automation::context::current()
        .map(|operation| operation.cancellation.child_token())
        .unwrap_or_default();
    let id = uuid::Uuid::new_v4().simple().to_string();
    let transfer = Arc::new(Transfer {
        id: id.clone(),
        environment_id: environment_id.into(),
        cancellation,
        measurement: Mutex::new((CopyProgress::default(), Instant::now())),
        started: Instant::now(),
        partial_kind: std::sync::atomic::AtomicU8::new(0),
    });
    registry.insert(id, transfer.clone());
    Ok(Lease { transfer })
}
pub(crate) fn find(id: &str) -> Option<Arc<Transfer>> {
    registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(id)
        .cloned()
}
pub(crate) fn cancel(environment_id: Option<&str>, id: Option<&str>) -> Value {
    let registry = registry().lock().unwrap_or_else(|e| e.into_inner());
    let cancelled=registry.values().filter(|transfer|environment_id.is_none_or(|env|transfer.environment_id==env)&&id.is_none_or(|id|transfer.id==id)).map(|transfer|{
        transfer.cancellation.cancel();json!({"transferId":transfer.id,"environmentId":transfer.environment_id,"cancelRequested":true,"partialCopyPolicy":transfer.partial_policy(),"phase":transfer.measurement.lock().unwrap_or_else(|e|e.into_inner()).0.phase})
    }).collect::<Vec<_>>();
    json!({"transfers":cancelled,"cancelRequested":!cancelled.is_empty()})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn no_progress_heartbeat_does_not_reset_inactivity_and_cancel_is_cooperative() {
        let lease = begin("env-transfer-heartbeat-test").unwrap();
        let transfer = lease.transfer.clone();
        let progress = transfer.report(CopyProgress {
            phase: "sending",
            completed_bytes: 5,
            total_bytes: 100,
            ..Default::default()
        });
        std::thread::sleep(Duration::from_millis(10));
        let next = transfer.report(progress.clone());
        assert_eq!(next.last_progress_at, progress.last_progress_at);
        assert!(transfer.idle_for() >= Duration::from_millis(10));
        let result = cancel(Some(&transfer.environment_id), Some(&transfer.id));
        assert_eq!(result["cancelRequested"], true);
        assert!(transfer
            .check()
            .unwrap_err()
            .starts_with("YOUGORI_OPERATION_CANCELLED"));
        drop(lease);
        assert!(find(&transfer.id).is_none());
    }
    #[test]
    fn environment_cancellation_preserves_independent_transfers_and_releases_admission() {
        let first_environment = format!("env-transfer-a-{}", uuid::Uuid::new_v4().simple());
        let second_environment = format!("env-transfer-b-{}", uuid::Uuid::new_v4().simple());
        let first = begin(&first_environment).unwrap();
        let second = begin(&second_environment).unwrap();
        assert!(begin(&first_environment).is_err());
        assert_eq!(cancel(Some(&first_environment), None)["transfers"].as_array().unwrap().len(), 1);
        assert!(first.transfer.check().is_err());
        assert!(second.transfer.check().is_ok());
        drop(first);
        let replacement = begin(&first_environment).unwrap();
        assert!(replacement.transfer.check().is_ok());
        assert!(second.transfer.check().is_ok());
    }
    #[tokio::test]
    async fn queued_operation_cancellation_reaches_its_transfer_without_cancelling_peers() {
        let cancellation = CancellationToken::new();
        let peer = begin(&format!("env-peer-{}", uuid::Uuid::new_v4().simple())).unwrap();
        let context = crate::automation::context::OperationContext {
            id: "disposable-cancellation-fixture".into(),
            cancellation: cancellation.clone(),
            progress: Arc::new(|_| {}),
        };
        let transfer = crate::automation::context::scope(context, async {
            begin(&format!("env-child-{}", uuid::Uuid::new_v4().simple())).unwrap()
        })
        .await;
        cancellation.cancel();
        assert!(transfer.transfer.check().unwrap_err().starts_with("YOUGORI_OPERATION_CANCELLED"));
        assert!(peer.transfer.check().is_ok());
    }
}
