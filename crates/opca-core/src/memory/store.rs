//! Persistent archive storage backed by `SQLite`.
//!
//! [`Store`] holds serialized items together with metadata (timestamp,
//! `task_id`, tags, auto-extracted keywords) and exposes multi-dimensional
//! recall via the [`RecallQuery`] vocabulary.
//!
//! ## Schema
//!
//! Three tables form the index:
//! - `memories` — content blob + timestamp + `task_id`
//! - `keywords` — inverted index (`keyword`, `memory_id`)
//! - `tags` — tag index (`tag`, `memory_id`)
//!
//! Each recall dimension maps to a single indexed query, so the "index" is
//! relational rather than in-memory.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use rusqlite::{Connection, params};

use super::index::{RecallQuery, extract_keywords};

/// Metadata captured alongside every archived item.
#[derive(Debug, Clone, Default)]
pub struct MemoryMeta {
    /// Wall-clock time the item was archived. Defaults to "now" at store time
    /// when left as `None`.
    pub timestamp: Option<SystemTime>,
    /// Optional task identifier for the `task_id` index.
    pub task_id: Option<String>,
    /// Focus tags (in addition to auto-extracted keywords).
    pub tags: Vec<String>,
    /// Extra text used only for keyword extraction (e.g. the rendered item
    /// text). The serialized item itself is *not* re-parsed; pass whatever
    /// substrings should be searchable here.
    pub searchable_text: String,
}

impl MemoryMeta {
    /// Empty metadata with the given searchable text. Most callers want this.
    #[must_use]
    pub fn from_text(text: impl Into<String>) -> Self {
        Self {
            searchable_text: text.into(),
            ..Default::default()
        }
    }
}

/// A single archived record returned by recall.
#[derive(Debug, Clone)]
pub struct ArchivedRecord {
    /// Monotonically increasing row id (archive insertion order).
    pub id: i64,
    /// Serialized item payload (JSON or any opaque string).
    pub content: String,
    /// When the item was archived.
    pub timestamp: SystemTime,
    /// Associated task id, if any.
    pub task_id: Option<String>,
    /// Focus tags.
    pub tags: Vec<String>,
}

/// SQLite-backed archive store.
///
/// Use [`Store::in_memory`] for tests and [`Store::open`] for file-backed
/// production storage. The connection is held inside a [`std::sync::Mutex`]
/// so the store is `Send + Sync` and safe to share across threads.
pub struct Store {
    conn: std::sync::Mutex<Connection>,
}

