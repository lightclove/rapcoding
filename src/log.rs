use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::error::Result;

const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;

#[derive(Clone, Copy, PartialEq, PartialOrd)]
pub enum Level {
    Warn = 0,
    Info = 1,
    Debug = 2,
}

#[derive(Clone)]
pub struct Logger {
    inner: Arc<Mutex<Inner>>,
}

struct Inner {
    file: Option<File>,
    path: PathBuf,
    level: Level,
    start: Instant,
    log_transcripts: bool,
    live: Option<LiveLog>,
}

impl Logger {
    pub fn new_with_live(
        path: PathBuf,
        level: Level,
        log_transcripts: bool,
        live: Option<LiveLog>,
    ) -> Result<Self> {
        let mut inner = Inner {
            file: None,
            path,
            level,
            start: Instant::now(),
            log_transcripts,
            live,
        };
        inner.rotate_if_needed()?;
        Ok(Logger {
            inner: Arc::new(Mutex::new(inner)),
        })
    }

    pub fn info(&self, msg: &str) {
        self.log(Level::Info, msg);
    }
    pub fn warn(&self, msg: &str) {
        self.log(Level::Warn, msg);
    }
    pub fn debug(&self, msg: &str) {
        self.log(Level::Debug, msg);
    }

    /// Log a transcript only if explicitly enabled (privacy default is off).
    pub fn transcript(&self, msg: &str) {
        // Scope the guard explicitly: `self.info` re-acquires the same mutex,
        // and a guard held across that call would deadlock.
        let enabled = {
            let inner = crate::sync::lock_recover(&self.inner);
            inner.log_transcripts
        };
        if enabled {
            self.info(msg);
        }
    }

    fn log(&self, level: Level, msg: &str) {
        let mut inner = crate::sync::lock_recover(&self.inner);
        if level > inner.level {
            return;
        }
        let secs = inner.start.elapsed().as_secs_f64();
        let line = format!("[{:>9.3}s] {:<5} {}", secs, level_name(level), sanitize(msg));
        write_line(&mut std::io::stderr(), &line);
        if let Some(live) = &inner.live {
            live.push(&line);
        }
        let _ = inner.rotate_if_needed();
        if let Some(f) = inner.file.as_mut() {
            write_line(f, &line);
            let _ = f.flush();
        }
    }
}

impl Inner {
    fn rotate_if_needed(&mut self) -> Result<()> {
        let too_big = self.path.exists()
            && fs::metadata(&self.path)
                .map(|m| m.len() > MAX_LOG_BYTES)
                .unwrap_or(false);
        if too_big {
            let bak = self.path.with_extension("log.1");
            let _ = fs::remove_file(&bak);
            let _ = fs::rename(&self.path, &bak);
            // Drop the open handle so the current log is recreated fresh below;
            // otherwise new lines keep appending to the rotated file forever.
            self.file = None;
        }
        if self.file.is_none() {
            if let Some(p) = self.path.parent() {
                let _ = fs::create_dir_all(p);
            }
            let f = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)?;
            self.file = Some(f);
        }
        Ok(())
    }
}

fn level_name(l: Level) -> &'static str {
    match l {
        Level::Warn => "WARN",
        Level::Info => "INFO",
        Level::Debug => "DEBUG",
    }
}

