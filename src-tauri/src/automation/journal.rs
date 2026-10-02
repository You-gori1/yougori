//! Allowlisted operation metadata only: no arguments, result bodies, URLs or raw errors.
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
const LIMIT: usize = 256;

pub(super) struct Journal {
    path: PathBuf,
}
impl Journal {
    pub(super) fn new(root: &Path) -> Self {
        Self {
            path: root.join("operations.json"),
        }
    }
    pub(super) fn load(&self) -> Vec<Value> {
        let Ok(metadata) = fs::symlink_metadata(&self.path) else {
            return Vec::new();
        };
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() > 2 * 1024 * 1024
        {
            return Vec::new();
        }
        let Ok(data) = fs::read(&self.path) else {
            return Vec::new();
        };
        let Ok(mut entries) = serde_json::from_slice::<Vec<Value>>(&data) else {
            return Vec::new();
        };
        entries.truncate(LIMIT);
        for entry in &mut entries {
            if matches!(
                entry["status"].as_str(),
                Some("queued" | "running" | "cancelling")
            ) {
                entry["status"] = json!("interrupted");
                entry["outcome"] = json!("reconciliation_required");
                entry["errorCode"] = json!("engine_interrupted");
                entry["errorDetails"] = json!({"code":"engine_interrupted","outcome":"reconciliation_required","retryable":false,"affectedResource":null});
                entry["completedAt"] = json!(chrono::Utc::now().to_rfc3339());
            }
        }
        entries
            .into_iter()
            .map(|entry| allowlisted(&entry))
            .collect()
    }
    pub(super) fn save(&self, entries: &[Value]) -> Result<(), String> {
        let parent = self
            .path
            .parent()
            .ok_or("Operation journal directory missing")?;
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.as_file()
                .set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(|e| e.to_string())?;
        }
        let entries = entries
            .iter()
            .rev()
            .take(LIMIT)
            .rev()
            .map(allowlisted)
            .collect::<Vec<_>>();
        serde_json::to_writer(&mut file, &entries).map_err(|e| e.to_string())?;
        file.write_all(b"\n").map_err(|e| e.to_string())?;
        file.as_file().sync_all().map_err(|e| e.to_string())?;
        file.persist(&self.path).map_err(|e| e.error.to_string())?;
        Ok(())
    }
}
fn valid_id(value: &str, prefix: &str) -> bool {
    value
        .strip_prefix(prefix)
        .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok())
}
fn allowlisted(value: &Value) -> Value {
    let mut safe = json!({});
    if let Some(id) = value["jobId"].as_str().filter(|id| valid_id(id, "job-")) {
        safe["jobId"] = json!(id);
    }
    if let Some(method) = value["method"]
        .as_str()
        .filter(|name| yougori_cli::catalog::find(name).is_ok())
    {
        safe["method"] = json!(method);
    }
    for (field, allowed) in [
        (
            "status",
            &[
                "queued",
                "running",
                "complete",
                "failed",
                "cancelled",
                "interrupted",
            ][..],
        ),
        (
            "outcome",
            &[
                "pending",
                "succeeded",
                "failed",
                "cancelled",
                "partial",
                "unknown",
                "not_started",
                "not_submitted",
                "reconciliation_required",
            ][..],
        ),
        (
            "errorCode",
            &[
                "operation_failed",
                "operation_cancelled",
                "operation_interrupted",
                "engine_interrupted",
                "transfer_inactive",
                "transfer_deadline",
                "outcome_unknown",
                "incompatible_engine",
                "engine_unavailable",
                "resource_conflict",
                "permission_denied",
                "invalid_request",
                "resource_not_found",
                "unsupported_capability",
                "durable_storage_missing",
                "shutdown_deadline",
            ][..],
        ),
    ] {
        if let Some(text) = value[field].as_str().filter(|v| allowed.contains(v)) {
            safe[field] = json!(text);
        }
    }
    for field in ["createdAt", "startedAt", "completedAt", "lastProgressAt"] {
        if let Some(stamp) = value[field]
            .as_str()
            .filter(|s| s.len() <= 64 && chrono::DateTime::parse_from_rfc3339(s).is_ok())
        {
            safe[field] = json!(stamp);
        }
    }
    for field in ["cancelRequested", "cancellable"] {
        if let Some(flag) = value[field].as_bool() {
            safe[field] = json!(flag);
        }
    }
    if let Some(resources) = value["resources"].as_array() {
        safe["resources"] = json!(resources
            .iter()
            .filter_map(Value::as_str)
            .filter(|r| *r == "configuration:settings"
                || ["environment:env-", "transfer:env-", "execution:env-"]
                    .iter()
                    .any(|prefix| valid_id(r, prefix)))
            .take(8)
            .collect::<Vec<_>>());
    }
    if let Some(details) = value.get("errorDetails") {
        let mut metadata = json!({});
        if let Some(code) = safe["errorCode"].as_str() {
            metadata["code"] = json!(code);
        }
        if let Some(outcome) = safe["outcome"].as_str() {
            metadata["outcome"] = json!(outcome);
        }
        if let Some(flag) = details["retryable"].as_bool() {
            metadata["retryable"] = json!(flag);
        }
        metadata["affectedResource"] = details["affectedResource"]
            .as_str()
            .filter(|id| valid_id(id, "env-"))
            .map(|id| json!(id))
            .unwrap_or(Value::Null);
        safe["errorDetails"] = metadata;
    }
    if let Some(progress) = value.get("progress") {
        let mut measurement = json!({});
        if let Some(phase) = progress["phase"].as_str().filter(|phase| {
            [
                "scanning",
                "archiving",
                "preparing",
                "connecting",
                "sending",
                "receiving",
                "waitingForSourceCompletion",
                "extracting",
                "verifying",
                "copying",
                "finishing",
                "complete",
                "setup",
                "starting",
                "publishing",
                "readiness",
                "executing",
                "waiting",
            ]
            .contains(phase)
        }) {
            measurement["phase"] = json!(phase);
        }
        for field in [
            "completedBytes",
            "totalBytes",
            "sentBytes",
            "confirmedBytes",
            "scannedEntries",
            "step",
        ] {
            if let Some(number) = progress[field].as_u64() {
                measurement[field] = json!(number);
            }
        }
        if let Some(stamp) = progress["lastProgressAt"]
            .as_str()
            .filter(|s| s.len() <= 64 && chrono::DateTime::parse_from_rfc3339(s).is_ok())
        {
            measurement["lastProgressAt"] = json!(stamp);
        }
        if let Some(id) = progress["transferId"]
            .as_str()
            .filter(|id| uuid::Uuid::parse_str(id).is_ok())
        {
            measurement["transferId"] = json!(id);
        }
        safe["progress"] = measurement;
    }
    if let Some(output) = value.get("output") {
        let mut metadata = json!({});
        if let Some(delivery) = output["delivery"]
            .as_str()
            .filter(|v| ["paged", "evicted"].contains(v))
        {
            metadata["delivery"] = json!(delivery);
        }
        if let Some(handle) = output["resultHandle"]
            .as_str()
            .filter(|id| valid_id(id, "job-"))
        {
            metadata["resultHandle"] = json!(handle);
        }
        for field in ["totalBytes", "availableBytes"] {
            if let Some(number) = output[field].as_u64() {
                metadata[field] = json!(number);
            }
        }
        for field in ["truncated", "outputAvailable"] {
            if let Some(flag) = output[field].as_bool() {
                metadata[field] = json!(flag);
            }
        }
        safe["output"] = metadata;
    }
    safe
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn export_phases_survive_restart_without_copying_untrusted_fields() {
        for phase in ["receiving", "waitingForSourceCompletion"] {
            let directory = tempfile::tempdir().unwrap();
            let journal = Journal::new(directory.path());
            journal
                .save(&[json!({
                    "jobId": "job-00000000-0000-0000-0000-000000000003",
                    "method": "copy_files_from_environment",
                    "status": "running",
                    "progress": {"phase": phase, "confirmedBytes": 12, "guestUrl": "SECRET"}
                })])
                .unwrap();
            let restored = journal.load();
            assert_eq!(restored[0]["progress"]["phase"], phase);
            assert_eq!(restored[0]["progress"]["confirmedBytes"], 12);
            assert!(!fs::read_to_string(&journal.path)
                .unwrap()
                .contains("SECRET"));
        }
    }
    #[test]
    fn partial_outcomes_keep_structured_codes_but_never_untrusted_error_fields() {
        let directory = tempfile::tempdir().unwrap();
        let journal = Journal::new(directory.path());
        journal.save(&[json!({"jobId":"job-00000000-0000-0000-0000-000000000002","method":"copy_files_to_environment","status":"failed","outcome":"partial","errorCode":"transfer_inactive","errorDetails":{"code":"transfer_inactive","outcome":"partial","retryable":false,"affectedResource":"env-00000000-0000-0000-0000-000000000002","message":"BEARER_SECRET","secret":"BEARER_SECRET"}})]).unwrap();
        let saved = fs::read_to_string(&journal.path).unwrap();
        assert!(!saved.contains("BEARER_SECRET"));
        let restored = journal.load();
        assert_eq!(restored[0]["outcome"], "partial");
        assert_eq!(restored[0]["errorDetails"]["code"], "transfer_inactive");
        assert_eq!(restored[0]["errorDetails"]["retryable"], false);
    }
    #[test]
    fn restart_retains_interruption_without_credentials_or_raw_errors() {
        let directory = tempfile::tempdir().unwrap();
        let journal = Journal::new(directory.path());
        journal.save(&[json!({"jobId":"job-00000000-0000-0000-0000-000000000001","method":"copy_files_to_environment","status":"running","resources":["environment:env-00000000-0000-0000-0000-000000000001","project:/secret-token","transfer:env-BEARER_SECRET"],"request":{"token":"SECRET"},"error":"http://secret-token/","result":{"password":"SECRET"},"output":{"resultHandle":"SECRET","credential":"SECRET","delivery":"paged","availableBytes":5},"progress":{"phase":"sending","sentBytes":4,"guestUrl":"http://SECRET/"}})]).unwrap();
        let saved = fs::read_to_string(&journal.path).unwrap();
        assert!(!saved.contains("SECRET"));
        assert!(!saved.contains("secret-token"));
        let loaded = journal.load();
        assert_eq!(loaded[0]["status"], "interrupted");
        assert_eq!(loaded[0]["outcome"], "reconciliation_required");
    }
}
