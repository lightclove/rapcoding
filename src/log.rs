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
}

impl Logger {
    pub fn new(path: PathBuf, level: Level, log_transcripts: bool) -> Result<Self> {
        let mut inner = Inner {
            file: None,
            path,
            level,
            start: Instant::now(),
            log_transcripts,
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
        if let Ok(inner) = self.inner.lock() {
            if inner.log_transcripts {
                drop(inner);
                self.info(msg);
            }
        }
    }

    fn log(&self, level: Level, msg: &str) {
        if let Ok(mut inner) = self.inner.lock() {
            if level > inner.level {
                return;
            }
            let secs = inner.start.elapsed().as_secs_f64();
            let line = format!("[{:>9.3}s] {:<5} {}", secs, level_name(level), msg);
            eprintln!("{line}");
            let _ = inner.rotate_if_needed();
            if let Some(f) = inner.file.as_mut() {
                let _ = writeln!(f, "{line}");
                let _ = f.flush();
            }
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
