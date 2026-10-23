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

/// Canonical config-style spelling of a parsed hotkey
/// ("ctrl+win+alt+space", "f9"). Modifier order follows the default config:
/// ctrl, win, alt, shift. Everything emitted here is accepted back by
/// [`parse_hotkey`] (round-trip invariant, covered by tests).
pub fn format_hotkey(hk: &HotKey) -> Result<String> {
    use global_hotkey::hotkey::Modifiers;
    let mut parts: Vec<String> = Vec::new();
    if hk.mods.contains(Modifiers::CONTROL) {
        parts.push("ctrl".into());
    }
    if hk.mods.contains(Modifiers::META) {
        parts.push("win".into());
    }
    if hk.mods.contains(Modifiers::ALT) {
        parts.push("alt".into());
    }
    if hk.mods.contains(Modifiers::SHIFT) {
        parts.push("shift".into());
    }
    parts.push(code_name(hk.key)?);
    Ok(parts.join("+"))
}

/// Lowercase name of a key code, exactly as [`parse_hotkey`] accepts it.
/// Errors on codes that cannot round-trip through the parser (modifier keys
/// used as the main key, media/browser keys): silently emitting an
/// unparseable name would break the config on next load.
pub fn code_name(c: global_hotkey::hotkey::Code) -> Result<String> {
    use global_hotkey::hotkey::Code as C;
    let name: &'static str = match c {
        C::KeyA => "a",
        C::KeyB => "b",
        C::KeyC => "c",
        C::KeyD => "d",
        C::KeyE => "e",
        C::KeyF => "f",
        C::KeyG => "g",
        C::KeyH => "h",
        C::KeyI => "i",
        C::KeyJ => "j",
        C::KeyK => "k",
        C::KeyL => "l",
        C::KeyM => "m",
        C::KeyN => "n",
        C::KeyO => "o",
        C::KeyP => "p",
        C::KeyQ => "q",
        C::KeyR => "r",
        C::KeyS => "s",
        C::KeyT => "t",
        C::KeyU => "u",
        C::KeyV => "v",
        C::KeyW => "w",
        C::KeyX => "x",
        C::KeyY => "y",
        C::KeyZ => "z",
        C::Digit0 => "0",
        C::Digit1 => "1",
        C::Digit2 => "2",
        C::Digit3 => "3",
        C::Digit4 => "4",
        C::Digit5 => "5",
        C::Digit6 => "6",
        C::Digit7 => "7",
        C::Digit8 => "8",
        C::Digit9 => "9",
        C::F1 => "f1",
        C::F2 => "f2",
        C::F3 => "f3",
        C::F4 => "f4",
        C::F5 => "f5",
        C::F6 => "f6",
        C::F7 => "f7",
        C::F8 => "f8",
        C::F9 => "f9",
        C::F10 => "f10",
        C::F11 => "f11",
        C::F12 => "f12",
        C::F13 => "f13",
        C::F14 => "f14",
        C::F15 => "f15",
        C::F16 => "f16",
        C::F17 => "f17",
        C::F18 => "f18",
        C::F19 => "f19",
        C::F20 => "f20",
        C::F21 => "f21",
        C::F22 => "f22",
        C::F23 => "f23",
        C::F24 => "f24",
        C::Space => "space",
        C::Enter => "enter",
        C::Tab => "tab",
        C::Backspace => "backspace",
        C::Escape => "escape",
        C::Comma => "comma",
        C::Period => "period",
        C::Slash => "slash",
        C::Semicolon => "semicolon",
        C::Quote => "quote",
        C::BracketLeft => "bracketleft",
        C::BracketRight => "bracketright",
        C::Backslash => "backslash",
        C::Backquote => "backquote",
        C::Minus => "minus",
        C::Equal => "equal",
        C::ArrowUp => "arrowup",
        C::ArrowDown => "arrowdown",
        C::ArrowLeft => "arrowleft",
        C::ArrowRight => "arrowright",
        C::Home => "home",
        C::End => "end",
        C::PageUp => "pageup",
        C::PageDown => "pagedown",
        C::Insert => "insert",
        C::Delete => "delete",
        C::Numpad0 => "numpad0",
        C::Numpad1 => "numpad1",
        C::Numpad2 => "numpad2",
        C::Numpad3 => "numpad3",
        C::Numpad4 => "numpad4",
        C::Numpad5 => "numpad5",
        C::Numpad6 => "numpad6",
        C::Numpad7 => "numpad7",
        C::Numpad8 => "numpad8",
        C::Numpad9 => "numpad9",
        C::NumpadAdd => "numpadadd",
        C::NumpadSubtract => "numpadsubtract",
        C::NumpadMultiply => "numpadmultiply",
        C::NumpadDivide => "numpaddivide",
        C::NumpadDecimal => "numpaddecimal",
        C::NumpadEnter => "numpadenter",
        _ => {
            return Err(Error::Hotkey(format!(
                "key '{c:?}' cannot be a hotkey main key"
            )))
        }
    };
    Ok(name.into())
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

    #[test]
    fn format_hotkey_canonical_modifier_order() {
        // Arrange: any modifier order parses.
        let hk = parse_hotkey("alt+ctrl+shift+f9").unwrap();
        // Act.
        let s = format_hotkey(&hk).unwrap();
        // Assert: canonical order matches the default config style.
        assert_eq!(s, "ctrl+alt+shift+f9");
    }

    #[test]
    fn format_hotkey_uses_win_alias() {
        // Arrange.
        let hk = parse_hotkey("ctrl+win+alt+space").unwrap();
        // Act.
        let s = format_hotkey(&hk).unwrap();
        // Assert.
        assert_eq!(s, "ctrl+win+alt+space");
    }

    #[test]
    fn format_parse_roundtrip_matrix() {
        // Arrange: one entry per key class (letters, digits, f-keys, punctuation,
        // arrows, navigation, numpad).
        let names = [
            "a",
            "ctrl+alt+space",
            "ctrl+win+alt+space",
            "shift+comma",
            "ctrl+f12",
            "ctrl+shift+arrowup",
            "ctrl+home",
            "alt+numpad5",
        ];
        for name in names {
            // Act.
            let parsed = parse_hotkey(name).expect("must parse");
            let formatted = format_hotkey(&parsed).expect("must format");
            let reparsed = parse_hotkey(&formatted).expect("must reparse");
            // Assert: identity through parse -> format -> parse.
            assert_eq!(parsed, reparsed, "roundtrip failed for '{name}'");
        }
    }

    #[test]
    fn code_name_covers_common_keys() {
        use global_hotkey::hotkey::Code;
        // Arrange/Act: sample across key classes.
        let pairs = [
            (Code::KeyA, "a"),
            (Code::Digit7, "7"),
            (Code::F12, "f12"),
            (Code::Space, "space"),
            (Code::Enter, "enter"),
            (Code::Escape, "escape"),
            (Code::Comma, "comma"),
            (Code::ArrowUp, "arrowup"),
            (Code::Numpad5, "numpad5"),
        ];
        // Assert.
        for (code, expected) in pairs {
            assert_eq!(code_name(code).unwrap(), expected);
        }
    }

    #[test]
    fn format_rejects_modifier_as_main_key() {
        use global_hotkey::hotkey::{Code, Modifiers};
        // Arrange: AltLeft as the *main* key cannot be expressed by the
        // parser ("alt" is treated as a modifier token).
        let hk = HotKey::new(Some(Modifiers::CONTROL), Code::AltLeft);
        // Act.
        let formatted = format_hotkey(&hk);
        // Assert: loud error instead of an unparseable config value.
        assert!(formatted.is_err());
    }
}
