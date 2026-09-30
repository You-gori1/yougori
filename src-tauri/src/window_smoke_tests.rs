//! Real WebView2/IPC regression, not a browser mock. No screenshots or user data.
use crate::{commands, store::PlatformStore};
use std::sync::{Arc, Mutex};
use tauri::{Manager, State};

#[derive(Default)]
struct SmokeResult {
    windows: Mutex<Vec<String>>,
    error: Mutex<Option<String>>,
}

#[tauri::command]
fn window_smoke_report(
    label: String,
    error: Option<String>,
    app: tauri::AppHandle,
    state: State<'_, Arc<SmokeResult>>,
) {
    if let Some(error) = error {
        *state.error.lock().unwrap() = Some(error);
        app.exit(1);
        return;
    }
    let mut windows = state.windows.lock().unwrap();
    if !windows.contains(&label) {
        windows.push(label);
    }
    if windows.len() == 2 {
        app.exit(0);
    }
}

#[test]
#[ignore = "opens real hidden WebView2 windows; requires the dev server on localhost:1420"]
fn native_guest_windows_render_through_async_ipc() {
    let data = tempfile::tempdir().unwrap();
    let store = PlatformStore::load(data.path().join("state.json")).unwrap();
    store.mutate(|s| {
        s.environments = vec![serde_json::from_value(serde_json::json!({
            "id":"env-window-smoke","name":"Window regression test","kind":"container","provider":"yougoriOci","status":"running","runtime":"alpine","description":"test","createdAt":"2026-01-01T00:00:00Z",
            "cpuUsage":0,"memoryUsageGb":0,"storageDeltaGb":0,"networkRxMbps":0,
            "resourcePolicy":{"cpu":{"min":0.1,"preferred":0.25,"max":1,"current":0},"memoryGb":{"min":0.125,"preferred":0.25,"max":0.5,"current":0},"priority":"normal","dynamic":false}
        })).unwrap()]; Ok(())
    }).unwrap();
    let result = Arc::new(SmokeResult::default());
    let mut context = tauri::generate_context!();
    context.config_mut().identifier = format!(
        "com.opendock.window-smoke-{}",
        uuid::Uuid::new_v4().simple()
    );
    context.config_mut().app.windows.clear();
    let app = tauri::Builder::default().any_thread().manage(store).manage(result.clone())
        .invoke_handler(tauri::generate_handler![commands::get_platform_state, commands::open_environment_window, window_smoke_report])
        .on_page_load(|webview, payload| {
            if !matches!(payload.event(), tauri::webview::PageLoadEvent::Finished) { return; }
            let code = if webview.label() == "main" {
                "(async()=>{try{for(let i=0;i<2;i++)await window.__TAURI_INTERNALS__.invoke('open_environment_window',{environmentId:'env-window-smoke'});}catch(e){window.__TAURI_INTERNALS__.invoke('window_smoke_report',{label:'main',error:String(e)});}})()".to_owned()
            } else {
                // Hide test windows after native construction; never capture their pixels.
                let _ = webview.window().hide();
                format!("let n=0;const t=setInterval(()=>{{if(document.querySelector('[aria-label=\"Switch environment\"]')){{clearInterval(t);window.__TAURI_INTERNALS__.invoke('window_smoke_report',{{label:{},error:null}});}}else if(++n>150){{clearInterval(t);window.__TAURI_INTERNALS__.invoke('window_smoke_report',{{label:'timeout',error:'Guest toolbar did not render'}});}}}},100);", serde_json::to_string(webview.label()).unwrap())
            };
            webview.eval(&code).unwrap();
        })
        .setup(move |app| {
            tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::App("index.html".into())).visible(false).data_directory(data.path().join("webview")).build()?;
            let handle = app.handle().clone();
            std::thread::spawn(move || { std::thread::sleep(std::time::Duration::from_secs(25)); handle.exit(1); });
            // The backing directory must outlive every test WebView2 process.
            app.manage(data);
            Ok(())
        }).build(context).unwrap();
    let code = app.run_return(|_, _| {});
    assert_eq!(code, 0, "{:?}", result.error.lock().unwrap());
    assert_eq!(result.windows.lock().unwrap().len(), 2);
}
