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
    /// A hotkey event; `id` is the index of the binding in `spawn_hotkeys`.
    Event { id: usize, state: HotEvent },
    /// Registration/thread failure — surfaces so the app does not run silent.
    Fatal(String),
}

/// Parse a hotkey string via global-hotkey's built-in `FromStr`
/// (supports "ctrl+alt+space", "f9", "cmd+shift+p", …).
/// "win" is accepted as an alias for the Windows/Command key.
pub fn parse_hotkey(s: &str) -> Result<HotKey> {
    let translated: String = s
        .split('+')
        .map(|t| {
            if t.trim().eq_ignore_ascii_case("win") {
                "super"
            } else {
                t
            }
        })
        .collect::<Vec<_>>()
        .join("+");
    HotKey::from_str(&translated).map_err(|e| Error::Hotkey(format!("invalid hotkey '{s}': {e}")))
}

/// Spawn the hotkey manager + message loop on a dedicated thread.
/// On Windows a Win32 message pump is required for `WM_HOTKEY` delivery.
pub fn spawn_hotkeys(bindings: &[String], tx: Sender<HotMsg>) -> Result<JoinHandle<()>> {
    let hotkeys: Vec<HotKey> = bindings
        .iter()
        .map(|s| parse_hotkey(s))
        .collect::<Result<_>>()?;

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
            for hk in &hotkeys {
                if let Err(e) = manager.register(*hk) {
                    let _ = tx.send(HotMsg::Fatal(format!("hotkey register: {e}")));
                    return;
                }
            }
            loop {
                pump_messages();
                while let Ok(ev) = GlobalHotKeyEvent::receiver().try_recv() {
                    let Some(id) = hotkeys.iter().position(|hk| hk.id() == ev.id) else {
                        continue;
                    };
                    let state = if ev.state == HotKeyState::Pressed {
                        HotEvent::Pressed
                    } else {
                        HotEvent::Released
                    };
                    if tx.send(HotMsg::Event { id, state }).is_err() {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn win_alias_parses_as_meta() {
        let hk = parse_hotkey("ctrl+win+alt+space").unwrap();
        use global_hotkey::hotkey::Modifiers;
        assert!(hk.mods.contains(Modifiers::META));
        assert!(hk.mods.contains(Modifiers::CONTROL));
        assert!(hk.mods.contains(Modifiers::ALT));
    }

    #[test]
    fn bad_order_rejected() {
        assert!(parse_hotkey("ctrl+space+alt").is_err());
    }

    #[test]
    fn plain_combo_still_works() {
        assert!(parse_hotkey("ctrl+alt+space").is_ok());
        assert!(parse_hotkey("f9").is_ok());
    }
}
