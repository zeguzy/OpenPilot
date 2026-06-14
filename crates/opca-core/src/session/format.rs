//! Session format types — the JSONL record vocabulary.
//!
//! Each session is an append-only log of [`SessionEntry`] records, one JSON
//! object per line (see `writer.rs` / `reader.rs`). The dual-layer design
//! (JSONL primary + `SQLite` index) follows Open Question #4 in `design.md`:
//! JSONL is human-inspectable and git-friendly; the `SQLite` index powers
//! cross-session lookups.
//!
//! # Wire format
//!
//! ```jsonc
//! {"timestamp":1718000000000,"kind":"user_message","data":{"text":"hi"}}
//! ```
//!
//! `kind` is serialized as `snake_case` so logs stay grep-friendly. The
//! `data` blob is opaque to the persistence layer — callers stash whatever
//! JSON shape they need (a rendered message, a tool-call descriptor, …).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Wall-clock timestamp of the entry, milliseconds since the Unix epoch.
pub type Timestamp = u64;

/// A single append-only record in a session log.
///
/// `Eq` is intentionally *not* derived: [`serde_json::Value`] permits `f64`
/// payloads, which are only `PartialEq` (NaN ≠ NaN).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[allow(clippy::derive_partial_eq_without_eq)]
pub struct SessionEntry {
    /// Milliseconds since the Unix epoch.
    pub timestamp: Timestamp,
    /// Discriminator selecting how `data` should be interpreted.
    pub kind: EntryKind,
    /// Opaque JSON payload. Shape is owned by the producer (Orchestrator,
    /// Task lifecycle, …).
    pub data: Value,
}

/// The kind tag carried by every [`SessionEntry`].
///
/// Variants cover the full lifecycle of a background-agent session:
/// conversation turns, tool traffic, heartbeats, highlights worth surfacing,
/// and the Task lifecycle boundaries. `SystemEvent` is the catch-all for
/// orchestration-level signals that don't fit a more specific variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    /// A user-authored message entering the Orchestrator.
    UserMessage,
    /// An assistant (LLM) message emitted into the Orchestrator conversation.
    AssistantMessage,
    /// A tool invocation requested by the assistant.
    ToolCall,
    /// The result payload of a completed tool invocation.
    ToolResult,
    /// A periodic Task liveness signal (see `lifecycle::heartbeat`).
    Heartbeat,
    /// A noteworthy moment the user may want to jump back to.
    Highlight,
    /// A new Task was created and entered the lifecycle.
    TaskCreated,
    /// A Task reached a terminal outcome.
    TaskCompleted,
    /// Catch-all for Orchestrator / runtime signals (e.g. compaction, swap).
    SystemEvent,
}

impl EntryKind {
    /// Stable string tag used in the JSONL `kind` field. Mirrors the
    /// `#[serde(rename_all = "snake_case")]` encoding so non-serde callers
    /// (e.g. ad-hoc SQL) see the same vocabulary.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UserMessage => "user_message",
            Self::AssistantMessage => "assistant_message",
            Self::ToolCall => "tool_call",
            Self::ToolResult => "tool_result",
            Self::Heartbeat => "heartbeat",
            Self::Highlight => "highlight",
            Self::TaskCreated => "task_created",
            Self::TaskCompleted => "task_completed",
            Self::SystemEvent => "system_event",
        }
    }
}

impl SessionEntry {
    /// Build a new entry tagged with the current wall-clock time.
    ///
    /// Convenience for producers that don't want to thread `SystemTime`
    /// through every call site. Tests that need determinism should set the
    /// `timestamp` field directly after construction.
    #[must_use]
    pub fn now(kind: EntryKind, data: Value) -> Self {
        Self {
            timestamp: now_millis(),
            kind,
            data,
        }
    }
}

/// Current wall-clock time in milliseconds since the Unix epoch.
fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .map(|m| u64::try_from(m).unwrap_or(0))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_kind_serializes_as_snake_case() {
        let entry = SessionEntry {
            timestamp: 1_718_000_000_000,
            kind: EntryKind::UserMessage,
            data: Value::String("hi".into()),
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("\"kind\":\"user_message\""), "got: {json}");

        let back: SessionEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(back, entry);
    }

    #[test]
    fn entry_kind_as_str_matches_serde_tag() {
        for kind in [
            EntryKind::UserMessage,
            EntryKind::AssistantMessage,
            EntryKind::ToolCall,
            EntryKind::ToolResult,
            EntryKind::Heartbeat,
            EntryKind::Highlight,
            EntryKind::TaskCreated,
            EntryKind::TaskCompleted,
            EntryKind::SystemEvent,
        ] {
            let json = serde_json::to_string(&kind).unwrap();
            // serde emits a quoted string for a unit variant.
            let expected = format!("\"{}\"", kind.as_str());
            assert_eq!(json, expected, "mismatch for {kind:?}");
        }
    }

    #[test]
    fn now_stamps_nonzero_timestamp() {
        let entry = SessionEntry::now(EntryKind::SystemEvent, Value::Null);
        assert!(entry.timestamp > 1_700_000_000_000); // post-2023 sanity
    }

    #[test]
    fn data_can_be_arbitrary_json() {
        let payload = serde_json::json!({
            "tool": "shell",
            "args": ["ls", "-la"],
            "ok": true,
        });
        let entry = SessionEntry::now(EntryKind::ToolCall, payload.clone());
        assert_eq!(entry.data, payload);
    }
}
