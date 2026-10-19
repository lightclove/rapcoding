use std::str::FromStr;
use std::sync::mpsc::Sender;
use std::thread::JoinHandle;
use std::time::Duration;

use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

use crate::error::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HotEvent {
    Pressed,
    Released,
}

/// Message sent from the hotkey thread to the main loop.
#[derive(Debug)]
pub enum HotMsg {
    Event(HotEvent),
    /// Registration/thread failure — surfaces so the app does not run silent.
    Fatal(String),
}

/// Parse a hotkey string via global-hotkey's built-in `FromStr`
/// (supports "ctrl+alt+space", "f9", "cmd+shift+p", …).
pub fn parse_hotkey(s: &str) -> Result<HotKey> {
    HotKey::from_str(s).map_err(|e| Error::Hotkey(format!("invalid hotkey '{s}': {e}")))
}

/// Spawn the hotkey manager + message loop on a dedicated thread.
/// On Windows a Win32 message pump is required for `WM_HOTKEY` delivery.
pub fn spawn_hotkey(hotkey: &str, tx: Sender<HotMsg>) -> Result<JoinHandle<()>> {
    let hotkey = parse_hotkey(hotkey)?;

    std::thread::Builder::new()
        .name("rapcoding-hotkey".into())
        .spawn(move || {
            let manager = match GlobalHotKeyManager::new() {
                Ok(m) => m,
                Err(e) => {
                    let _ = tx.send(HotMsg::Fatal(format!("hotkey manager: {e}")));
                    return;
                }
            };
            if let Err(e) = manager.register(hotkey) {
                let _ = tx.send(HotMsg::Fatal(format!("hotkey register: {e}")));
                return;
            }
            loop {
                pump_messages();
                while let Ok(ev) = GlobalHotKeyEvent::receiver().try_recv() {
                    let state = if ev.state == HotKeyState::Pressed {
                        HotEvent::Pressed
                    } else {
                        HotEvent::Released
                    };
                    if tx.send(HotMsg::Event(state)).is_err() {
                        return;
                    }
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        })
        .map_err(|e| Error::Hotkey(format!("spawn thread: {e}")))
}

#[cfg(target_os = "windows")]
fn pump_messages() {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE, WM_QUIT,
    };
    let mut msg: MSG = unsafe { std::mem::zeroed() };
    unsafe {
        while PeekMessageW(&mut msg, 0 as HWND, 0, 0, PM_REMOVE) != 0 {
            if msg.message == WM_QUIT {
                break;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    std::thread::sleep(Duration::from_millis(5));
}

#[cfg(not(target_os = "windows"))]
fn pump_messages() {
    // Linux X11: global-hotkey delivers events without an extra pump.
    // macOS: the manager must run on the main thread (see README limitation).
    std::thread::sleep(Duration::from_millis(5));
}
