//! `SQLite` session index (Task 15.1 — index layer).
//!
//! While session *content* lives in JSONL (see `writer.rs`), this module
//! holds the *metadata* needed to enumerate, sort, and resume sessions
//! without scanning every JSONL file: id, on-disk path, cwd, start/end
//! timestamps, and a running message count.
//!
//! File location: `<agent_dir>/session-index.sqlite`. Opened via
//! [`SessionIndex::open`] which idempotently creates the schema.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use rusqlite::{Connection, params};

/// The `SQLite` index file name under the agent data dir.
pub const SESSION_INDEX_FILE: &str = "session-index.sqlite";

/// Default search path for the index file under an agent data dir.
#[must_use]
pub fn session_index_path(agent_dir: &Path) -> PathBuf {
    agent_dir.join(SESSION_INDEX_FILE)
}

/// Metadata snapshot for a single session row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionMeta {
    pub session_id: String,
    pub path: String,
    pub cwd: Option<String>,
    pub started_at: u64,
    pub updated_at: u64,
    pub message_count: u64,
}

/// SQLite-backed index of session metadata.
///
/// The connection is held inside a [`std::sync::Mutex`] so the index is
/// `Send + Sync`. Writers (Orchestrator save loop) and readers (resume UI)
/// can share a single handle across threads.
pub struct SessionIndex {
    conn: std::sync::Mutex<Connection>,
}

impl SessionIndex {
    /// Open an in-memory index (tests / throwaway sessions).
    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::init(&conn)?;
        Ok(Self { conn: conn.into() })
    }

    /// Open (or create) the file-backed index at `path`.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating parent dir for {}", path.display()))?;
        }
        let conn = Connection::open(path)
            .with_context(|| format!("opening session index at {}", path.display()))?;
        Self::init(&conn)?;
        Ok(Self { conn: conn.into() })
    }

    fn init(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS sessions (
                id            TEXT    PRIMARY KEY,
                path          TEXT    NOT NULL,
                cwd           TEXT,
                started_at    INTEGER NOT NULL,
                updated_at    INTEGER NOT NULL,
                message_count INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_sessions_updated ON sessions(updated_at DESC);
            ",
        )?;
        Ok(())
    }

    /// Upsert a session row, bumping `updated_at` to "now" and overwriting
    /// `message_count`. Used by the Orchestrator save loop on each flush.
    pub fn upsert(
        &self,
        session_id: &str,
        path: &Path,
        cwd: Option<&str>,
        message_count: u64,
    ) -> Result<()> {
        let now = now_millis();
        let path_str = path.to_string_lossy();
        let conn = self.conn.lock().expect("session index mutex poisoned");
        conn.execute(
            "INSERT INTO sessions (id, path, cwd, started_at, updated_at, message_count)
             VALUES (?1, ?2, ?3, ?4, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET
                 path          = excluded.path,
                 cwd           = excluded.cwd,
                 updated_at    = excluded.updated_at,
                 message_count = excluded.message_count",
            params![session_id, path_str.as_ref(), cwd, now, message_count],
        )?;
        Ok(())
    }

    /// Look up the metadata for a single session id, if present.
    pub fn get(&self, session_id: &str) -> Result<Option<SessionMeta>> {
        let conn = self.conn.lock().expect("session index mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, path, cwd, started_at, updated_at, message_count
             FROM sessions WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![session_id], row_to_meta)?;
        Ok(rows.next().transpose()?)
    }

    /// Return all sessions, newest by `updated_at` first (most recently
    /// active session on top — the natural ordering for a "resume" menu).
    pub fn list_recent(&self) -> Result<Vec<SessionMeta>> {
        let conn = self.conn.lock().expect("session index mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, path, cwd, started_at, updated_at, message_count
             FROM sessions ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map([], row_to_meta)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Total number of indexed sessions.
    pub fn count(&self) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .expect("session index count mutex poisoned");
        let n =
            conn.query_row::<i64, _, _>("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))?;
        Ok(n)
    }
}

fn row_to_meta(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionMeta> {
    Ok(SessionMeta {
        session_id: row.get(0)?,
        path: row.get(1)?,
        cwd: row.get(2)?,
        started_at: row.get(3)?,
        updated_at: row.get(4)?,
        message_count: row.get(5)?,
    })
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(0))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_inserts_then_updates_same_row() {
        let idx = SessionIndex::in_memory().unwrap();
        assert_eq!(idx.count().unwrap(), 0);

        idx.upsert("s1", Path::new("/tmp/s1.jsonl"), Some("/repo"), 3)
            .unwrap();
        let got = idx.get("s1").unwrap().unwrap();
        assert_eq!(got.session_id, "s1");
        assert_eq!(got.cwd.as_deref(), Some("/repo"));
        assert_eq!(got.message_count, 3);

        // Second upsert overwrites message_count + path but keeps id.
        idx.upsert("s1", Path::new("/tmp/s1-v2.jsonl"), None, 10)
            .unwrap();
        let got2 = idx.get("s1").unwrap().unwrap();
        assert_eq!(got2.path, "/tmp/s1-v2.jsonl");
        assert_eq!(got2.cwd, None);
        assert_eq!(got2.message_count, 10);

        assert_eq!(idx.count().unwrap(), 1);
    }

    #[test]
    fn list_recent_orders_by_updated_at_desc() {
        let idx = SessionIndex::in_memory().unwrap();
        idx.upsert("old", Path::new("/o"), None, 1).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        idx.upsert("new", Path::new("/n"), None, 2).unwrap();

        let list = idx.list_recent().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].session_id, "new");
        assert_eq!(list[1].session_id, "old");
    }

    #[test]
    fn open_creates_file_backed_index() {
        let tmp = tempfile::tempdir().unwrap();
        let path = session_index_path(tmp.path());
        {
            let idx = SessionIndex::open(&path).unwrap();
            idx.upsert("x", Path::new("/x"), None, 0).unwrap();
        }
        assert!(path.exists());

        // Reopen — data persists.
        let reopened = SessionIndex::open(&path).unwrap();
        assert_eq!(reopened.count().unwrap(), 1);
        assert!(reopened.get("x").unwrap().is_some());
    }

    #[test]
    fn session_index_path_is_agent_dir_child() {
        let p = session_index_path(Path::new("/repo/.agent"));
        assert_eq!(p.file_name().unwrap(), SESSION_INDEX_FILE);
    }
}
