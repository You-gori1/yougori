//! Capture only the Windows keys, and only over the focused guest display.
//! No ordinary keystrokes are captured, stored, or sent through this module.
use serde::Deserialize;
use crate::WebviewWindow;

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureBounds {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

impl CaptureBounds {
    fn valid(self) -> bool {
        [self.left, self.top, self.right, self.bottom]
            .into_iter()
            .all(|n| n.is_finite() && (0.0..=100_000.0).contains(&n))
            && self.right > self.left
            && self.bottom > self.top
    }
    #[cfg(any(windows, test))]
    fn contains(self, x: i32, y: i32) -> bool {
        f64::from(x) >= self.left
            && f64::from(x) < self.right
            && f64::from(y) >= self.top
            && f64::from(y) < self.bottom
    }
}

#[tauri::command]
pub fn set_guest_keyboard_capture(
    window: WebviewWindow,
    token: String,
    bounds: Option<CaptureBounds>,
) -> Result<(), String> {
    if !window.label().starts_with("environment-env-") || token.is_empty() || token.len() > 100 {
        return Err("Keyboard capture is only available in a guest window".into());
    }
    if bounds.is_some_and(|rect| !rect.valid()) {
        return Err("Invalid guest display bounds".into());
    }
    #[cfg(windows)]
    return native::set(window, token, bounds);
    #[cfg(not(windows))]
    {
        let _ = (window, token, bounds);
        Ok(())
    }
}

pub fn release_window(label: &str) {
    #[cfg(windows)]
    native::release(label);
    #[cfg(not(windows))]
    let _ = label;
}

#[cfg(windows)]
mod native {
    use super::*;
    use serde::Serialize;
    use std::sync::{mpsc, Mutex, OnceLock};
    use tauri::Emitter;
    use windows_sys::Win32::{
        Foundation::{LPARAM, LRESULT, POINT, WPARAM},
        Graphics::Gdi::ScreenToClient,
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            CallNextHookEx, GetCursorPos, GetForegroundWindow, GetMessageW, SetWindowsHookExW,
            UnhookWindowsHookEx, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP,
            WM_SYSKEYDOWN, WM_SYSKEYUP,
        },
    };

    #[derive(Clone)]
    struct Target {
        window: WebviewWindow,
        hwnd: usize,
        token: String,
        bounds: CaptureBounds,
    }
    #[derive(Clone, Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Key {
        token: String,
        code: &'static str,
        keysym: u32,
        down: bool,
    }
    #[derive(Default)]
    struct Capture {
        target: Option<Target>,
        pressed: [Option<Target>; 2],
    }
    static CAPTURE: Mutex<Capture> = Mutex::new(Capture {
        target: None,
        pressed: [None, None],
    });
    static EVENTS: OnceLock<mpsc::Sender<(WebviewWindow, Key)>> = OnceLock::new();
    static STARTED: OnceLock<Result<(), String>> = OnceLock::new();

    fn emit(target: &Target, index: usize, down: bool) {
        if let Some(sender) = EVENTS.get() {
            let _ = sender.send((
                target.window.clone(),
                Key {
                    token: target.token.clone(),
                    code: if index == 0 { "MetaLeft" } else { "MetaRight" },
                    keysym: if index == 0 { 0xffeb } else { 0xffec },
                    down,
                },
            ));
        }
    }

    unsafe extern "system" fn hook(code: i32, message: WPARAM, data: LPARAM) -> LRESULT {
        if code >= 0
            && matches!(
                message as u32,
                WM_KEYDOWN | WM_SYSKEYDOWN | WM_KEYUP | WM_SYSKEYUP
            )
        {
            let key = &*(data as *const KBDLLHOOKSTRUCT);
            let index = match key.vkCode {
                0x5b => Some(0),
                0x5c => Some(1),
                _ => None,
            };
            if let Some(index) = index {
                // Never block the Windows low-level hook on IPC or a mutex.
                if let Ok(mut state) = CAPTURE.try_lock() {
                    let down = matches!(message as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
                    if !down {
                        if let Some(target) = state.pressed[index].take() {
                            emit(&target, index, false);
                            return 1;
                        }
                    } else if state.pressed[index].is_some() {
                        return 1; // Repeats must not leak to the host after leaving the guest.
                    } else if let Some(target) = state.target.clone() {
                        let hwnd = target.hwnd as _;
                        let mut point = POINT { x: 0, y: 0 };
                        if GetForegroundWindow() == hwnd
                            && GetCursorPos(&mut point) != 0
                            && ScreenToClient(hwnd, &mut point) != 0
                            && target.bounds.contains(point.x, point.y)
                        {
                            emit(&target, index, true);
                            state.pressed[index] = Some(target);
                            return 1;
                        }
                    }
                }
            }
        }
        CallNextHookEx(std::ptr::null_mut(), code, message, data)
    }

    fn start() -> Result<(), String> {
        STARTED
            .get_or_init(|| {
                let (sender, receiver) = mpsc::channel::<(WebviewWindow, Key)>();
                EVENTS
                    .set(sender)
                    .map_err(|_| "Keyboard capture already initialized")?;
                std::thread::Builder::new()
                    .name("guest-key-events".into())
                    .spawn(move || {
                        for (window, key) in receiver {
                            let _ = window.emit_to(window.label(), "guest-system-key", key);
                        }
                    })
                    .map_err(|e| e.to_string())?;
                let (ready, result) = mpsc::sync_channel(1);
                std::thread::Builder::new()
                    .name("guest-key-hook".into())
                    .spawn(move || unsafe {
                        let handle = SetWindowsHookExW(
                            WH_KEYBOARD_LL,
                            Some(hook),
                            GetModuleHandleW(std::ptr::null()),
                            0,
                        );
                        if handle.is_null() {
                            let _ = ready.send(Err(format!(
                                "Windows keyboard capture: {}",
                                std::io::Error::last_os_error()
                            )));
                            return;
                        }
                        let _ = ready.send(Ok(()));
                        let mut message: MSG = std::mem::zeroed();
                        while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {}
                        UnhookWindowsHookEx(handle);
                    })
                    .map_err(|e| e.to_string())?;
                result.recv().map_err(|e| e.to_string())?
            })
            .clone()
    }

    pub(super) fn set(
        window: WebviewWindow,
        token: String,
        bounds: Option<CaptureBounds>,
    ) -> Result<(), String> {
        if bounds.is_some() {
            start()?;
        }
        let hwnd = if bounds.is_some() {
            let hwnd = window.hwnd().map_err(|e| e.to_string())?.0 as usize;
            // An old window's delayed IPC must not steal capture from the newly
            // focused window. The hook checks this again at the actual key-down.
            if unsafe { GetForegroundWindow() } as usize != hwnd {
                return Ok(());
            }
            hwnd
        } else {
            0
        };
        let mut state = CAPTURE
            .lock()
            .map_err(|_| "Keyboard capture is unavailable")?;
        if let Some(bounds) = bounds {
            state.target = Some(Target {
                window,
                hwnd,
                token,
                bounds,
            });
        } else if state
            .target
            .as_ref()
            .is_some_and(|t| t.window.label() == window.label() && t.token == token)
        {
            state.target = None;
        }
        Ok(())
    }

    pub(super) fn release(label: &str) {
        if let Ok(mut state) = CAPTURE.lock() {
            if state
                .target
                .as_ref()
                .is_some_and(|t| t.window.label() == label)
            {
                state.target = None;
            }
            for (index, target) in state.pressed.iter().enumerate() {
                if let Some(target) = target.as_ref().filter(|t| t.window.label() == label) {
                    emit(target, index, false);
                }
            }
            // Keep physical key-up ownership until key-up, to avoid opening Start on the host.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capture_is_bounded_to_guest_pixels() {
        let rect = CaptureBounds {
            left: 0.0,
            top: 80.0,
            right: 1200.0,
            bottom: 760.0,
        };
        assert!(rect.valid());
        assert!(rect.contains(600, 400));
        for (x, y) in [(1, 79), (-1, 100), (1200, 100), (100, 760)] {
            assert!(!rect.contains(x, y));
        }
        assert!(!CaptureBounds {
            right: f64::NAN,
            ..rect
        }
        .valid());
        assert!(!CaptureBounds {
            bottom: 10.0,
            ..rect
        }
        .valid());
    }
}