impl Store {
    /// Open a new in-memory database. The database lives as long as the
    /// `Store` (and any last cloned handle) does.
    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::init(&conn)?;
        Ok(Self { conn: conn.into() })
    }

    /// Open (or create) a file-backed database at `path`.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("opening memory store at {}", path.display()))?;
        Self::init(&conn)?;
        Ok(Self { conn: conn.into() })
    }

    fn init(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS memories (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                content     TEXT    NOT NULL,
                timestamp   INTEGER NOT NULL,
                task_id     TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_memories_timestamp ON memories(timestamp);
            CREATE INDEX IF NOT EXISTS idx_memories_task_id   ON memories(task_id);

            CREATE TABLE IF NOT EXISTS keywords (
                keyword     TEXT    NOT NULL,
                memory_id   INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
                PRIMARY KEY (keyword, memory_id)
            );
            CREATE INDEX IF NOT EXISTS idx_keywords_keyword ON keywords(keyword);

            CREATE TABLE IF NOT EXISTS tags (
                tag         TEXT    NOT NULL,
                memory_id   INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
                PRIMARY KEY (tag, memory_id)
            );
            CREATE INDEX IF NOT EXISTS idx_tags_tag ON tags(tag);
            ",
        )?;
        Ok(())
    }

    /// Persist a serialized item with the given metadata, returning its row id.
    pub fn store(&self, content: &str, meta: &MemoryMeta) -> Result<i64> {
        let timestamp = system_millis(meta.timestamp.unwrap_or_else(SystemTime::now));

        let mut keywords = extract_keywords(&meta.searchable_text);
        for tag in &meta.tags {
            keywords.extend(extract_keywords(tag));
        }
        keywords.sort_unstable();
        keywords.dedup();

        let id = {
            let conn = self.conn.lock().expect("store mutex poisoned");
            conn.execute(
                "INSERT INTO memories (content, timestamp, task_id) VALUES (?1, ?2, ?3)",
                params![content, timestamp, meta.task_id],
            )?;
            let id = conn.last_insert_rowid();
            for kw in &keywords {
                conn.execute(
                    "INSERT OR IGNORE INTO keywords (keyword, memory_id) VALUES (?1, ?2)",
                    params![kw, id],
                )?;
            }
            for tag in &meta.tags {
                conn.execute(
                    "INSERT OR IGNORE INTO tags (tag, memory_id) VALUES (?1, ?2)",
                    params![tag, id],
                )?;
            }
            id
        };
        Ok(id)
    }

    /// Return every archived record, in insertion order (ascending id).
    pub fn all(&self) -> Result<Vec<ArchivedRecord>> {
        self.query_records(
            "SELECT id, content, timestamp, task_id FROM memories ORDER BY id ASC",
            [],
        )
    }

    /// Number of archived records.
    pub fn count(&self) -> Result<i64> {
        let n = {
            let conn = self.conn.lock().expect("count mutex poisoned");
            conn.query_row::<i64, _, _>("SELECT COUNT(*) FROM memories", [], |row| row.get(0))?
        };
        Ok(n)
    }

    /// Resolve a [`RecallQuery`] against the appropriate index. Order is
    /// ascending by id (insertion order) for deterministic test output.
    pub fn recall(&self, query: &RecallQuery) -> Result<Vec<ArchivedRecord>> {
        match query {
            RecallQuery::Keyword(s) | RecallQuery::Semantic(s) => self.recall_by_keywords(s),
            RecallQuery::TimeRange { from, to } => self.recall_by_time(*from, *to),
            RecallQuery::TaskId(id) => self.recall_by_task_id(id),
            RecallQuery::Tag(tag) => self.recall_by_tag(tag),
        }
    }

    fn recall_by_keywords(&self, query: &str) -> Result<Vec<ArchivedRecord>> {
        let keywords = extract_keywords(query);
        if keywords.is_empty() {
            return self.all();
        }
        let placeholders = std::iter::repeat_n("?", keywords.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT m.id, m.content, m.timestamp, m.task_id
             FROM memories m
             WHERE EXISTS (
                 SELECT 1 FROM keywords k
                 WHERE k.memory_id = m.id AND k.keyword IN ({placeholders})
             )
             ORDER BY m.id ASC"
        );
        let params: Vec<&dyn rusqlite::ToSql> =
            keywords.iter().map(|k| k as &dyn rusqlite::ToSql).collect();
        self.query_records(&sql, params.as_slice())
    }

    fn recall_by_time(&self, from: SystemTime, to: SystemTime) -> Result<Vec<ArchivedRecord>> {
        let from_ms = system_millis(from);
        let to_ms = system_millis(to);
        self.query_records(
            "SELECT id, content, timestamp, task_id FROM memories
             WHERE timestamp BETWEEN ?1 AND ?2 ORDER BY id ASC",
            params![from_ms, to_ms],
        )
    }

    fn recall_by_task_id(&self, task_id: &str) -> Result<Vec<ArchivedRecord>> {
        self.query_records(
            "SELECT id, content, timestamp, task_id FROM memories
             WHERE task_id = ?1 ORDER BY id ASC",
            params![task_id],
        )
    }

    fn recall_by_tag(&self, tag: &str) -> Result<Vec<ArchivedRecord>> {
        self.query_records(
            "SELECT m.id, m.content, m.timestamp, m.task_id
             FROM memories m JOIN tags t ON t.memory_id = m.id
             WHERE t.tag = ?1 ORDER BY m.id ASC",
            params![tag],
        )
    }

    #[allow(clippy::significant_drop_tightening)]
    fn query_records<P>(&self, sql: &str, params: P) -> Result<Vec<ArchivedRecord>>
    where
        P: rusqlite::Params,
    {
        let conn = self.conn.lock().expect("query mutex poisoned");
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(params, |row| {
            let id: i64 = row.get(0)?;
            let content: String = row.get(1)?;
            let ts_ms: i64 = row.get(2)?;
            let task_id: Option<String> = row.get(3)?;
            Ok(ArchivedRecord {
                id,
                content,
                timestamp: millis_to_system(ts_ms),
                task_id,
                tags: Vec::new(),
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            let mut rec = r?;
            rec.tags = fetch_tags_for(&conn, rec.id)?;
            out.push(rec);
        }
        Ok(out)
    }
}

fn fetch_tags_for(conn: &Connection, id: i64) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT tag FROM tags WHERE memory_id = ?1 ORDER BY tag")?;
    let rows = stmt.query_map(params![id], |row| row.get::<_, String>(0))?;
    let mut tags = Vec::new();
    for t in rows {
        tags.push(t?);
    }
    Ok(tags)
}

/// Convert `SystemTime` to milliseconds since the Unix epoch. Pre-epoch times
/// clamp to 0 (they make no sense for archival).
fn system_millis(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// Inverse of [`system_millis`].
fn millis_to_system(ms: i64) -> SystemTime {
    let u64_ms = u64::try_from(ms).unwrap_or(0);
    UNIX_EPOCH + std::time::Duration::from_millis(u64_ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn in_memory_store_roundtrips_content() {
        let store = Store::in_memory().unwrap();
        let meta = MemoryMeta {
            searchable_text: "hello world".into(),
            ..Default::default()
        };
        let id = store.store("payload-1", &meta).unwrap();
        assert!(id > 0);
        assert_eq!(store.count().unwrap(), 1);

        let all = store.all().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].content, "payload-1");
    }

    #[test]
    fn recall_by_keyword_matches_extracted_tokens() {
        let store = Store::in_memory().unwrap();
        store
            .store(
                "a",
                &MemoryMeta {
                    searchable_text: "auth refactor summary".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        store
            .store(
                "b",
                &MemoryMeta {
                    searchable_text: "unrelated network thing".into(),
                    ..Default::default()
                },
            )
            .unwrap();

        let auth = store.recall(&RecallQuery::Keyword("auth".into())).unwrap();
        assert_eq!(auth.len(), 1);
        assert_eq!(auth[0].content, "a");

        // OR semantics: both keywords present in same record count once.
        let both = store
            .recall(&RecallQuery::Keyword("auth refactor".into()))
            .unwrap();
        assert_eq!(both.len(), 1);
        assert_eq!(both[0].content, "a");
    }

    #[test]
    fn recall_by_task_id_and_tag() {
        let store = Store::in_memory().unwrap();
        let meta = MemoryMeta {
            task_id: Some("task-A".into()),
            tags: vec!["security".into(), "auth".into()],
            searchable_text: "x".into(),
            ..Default::default()
        };
        store.store("p", &meta).unwrap();

        let by_task = store.recall(&RecallQuery::TaskId("task-A".into())).unwrap();
        assert_eq!(by_task.len(), 1);
        assert_eq!(by_task[0].task_id.as_deref(), Some("task-A"));

        let by_tag = store.recall(&RecallQuery::Tag("security".into())).unwrap();
        assert_eq!(by_tag.len(), 1);
        assert_eq!(by_tag[0].content, "p");

        // Tags also surface in keyword index.
        let kw = store.recall(&RecallQuery::Keyword("auth".into())).unwrap();
        assert_eq!(kw.len(), 1);
    }

    #[test]
    fn recall_by_time_range_is_inclusive() {
        let store = Store::in_memory().unwrap();
        let t0 = UNIX_EPOCH + Duration::from_secs(1000);
        let t1 = UNIX_EPOCH + Duration::from_secs(2000);
        store
            .store(
                "old",
                &MemoryMeta {
                    timestamp: Some(t0),
                    searchable_text: "x".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        store
            .store(
                "new",
                &MemoryMeta {
                    timestamp: Some(t1),
                    searchable_text: "x".into(),
                    ..Default::default()
                },
            )
            .unwrap();

        let only_old = store
            .recall(&RecallQuery::TimeRange {
                from: t0,
                to: t0 + Duration::from_secs(1),
            })
            .unwrap();
        assert_eq!(only_old.len(), 1);
        assert_eq!(only_old[0].content, "old");

        let both = store
            .recall(&RecallQuery::TimeRange { from: t0, to: t1 })
            .unwrap();
        assert_eq!(both.len(), 2);
    }
}
