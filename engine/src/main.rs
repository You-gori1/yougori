//! `yougori-engine`: Yougori's engine with no dashboard, windows, tray or WebView, so it runs
//! without a display. The `yougori` CLI starts it when the desktop app is not installed. It
//! keeps the same state, environments and local control pipe as the app, and only one of the
//! two runs at a time; opening the app while nothing runs here hands the environments over.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    yougori_lib::run();
}
