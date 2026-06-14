//! JSONL session reader (Task 15.3).
//!
//! [`SessionReader`] reloads the full conversation + Task state from a JSONL
//! file written by [`crate::session::SessionWriter`]. Malformed lines are
//! reported as errors (not silently skipped) so a truncated final line — the
//! only corruption mode possible under append-only writes — surfaces during
//! restore instead of silently dropping history.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use anyhow::{Context, Result};

use super::format::SessionEntry;

/// Stateless reader for session JSONL files.
///
/// All methods are associated functions: there is no per-reader state to
/// cache. Lines are read in file order, which is the same as append order
/// (see [`crate::session::SessionWriter`]).
pub struct SessionReader;

impl SessionReader {
    /// Read every entry from `path`, in file order.
    ///
    /// Blank lines (e.g. a stray trailing newline) are tolerated and
    /// skipped. Any line that fails to deserialize as a [`SessionEntry`]
    /// aborts the read with a context-rich error.
    pub fn read(path: &Path) -> Result<Vec<SessionEntry>> {
        let file =
            File::open(path).with_context(|| format!("opening session file {}", path.display()))?;
        let reader = BufReader::new(file);
        let mut out = Vec::new();
        for (idx, line) in reader.lines().enumerate() {
            let line =
                line.with_context(|| format!("reading line {} of {}", idx + 1, path.display()))?;
            if line.trim().is_empty() {
                continue;
            }
            let entry: SessionEntry = serde_json::from_str(&line)
                .with_context(|| format!("parsing line {} of {}", idx + 1, path.display()))?;
            out.push(entry);
        }
        Ok(out)
    }

    /// Read entries with index in `[from, to)` (half-open, 0-based, in file
    /// order). `from >= to` yields an empty vec. Out-of-range indices clamp
    /// to the available range rather than erroring — this mirrors how a
    /// "give me the last N turns" caller would expect to page.
    pub fn read_range(path: &Path, from: usize, to: usize) -> Result<Vec<SessionEntry>> {
        if from >= to {
            return Ok(Vec::new());
        }
        let all = Self::read(path)?;
        if from >= all.len() {
            return Ok(Vec::new());
        }
        let end = to.min(all.len());
        Ok(all[from..end].to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::format::EntryKind;
    use crate::session::writer::{SESSIONS_DIR, SessionWriter};
    use serde_json::{Value, json};

    fn write_session(dir: &Path, id: &str, entries: &[SessionEntry]) -> std::path::PathBuf {
        let session_dir = dir.join(SESSIONS_DIR);
        let mut w = SessionWriter::create(&session_dir, id).unwrap();
        for e in entries {
            w.append(e).unwrap();
        }
        w.flush().unwrap();
        session_dir.join(format!("{id}.jsonl"))
    }

    #[test]
    fn read_roundtrips_written_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let entries = vec![
            SessionEntry {
                timestamp: 1,
                kind: EntryKind::UserMessage,
                data: json!({"text": "hello"}),
            },
            SessionEntry {
                timestamp: 2,
                kind: EntryKind::AssistantMessage,
                data: json!({"text": "hi back"}),
            },
        ];
        let path = write_session(tmp.path(), "rt", &entries);
        let read = SessionReader::read(&path).unwrap();
        assert_eq!(read, entries);
    }

    #[test]
    fn read_skips_blank_lines() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write_session(
            tmp.path(),
            "blank",
            &[SessionEntry {
                timestamp: 9,
                kind: EntryKind::SystemEvent,
                data: Value::Null,
            }],
        );
        // Append a couple of stray newlines.
        {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .unwrap();
            writeln!(f).unwrap();
            writeln!(f).unwrap();
        }
        let read = SessionReader::read(&path).unwrap();
        assert_eq!(read.len(), 1, "blank lines should be skipped");
    }

    #[test]
    fn read_range_is_half_open_and_clamps() {
        let tmp = tempfile::tempdir().unwrap();
        let entries: Vec<SessionEntry> = (0..5)
            .map(|i| SessionEntry {
                timestamp: i,
                kind: EntryKind::Heartbeat,
                data: json!({"i": i}),
            })
            .collect();
        let path = write_session(tmp.path(), "range", &entries);

        let mid = SessionReader::read_range(&path, 1, 4).unwrap();
        assert_eq!(mid.len(), 3);
        assert_eq!(
            mid.iter().map(|e| e.timestamp).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );

        // `to` past end clamps.
        let clamped = SessionReader::read_range(&path, 3, 100).unwrap();
        assert_eq!(clamped.len(), 2);

        // `from` past end is empty.
        let past = SessionReader::read_range(&path, 100, 200).unwrap();
        assert!(past.is_empty());

        // empty range is empty.
        let empty = SessionReader::read_range(&path, 2, 2).unwrap();
        assert!(empty.is_empty());
    }

    #[test]
    fn read_reports_malformed_line_with_context() {
        let tmp = tempfile::tempdir().unwrap();
        let session_dir = tmp.path().join(SESSIONS_DIR);
        std::fs::create_dir_all(&session_dir).unwrap();
        let path = session_dir.join("bad.jsonl");
        std::fs::write(&path, b"{\"valid\":false}\n").unwrap();
        let err = SessionReader::read(&path).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("parsing line"), "got: {msg}");
    }
}
