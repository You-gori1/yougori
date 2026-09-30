use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager,
};

fn show_dashboard(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// A presence icon while Desktop is open; it never hides the window or prevents exit.
pub fn install(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "tray-open", "Open Yougori", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "tray-quit", "Quit Yougori…", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let mut builder = TrayIconBuilder::with_id("yougori")
        .tooltip("Yougori")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if matches!(event, TrayIconEvent::Click {
                button: MouseButton::Left, button_state: MouseButtonState::Up, ..
            }) {
                show_dashboard(tray.app_handle());
            }
        })
        .on_menu_event(|app, event| match event.id.as_ref() {
            "tray-open" => show_dashboard(app),
            "tray-quit" => {
                show_dashboard(app);
                if let Some(window) = app.get_webview_window("main") {
                    // Follow the title-bar close path, including its confirmation.
                    let _ = window.close();
                }
            }
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    // Tauri owns the tray resource until application shutdown.
    builder.build(app)?;
    Ok(())
}
