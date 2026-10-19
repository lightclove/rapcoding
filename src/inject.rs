use std::time::Duration;

use enigo::{Direction, Enigo, Key, Keyboard, Settings};

use crate::error::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PasteChord {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub meta: bool,
    pub key: char,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Method {
    Paste,
    Type,
}

#[derive(Clone)]
pub struct Injector {
    enabled: bool,
    method: Method,
    chord: PasteChord,
    restore_clipboard: bool,
    settle: Duration,
}

impl Injector {
    pub fn new(
        enabled: bool,
        method: &str,
        paste_shortcut: &str,
        restore_clipboard: bool,
        settle_ms: u64,
    ) -> Result<Self> {
        let method = match method {
            "paste" => Method::Paste,
            "type" => Method::Type,
            other => {
                return Err(Error::Inject(format!(
                    "inject.method must be 'paste' or 'type', got '{other}'"
                )));
            }
        };
        let chord = if paste_shortcut.is_empty() {
            default_chord()
        } else {
            parse_chord(paste_shortcut)?
        };
        Ok(Injector {
            enabled,
            method,
            chord,
            restore_clipboard,
            settle: Duration::from_millis(settle_ms),
        })
    }

    /// Inject text at the cursor. Returns a human-readable summary of what
    /// happened (used for logging). On failure the text is left in the
    /// clipboard so the user can paste manually.
    pub fn inject(&self, text: &str) -> Result<String> {
        if !self.enabled {
            return Ok("injection disabled".into());
        }
        match self.method {
            Method::Type => {
                let mut e = Enigo::new(&Settings::default())
                    .map_err(|e| Error::Inject(format!("enigo init: {e}")))?;
                e.text(text)
                    .map_err(|e| Error::Inject(format!("typing: {e}")))?;
                Ok("typed".into())
            }
            Method::Paste => {
                let mut clip = arboard::Clipboard::new()
                    .map_err(|e| Error::Inject(format!("clipboard: {e}")))?;
                let prev: Option<String> = clip.get_text().ok();
                let had_text = prev.is_some();
                clip.set_text(text)
                    .map_err(|e| Error::Inject(format!("set clipboard: {e}")))?;

                let mut e = Enigo::new(&Settings::default())
                    .map_err(|e| Error::Inject(format!("enigo init: {e}")))?;
                send_chord(&mut e, self.chord)?;
                std::thread::sleep(self.settle);

                let note = if self.restore_clipboard {
                    if had_text {
                        let _ = clip.set_text(prev.as_deref().unwrap_or(""));
                        "clipboard restored".to_string()
                    } else {
                        "clipboard left as text (no prior text to restore)".to_string()
                    }
                } else {
                    "clipboard left as recognized text".to_string()
                };
                Ok(format!("paste ({})", note))
            }
        }
    }
}

pub fn parse_chord(s: &str) -> Result<PasteChord> {
    let mut ctrl = false;
    let mut shift = false;
    let mut alt = false;
    let mut meta = false;
    let mut key: Option<char> = None;

    for raw in s.split('+') {
        let part = raw.trim().to_ascii_lowercase();
        match part.as_str() {
            "ctrl" | "control" => ctrl = true,
            "shift" => shift = true,
            "alt" | "option" => alt = true,
            "meta" | "cmd" | "super" | "command" => meta = true,
            "" => {}
            p => {
                let mut ch = p.chars();
                if let (Some(c), None) = (ch.next(), ch.next()) {
                    if c.is_ascii_alphanumeric() {
                        key = Some(c.to_ascii_lowercase());
                        continue;
                    }
                }
                return Err(Error::Inject(format!(
                    "invalid paste shortcut '{s}': unknown token '{p}'"
                )));
            }
        }
    }
    let key = key.ok_or_else(|| Error::Inject(format!("paste shortcut '{s}' has no key")))?;
    Ok(PasteChord {
        ctrl,
        shift,
        alt,
        meta,
        key,
    })
}

pub fn default_chord() -> PasteChord {
    #[cfg(target_os = "macos")]
    {
        parse_chord("cmd+v").expect("static chord")
    }
    #[cfg(not(target_os = "macos"))]
    {
        parse_chord("ctrl+v").expect("static chord")
    }
}

fn send_chord(e: &mut Enigo, c: PasteChord) -> Result<()> {
    if c.ctrl {
        e.key(Key::Control, Direction::Press)
            .map_err(|err| Error::Inject(format!("keyboard: {err}")))?;
    }
    if c.meta {
        e.key(Key::Meta, Direction::Press)
            .map_err(|err| Error::Inject(format!("keyboard: {err}")))?;
    }
    if c.alt {
        e.key(Key::Alt, Direction::Press)
            .map_err(|err| Error::Inject(format!("keyboard: {err}")))?;
    }
    if c.shift {
        e.key(Key::Shift, Direction::Press)
            .map_err(|err| Error::Inject(format!("keyboard: {err}")))?;
    }
    e.key(Key::Unicode(c.key), Direction::Click)
        .map_err(|err| Error::Inject(format!("keyboard: {err}")))?;
    if c.shift {
        e.key(Key::Shift, Direction::Release)
            .map_err(|err| Error::Inject(format!("keyboard: {err}")))?;
    }
    if c.alt {
        e.key(Key::Alt, Direction::Release)
            .map_err(|err| Error::Inject(format!("keyboard: {err}")))?;
    }
    if c.meta {
        e.key(Key::Meta, Direction::Release)
            .map_err(|err| Error::Inject(format!("keyboard: {err}")))?;
    }
    if c.ctrl {
        e.key(Key::Control, Direction::Release)
            .map_err(|err| Error::Inject(format!("keyboard: {err}")))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ctrl_v() {
        let c = parse_chord("ctrl+v").unwrap();
        assert!(c.ctrl && !c.shift && !c.meta && !c.alt && c.key == 'v');
    }

    #[test]
    fn parse_cmd_shift_v() {
        let c = parse_chord("cmd+shift+v").unwrap();
        assert!(c.meta && c.shift && c.key == 'v');
    }

    #[test]
    fn parse_rejects_bad_token() {
        assert!(parse_chord("ctrl+f11").is_err());
    }

    #[test]
    fn default_chord_is_valid() {
        let c = default_chord();
        assert!(c.key == 'v');
    }
}
