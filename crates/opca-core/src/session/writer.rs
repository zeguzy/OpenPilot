//! Append-only JSONL session writer (Task 15.2).
//!
//! [`SessionWriter`] owns a [`std::fs::File`] opened in append mode and writes
//! one [`SessionEntry`] per line. Append-only means a crash mid-session
//! truncates at most the final line — earlier entries are durable. Each
//! successful [`SessionWriter::append`] increments the running message count
//! so the `SQLite` index (see `index.rs`) can be kept in sync without
//! re-scanning the file.
//!
//! File location convention: `<agent_dir>/sessions/<session-id>.jsonl` where
//! `agent_dir` is typically `<project>/.agent`.

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::Path;

use anyhow::{Context, Result};

use super::format::SessionEntry;

/// The subdirectory under the agent data dir that holds session JSONL files.
pub const SESSIONS_DIR: &str = "sessions";

/// Append-only JSONL writer for a single session.
///
/// Created via [`SessionWriter::create`], which opens `<session_dir>/<session_id>.jsonl`
/// for appending (creating the file and parent directories as needed). Use
/// [`SessionWriter::append`] to emit entries and [`SessionWriter::flush`] to
/// force a durable boundary (e.g. before a Task is handed off to another
/// process).
pub struct SessionWriter {
    writer: BufWriter<File>,
    session_id: String,
    message_count: u64,
}

impl SessionWriter {
    /// Open (or create) the JSONL file for `session_id` under `session_dir`.
    ///
    /// Parent directories are created if missing. The file is opened in
    /// append mode so reopening an existing session picks up where it left
    /// off; in that case the in-memory `message_count` starts at zero and
    /// should be reconciled against the `SQLite` index by the caller.
    pub fn create(session_dir: &Path, session_id: &str) -> Result<Self> {
        std::fs::create_dir_all(session_dir)
            .with_context(|| format!("creating session dir {}", session_dir.display()))?;

        let path = session_dir.join(format!("{session_id}.jsonl"));
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("opening session file {}", path.display()))?;

        Ok(Self {
            writer: BufWriter::new(file),
            session_id: session_id.to_string(),
            message_count: 0,
        })
    }

    /// Append a single entry as one JSON line. The line is flushed to the
    /// OS buffer but not necessarily fsynced — call [`Self::flush`] for that.
    pub fn append(&mut self, entry: &SessionEntry) -> Result<()> {
        let line = serde_json::to_string(entry).context("serializing session entry")?;
        writeln!(self.writer, "{line}").context("writing session entry")?;
        self.message_count = self.message_count.saturating_add(1);
        Ok(())
    }

    /// Flush the buffered writer to the underlying file. Does not fsync the
    /// file descriptor — that's intentionally left to higher-level
    /// checkpointing so tests stay fast.
    pub fn flush(&mut self) -> Result<()> {
        self.writer.flush().context("flushing session writer")
    }

    /// The session id this writer is bound to.
    #[must_use]
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Number of entries appended since this writer was opened. NOTE: this
    /// is the *append* count, not the total line count of the file (a
    /// reopened file may already contain lines from a prior run).
    #[must_use]
    pub const fn message_count(&self) -> u64 {
        self.message_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::format::EntryKind;
    use serde_json::Value;

    #[test]
    fn create_makes_session_dir_and_file() {
        let tmp = tempfile::tempdir().unwrap();
        let session_dir = tmp.path().join(SESSIONS_DIR);
        let mut w = SessionWriter::create(&session_dir, "sess-1").unwrap();
        w.append(&SessionEntry::now(EntryKind::UserMessage, Value::Null))
            .unwrap();
        w.flush().unwrap();

        let file = session_dir.join("sess-1.jsonl");
        assert!(file.exists(), "expected file at {}", file.display());
        let body = std::fs::read_to_string(&file).unwrap();
        assert_eq!(body.lines().count(), 1);
        assert!(body.contains("\"kind\":\"user_message\""));
    }

    #[test]
    fn append_is_actually_append_only() {
        let tmp = tempfile::tempdir().unwrap();
        let session_dir = tmp.path().join(SESSIONS_DIR);

        let mut w1 = SessionWriter::create(&session_dir, "sess-2").unwrap();
        w1.append(&SessionEntry::now(EntryKind::UserMessage, Value::Null))
            .unwrap();
        w1.flush().unwrap();
        drop(w1);

        // Reopen — existing line must survive.
        let mut w2 = SessionWriter::create(&session_dir, "sess-2").unwrap();
        w2.append(&SessionEntry::now(EntryKind::AssistantMessage, Value::Null))
            .unwrap();
        w2.flush().unwrap();

        let body = std::fs::read_to_string(session_dir.join("sess-2.jsonl")).unwrap();
        assert_eq!(body.lines().count(), 2, "append-only violated: {body}");
        // The reopened writer's count reflects only its own appends.
        assert_eq!(w2.message_count(), 1);
    }

    #[test]
    fn message_count_increments_per_append() {
        let tmp = tempfile::tempdir().unwrap();
        let mut w = SessionWriter::create(&tmp.path().join(SESSIONS_DIR), "s").unwrap();
        assert_eq!(w.message_count(), 0);
        for i in 0..5 {
            w.append(&SessionEntry {
                timestamp: i,
                kind: EntryKind::Heartbeat,
                data: Value::Null,
            })
            .unwrap();
        }
        assert_eq!(w.message_count(), 5);
        assert_eq!(w.session_id(), "s");
    }
}