/// Escape control characters so a hostile/malformed input (a device name or a
/// transcript) cannot forge log lines or inject terminal escape sequences.
pub(crate) fn sanitize(msg: &str) -> String {
    let mut out = String::with_capacity(msg.len());
    for c in msg.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || (0x7f..=0x9f).contains(&(c as u32)) => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

/// Write a line to a sink, swallowing I/O errors so a broken pipe or a closed
/// stream can never panic a caller (the daemon loop or the audio thread).
pub(crate) fn write_line<W: std::io::Write>(w: &mut W, line: &str) {
    let _ = writeln!(w, "{line}");
}

/// Ring buffer of recent log lines, shared with the GUI for live display.
#[derive(Clone)]
pub struct LiveLog {
    inner: Arc<Mutex<VecDeque<String>>>,
    cap: usize,
}

impl LiveLog {
    #[cfg(feature = "gui")]
    pub fn new(cap: usize) -> Self {
        LiveLog {
            inner: Arc::new(Mutex::new(VecDeque::new())),
            cap: cap.max(1),
        }
    }

    /// Append a line, dropping the oldest when at capacity.
    pub fn push(&self, line: &str) {
        let mut q = crate::sync::lock_recover(&self.inner);
        while q.len() >= self.cap {
            q.pop_front();
        }
        q.push_back(line.to_string());
    }

    /// Oldest-first snapshot of the buffered lines.
    #[cfg(feature = "gui")]
    pub fn snapshot(&self) -> Vec<String> {
        crate::sync::lock_recover(&self.inner).iter().cloned().collect()
    }
}

/// Copy the current log file to a user-chosen destination (GUI "export").
#[cfg(feature = "gui")]
pub fn export_log(src: &std::path::Path, dst: &std::path::Path) -> Result<()> {
    std::fs::copy(src, dst)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_reopens_fresh_file() {
        let dir = std::env::temp_dir().join("rapcoding-log-rot");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rapcoding.log");
        fs::write(&path, vec![b'x'; (MAX_LOG_BYTES as usize) + 1]).unwrap();
        let f = OpenOptions::new().append(true).open(&path).unwrap();
        let mut inner = Inner {
            file: Some(f),
            path: path.clone(),
            level: Level::Info,
            start: Instant::now(),
            log_transcripts: false,
            live: None,
        };
        inner.rotate_if_needed().unwrap();
        assert!(path.exists(), "current log must be recreated after rotation");
        assert!(
            dir.join("rapcoding.log.1").exists(),
            "old log must be rotated away"
        );
        if let Some(f) = inner.file.as_mut() {
            let _ = writeln!(f, "AFTER_ROTATE");
            let _ = f.flush();
        }
        let current = fs::read_to_string(&path).unwrap();
        assert!(
            current.contains("AFTER_ROTATE"),
            "new lines must land in the current log, not the rotated one"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn log_recovers_from_poison() {
        let dir = std::env::temp_dir().join("rapcoding-log-poison");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rapcoding.log");
        let logger = Logger::new_with_live(path.clone(), Level::Info, false, None).unwrap();
        let inner = logger.inner.clone();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _g = inner.lock().unwrap();
            panic!("poison");
        }));
        logger.info("after poison");
        let content = fs::read_to_string(&path).unwrap_or_default();
        assert!(content.contains("after poison"), "log must recover from a poisoned mutex");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn log_sanitizes_control_chars() {
        let dir = std::env::temp_dir().join("rapcoding-log-sanitize");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rapcoding.log");
        let logger = Logger::new_with_live(path.clone(), Level::Info, false, None).unwrap();
        logger.info("line1\nline2\x1b[31mRED\tend");
        let content = fs::read_to_string(&path).unwrap_or_default();
        assert_eq!(content.lines().count(), 1, "one log entry = one line (no raw newline)");
        assert!(!content.contains('\x1b'), "no raw escape sequences");
        assert!(content.contains("\\n"), "newline must be escaped");
        assert!(content.contains("\\x1b"), "escape must be hex-escaped");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn sanitize_escapes_control_chars() {
        assert_eq!(sanitize("a\nb"), "a\\nb");
        assert_eq!(sanitize("x\t\r"), "x\\t\\r");
        assert_eq!(sanitize("a\x1b[31m"), "a\\x1b[31m");
        assert_eq!(sanitize("plain"), "plain");
    }

    struct BrokenWriter;

    impl std::io::Write for BrokenWriter {
        fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "broken pipe",
            ))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "broken pipe",
            ))
        }
    }

    #[test]
    fn write_line_never_panics_on_broken_writer() {
        let mut w = BrokenWriter;
        // Must not panic even though every write fails.
        write_line(&mut w, "hello");
        write_line(&mut w, "a\nb\x1b[31m");
    }

    #[test]
    fn transcript_not_logged_by_default() {
        let dir = std::env::temp_dir().join("rapcoding-log-priv");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rapcoding.log");
        let logger = Logger::new_with_live(path.clone(), Level::Info, false, None).unwrap();
        logger.transcript("SECRET");
        let content = fs::read_to_string(&path).unwrap_or_default();
        assert!(!content.contains("SECRET"), "transcripts must not be logged by default");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn transcript_logged_when_enabled() {
        let dir = std::env::temp_dir().join("rapcoding-log-priv2");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rapcoding.log");
        let logger = Logger::new_with_live(path.clone(), Level::Info, true, None).unwrap();
        logger.transcript("HELLO");
        let content = fs::read_to_string(&path).unwrap_or_default();
        assert!(content.contains("HELLO"), "transcripts must be logged when enabled");
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn live_log_caps_at_limit() {
        let live = LiveLog::new(3);
        live.push("1");
        live.push("2");
        live.push("3");
        live.push("4");
        assert_eq!(
            live.snapshot(),
            vec!["2".to_string(), "3".to_string(), "4".to_string()]
        );
    }

    #[cfg(feature = "gui")]
    #[test]
    fn live_log_keeps_order() {
        let live = LiveLog::new(10);
        live.push("a");
        live.push("b");
        assert_eq!(live.snapshot(), vec!["a".to_string(), "b".to_string()]);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn logger_feeds_live_log() {
        let dir = std::env::temp_dir().join("rapcoding-log-live");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rapcoding.log");
        let live = LiveLog::new(100);
        let logger =
            Logger::new_with_live(path.clone(), Level::Info, false, Some(live.clone())).unwrap();
        logger.info("hello live");
        let snap = live.snapshot();
        assert!(snap.iter().any(|l| l.contains("hello live")));
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn export_log_copies_content() {
        let dir = std::env::temp_dir().join("rapcoding-log-export");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let src = dir.join("a.log");
        let dst = dir.join("b.log");
        fs::write(&src, "hello\nexport\n").unwrap();
        export_log(&src, &dst).unwrap();
        assert_eq!(fs::read_to_string(&dst).unwrap(), "hello\nexport\n");
        let _ = fs::remove_dir_all(&dir);
    }
}
