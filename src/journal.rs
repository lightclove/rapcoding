//! Persistent management journal: a timestamped, append-only event log
//! (session start/stop, config changes, model downloads). Read by the GUI.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

#[derive(Clone)]
pub struct Journal {
    path: PathBuf,
}

impl Journal {
    pub fn new(path: PathBuf) -> Self {
        Journal { path }
    }

    /// Append one event (best-effort: a journal failure must never break the
    /// daemon). Messages are sanitized so they stay on a single line.
    pub fn append(&self, kind: &str, msg: &str) {
        let line = format!(
            "[{}] {} {}",
            timestamp(),
            kind,
            crate::log::sanitize(msg)
        );
        if let Some(p) = self.path.parent() {
            let _ = fs::create_dir_all(p);
        }
        if let Ok(mut f) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        {
            let _ = writeln!(f, "{line}");
        }
    }

    /// Newest-last list of entries (empty when the journal is absent).
    pub fn read(&self) -> Vec<String> {
        fs::read_to_string(&self.path)
            .map(|s| s.lines().map(str::to_string).collect())
            .unwrap_or_default()
    }
}

fn timestamp() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "unknown".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rapcoding-{name}"));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn append_and_read_roundtrip() {
        let dir = tmp("journal-roundtrip");
        let j = Journal::new(dir.join("journal.log"));
        j.append("config", "theme -> dark");
        j.append("session", "start");
        let lines = j.read();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("config") && lines[0].contains("theme -> dark"));
        assert!(lines[1].contains("session") && lines[1].contains("start"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn append_sanitizes_newlines() {
        let dir = tmp("journal-sanitize");
        let j = Journal::new(dir.join("journal.log"));
        j.append("config", "a\nb");
        let lines = j.read();
        assert_eq!(lines.len(), 1, "one event must stay on one line");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_missing_journal_is_empty() {
        let dir = tmp("journal-missing");
        let j = Journal::new(dir.join("journal.log"));
        assert!(j.read().is_empty());
        let _ = fs::remove_dir_all(&dir);
    }
}
