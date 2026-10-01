//! Append to WebView2's own menu without removing its standard commands.
use std::{cell::RefCell, collections::HashMap};
use webview2_com::{
    ContextMenuRequestedEventHandler, CustomItemSelectedEventHandler,
    ExecuteScriptCompletedHandler, Microsoft::Web::WebView2::Win32::*,
};
use windows::core::{w, Interface, HSTRING};

thread_local! {
    static HANDLERS: RefCell<HashMap<usize, i64>> = RefCell::new(HashMap::new());
}

pub fn install(webview: &tauri::Webview) {
    let window = webview.clone();
    let _ = webview.with_webview(move |native| unsafe {
        let result = (|| -> windows::core::Result<()> {
            let core = native.controller().CoreWebView2()?;
            let menu_core: ICoreWebView2_11 = core.cast()?;
            let identity = core.as_raw() as usize;
            if let Some(old) = HANDLERS.with(|handlers| handlers.borrow_mut().remove(&identity)) {
                let _ = menu_core.remove_ContextMenuRequested(old);
            }
            let environment: ICoreWebView2Environment9 = native.environment().cast()?;
            let handler = ContextMenuRequestedEventHandler::create(Box::new(move |sender, args| {
                let (Some(sender), Some(args)) = (sender, args) else { return Ok(()) };
                let deferral = args.GetDeferral()?;
                let completion = deferral.clone();
                let environment = environment.clone();
                let window = window.clone();
                let callback = ExecuteScriptCompletedHandler::create(Box::new(move |result, json| {
                    let append = (|| -> windows::core::Result<()> {
                        result?;
                        let id = serde_json::from_str::<String>(&json).unwrap_or_default();
                        if id.is_empty() || id.len() > 200 { return Ok(()) }
                        let items = args.MenuItems()?;
                        let download = environment.CreateContextMenuItem(w!("Create a download link"), None, COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND_COMMAND)?;
                        let download_window = window.clone();
                        let download_id = id.clone();
                        let selected_download = CustomItemSelectedEventHandler::create(Box::new(move |_, _| {
                            let detail = serde_json::to_string(&download_id).unwrap_or_default();
                            let _ = download_window.eval(&format!("window.dispatchEvent(new CustomEvent('yougori-download-node', {{detail:{detail}}}))"));
                            Ok(())
                        }));
                        let mut download_token = 0;
                        download.add_CustomItemSelected(&selected_download, &mut download_token)?;
                        let mut download_index = 0;
                        items.Count(&mut download_index)?;
                        items.InsertValueAtIndex(download_index, &download)?;
                        let item = environment.CreateContextMenuItem(w!("Delete node"), None, COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND_COMMAND)?;
                        let selected = CustomItemSelectedEventHandler::create(Box::new(move |_, _| {
                            // Only opens the existing UI confirmation; no deletion in native menu code.
                            let detail = serde_json::to_string(&id).unwrap_or_default();
                            let _ = window.eval(&format!("window.dispatchEvent(new CustomEvent('yougori-delete-node', {{detail:{detail}}}))"));
                            Ok(())
                        }));
                        let mut token = 0;
                        item.add_CustomItemSelected(&selected, &mut token)?;
                        let mut count = 0;
                        items.Count(&mut count)?;
                        items.InsertValueAtIndex(count, &item)?;
                        Ok(())
                    })();
                    // Always let the normal menu open, including unsupported targets/errors.
                    if let Err(error) = append { eprintln!("Node context menu: {error}"); }
                    completion.Complete()
                }));
                if let Err(error) = sender.ExecuteScript(&HSTRING::from("(() => { const id = document.documentElement.dataset.yougoriNodeContext || ''; delete document.documentElement.dataset.yougoriNodeContext; return id; })()"), &callback) {
                    let _ = deferral.Complete();
                    return Err(error);
                }
                Ok(())
            }));
            let mut token = 0;
            menu_core.add_ContextMenuRequested(&handler, &mut token)?;
            HANDLERS.with(|handlers| { handlers.borrow_mut().insert(identity, token); });
            Ok(())
        })();
        if let Err(error) = result { eprintln!("Native node menu unavailable: {error}"); }
    });
}
