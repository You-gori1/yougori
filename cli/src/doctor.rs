//! `yougori doctor`: is this computer ready, and what to do when it isn't. Read-only.
use crate::public::call;
use serde_json::{json, Value};

fn check(name: &str, status: &str, detail: impl Into<String>, fix: Option<&str>) -> Value {
    json!({"name": name, "status": status, "detail": detail.into(), "fix": fix})
}

fn on_path() -> Option<std::path::PathBuf> {
    let name = if cfg!(windows) { "yougori.exe" } else { "yougori" };
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).map(|dir| dir.join(name)).find(|p| p.is_file())
}

/// Every check runs; a failure in one never hides the others.
pub async fn run() -> Value {
    let mut checks = vec![check("CLI", "ok", format!("yougori {}", env!("CARGO_PKG_VERSION")), None)];
    checks.push(match on_path() {
        Some(path) => check("PATH", "ok", format!("`yougori` runs from {}", path.display()), None),
        None => check("PATH", "warning", "`yougori` is not on your PATH, so new terminals won't find it", Some("Reinstall Yougori, or add its cli folder to your user PATH")),
    });
    let app = crate::client::desktop_executable(None).ok();
    let standalone = crate::client::engine_executable();
    checks.push(match (&app, &standalone) {
        (Some(app), _) => check("Installed", "ok", format!("Yougori app at {}", app.display()), None),
        (None, Some(engine)) => check("Installed", "ok", format!("Engine only (no desktop app) at {}", engine.display()), None),
        (None, None) => check("Installed", "error", "Neither the Yougori app nor the standalone engine was found", Some("Reinstall Yougori, or set YOUGORI_APP / YOUGORI_ENGINE to its absolute path")),
    });
    let engine = crate::client::call(&crate::client::request("app_status", json!({}))).await;
    let Ok(engine) = engine else {
        checks.push(check("Engine", "error", "The Yougori engine is not running", Some("Run `yougori app start`, or open the Yougori app")));
        return summarize(checks);
    };
    let how = if engine["engineOnly"] == true { " as the standalone engine (no dashboard)" } else if engine["headless"] == true { " in the background" } else { "" };
    checks.push(check("Engine", "ok", format!("Running {} (pid {}){how}", engine["version"].as_str().unwrap_or("?"), engine["pid"]), None));
    if let Ok(state) = call("get_platform_state", json!({})).await {
        for provider in state["providers"].as_array().into_iter().flatten() {
            let name = provider["name"].as_str().unwrap_or("Runtime");
            let detail = provider["detail"].as_str().unwrap_or("").to_owned();
            checks.push(match provider["status"].as_str() {
                Some("ready") => check(name, "ok", detail, None),
                Some("needsSetup") => check(name, "warning", detail, Some("Open Yougori and follow the setup for this runtime")),
                _ => check(name, "warning", detail, None),
            });
        }
        let host = &state["host"];
        if let (Some(total), Some(used)) = (host["totalStorageGb"].as_f64(), host["usedStorageGb"].as_f64()) {
            let free = total - used;
            checks.push(if free < 20.0 {
                check("Disk space", "warning", format!("{free:.0} GB free on the environment drive"), Some("Free space or move storage with `yougori storage location --set PATH`"))
            } else {
                check("Disk space", "ok", format!("{free:.0} GB free on the environment drive"), None)
            });
        }
        let settings = &state["settings"];
        checks.push(if settings["launchAtStartup"] == true {
            check("Start at login", "ok", if settings["startupHeadless"] == true { "The engine starts in the background at login" } else { "The app opens at login" }, None)
        } else {
            check("Start at login", "info", "The engine starts only when you run a command or open the app", Some("`yougori app autostart on` keeps it ready in the background"))
        });
    }
    if let Ok(gpu) = call("get_cuda_runtime_status", json!({})).await {
        for item in gpu["checks"].as_array().into_iter().flatten() {
            let name = format!("GPU · {}", item["name"].as_str().unwrap_or("check"));
            checks.push(check(&name, if item["passed"] == true { "ok" } else { "warning" }, item["detail"].as_str().unwrap_or(""), None));
        }
        if gpu["supported"] == true && gpu["installed"] != true {
            checks.push(check("GPU · CUDA runtime", "warning", gpu["detail"].as_str().unwrap_or("Not installed"), Some("`yougori gpu setup --yes`")));
        }
    }
    summarize(checks)
}

fn summarize(checks: Vec<Value>) -> Value {
    let errors = checks.iter().filter(|c| c["status"] == "error").count();
    let warnings = checks.iter().filter(|c| c["status"] == "warning").count();
    json!({"ready": errors == 0, "errors": errors, "warnings": warnings, "checks": checks})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn readiness_counts_errors_and_warnings() {
        let summary = summarize(vec![check("A", "ok", "", None), check("B", "warning", "", Some("fix")), check("C", "error", "", None)]);
        assert_eq!((summary["ready"].as_bool(), summary["errors"].as_u64(), summary["warnings"].as_u64()), (Some(false), Some(1), Some(1)));
        assert_eq!(summary["checks"][1]["fix"], "fix");
    }
}
