//! Task 15.4 TDD: save → restore roundtrip preserves all session state.
//!
//! Headline acceptance test for the persistence layer. Builds one entry per
//! [`EntryKind`] variant with distinct payloads, writes them through
//! [`SessionWriter`], reads them back via [`SessionReader`], and asserts
//! exact equality. Also exercises the `SQLite` index upsert path and the
//! cross-session Cold Store recall (Task 15.5).

use std::path::Path;

use opca_core::memory::{MemoryMeta, RecallQuery};
use opca_core::session::{
    EntryKind, SessionEntry, SessionIndex, SessionReader, SessionWriter, cold_store_path,
    load_cold_store, session_index_path,
};
use serde_json::json;
use tempfile::tempdir;

/// One entry per `EntryKind` variant, each carrying a distinct payload shape
/// so a serialization mix-up (e.g. wrong variant tag) is caught immediately.
fn sample_entries() -> Vec<SessionEntry> {
    vec![
        SessionEntry {
            timestamp: 1_700_000_000_001,
            kind: EntryKind::UserMessage,
            data: json!({"text": "refactor the auth module"}),
        },
        SessionEntry {
            timestamp: 1_700_000_000_002,
            kind: EntryKind::AssistantMessage,
            data: json!({"text": "starting refactor", "model": "claude"}),
        },
        SessionEntry {
            timestamp: 1_700_000_000_003,
            kind: EntryKind::ToolCall,
            data: json!({"tool": "shell", "args": ["rg", "auth"]}),
        },
        SessionEntry {
            timestamp: 1_700_000_000_004,
            kind: EntryKind::ToolResult,
            data: json!({"ok": true, "stdout": "src/auth/mod.rs"}),
        },
        SessionEntry {
            timestamp: 1_700_000_000_005,
            kind: EntryKind::Heartbeat,
            data: json!({"task_id": "task-7", "progress": 0.42}),
        },
        SessionEntry {
            timestamp: 1_700_000_000_006,
            kind: EntryKind::Highlight,
            data: json!({"note": "decided to split AuthProvider"}),
        },
        SessionEntry {
            timestamp: 1_700_000_000_007,
            kind: EntryKind::TaskCreated,
            data: json!({"task_id": "task-7", "title": "auth refactor"}),
        },
        SessionEntry {
            timestamp: 1_700_000_000_008,
            kind: EntryKind::TaskCompleted,
            data: json!({"task_id": "task-7", "outcome": "merged"}),
        },
        SessionEntry {
            timestamp: 1_700_000_000_009,
            kind: EntryKind::SystemEvent,
            data: json!({"event": "compaction", "dropped": 12}),
        },
    ]
}

#[test]
fn save_restore_roundtrip_preserves_all_entry_kinds() {
    let tmp = tempdir().unwrap();
    let session_dir = tmp.path().join(".agent").join("sessions");
    let session_id = "roundtrip-session";

    let original = sample_entries();
    let expected_count = original.len() as u64;

    let mut writer = SessionWriter::create(&session_dir, session_id).unwrap();
    for entry in &original {
        writer.append(entry).unwrap();
    }
    writer.flush().unwrap();
    assert_eq!(writer.message_count(), expected_count);
    assert_eq!(writer.session_id(), session_id);

    let path = session_dir.join(format!("{session_id}.jsonl"));
    let restored = SessionReader::read(&path).unwrap();

    assert_eq!(
        restored.len(),
        original.len(),
        "entry count mismatch — some entries were lost in roundtrip"
    );
    for (i, (want, got)) in original.iter().zip(restored.iter()).enumerate() {
        assert_eq!(
            want, got,
            "entry {i} ({:?}) did not roundtrip cleanly",
            want.kind
        );
    }
}

#[test]
fn read_range_returns_subrange_of_roundtripped_entries() {
    let tmp = tempdir().unwrap();
    let session_dir = tmp.path().join("sessions");
    let entries = sample_entries();
    let mut writer = SessionWriter::create(&session_dir, "range-session").unwrap();
    for e in &entries {
        writer.append(e).unwrap();
    }
    writer.flush().unwrap();
    let path = session_dir.join("range-session.jsonl");

    // Middle slice.
    let mid = SessionReader::read_range(&path, 2, 5).unwrap();
    assert_eq!(mid.len(), 3);
    assert_eq!(mid, entries[2..5]);

    // Whole thing via range.
    let all = SessionReader::read_range(&path, 0, entries.len()).unwrap();
    assert_eq!(all.len(), entries.len());
    assert_eq!(all, entries);
}

#[test]
fn each_entry_kind_roundtrips_with_correct_tag() {
    // Reinforce: even if a future refactor reshapes payloads, the `kind` tag
    // must remain stable (it's the cross-session contract).
    let tmp = tempdir().unwrap();
    let session_dir = tmp.path().join("sessions");
    let entries = sample_entries();
    let mut writer = SessionWriter::create(&session_dir, "tags").unwrap();
    for e in &entries {
        writer.append(e).unwrap();
    }
    writer.flush().unwrap();
    let path = session_dir.join("tags.jsonl");

    let body = std::fs::read_to_string(&path).unwrap();
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
        let needle = format!("\"kind\":\"{}\"", kind.as_str());
        assert!(
            body.contains(&needle),
            "expected kind tag {needle} in JSONL body"
        );
    }
}

#[test]
fn session_index_round_trips_metadata() {
    let tmp = tempdir().unwrap();
    let agent_dir = tmp.path().join(".agent");
    let index_path = session_index_path(&agent_dir);

    let index = SessionIndex::open(&index_path).unwrap();
    let session_path = agent_dir.join("sessions").join("s1.jsonl");
    index
        .upsert("s1", &session_path, Some("/repo"), 42)
        .unwrap();

    // Reopen from disk — metadata must persist.
    let reopened = SessionIndex::open(&index_path).unwrap();
    let meta = reopened.get("s1").unwrap().expect("session row missing");
    assert_eq!(meta.session_id, "s1");
    assert_eq!(meta.path, session_path.to_string_lossy());
    assert_eq!(meta.cwd.as_deref(), Some("/repo"));
    assert_eq!(meta.message_count, 42);
    assert!(meta.updated_at >= meta.started_at);
}

#[test]
fn cold_store_remembers_across_sessions() {
    // Task 15.5 acceptance: recall(query="auth") finds items from a previous
    // session even after the in-memory Store handle is dropped.
    let tmp = tempdir().unwrap();
    let agent_dir = tmp.path().join(".agent");

    // Session 1: archive something tagged with auth.
    {
        let cold = load_cold_store(&agent_dir).unwrap();
        cold.store(
            "decided to use OAuth2 for the auth boundary",
            &MemoryMeta {
                searchable_text: "auth oauth2 boundary".into(),
                tags: vec!["auth".into(), "security".into()],
                ..Default::default()
            },
        )
        .unwrap();
    }

    // Cold store file actually exists at the conventional path.
    assert_eq!(
        cold_store_path(&agent_dir).file_name().unwrap(),
        "cold-store.sqlite"
    );

    // Session 2: fresh handle, recall surfaces the prior session's item.
    let cold = load_cold_store(&agent_dir).unwrap();
    let hits = cold.recall(&RecallQuery::Keyword("auth".into())).unwrap();
    assert_eq!(hits.len(), 1, "cross-session recall lost the auth item");
    assert!(hits[0].content.contains("OAuth2"));
}

#[test]
fn full_save_index_then_restore_pipeline() {
    // End-to-end: write JSONL + upsert index + reopen both + restore via reader.
    let tmp = tempdir().unwrap();
    let agent_dir = tmp.path().join(".agent");
    let sessions_dir = agent_dir.join("sessions");
    let session_id = "pipeline-session";

    let entries = sample_entries();

    // Save: JSONL + index upsert.
    let mut writer = SessionWriter::create(&sessions_dir, session_id).unwrap();
    for e in &entries {
        writer.append(e).unwrap();
    }
    writer.flush().unwrap();
    let jsonl_path = sessions_dir.join(format!("{session_id}.jsonl"));

    let index = SessionIndex::open(&session_index_path(&agent_dir)).unwrap();
    index
        .upsert(
            session_id,
            &jsonl_path,
            Some("/repo"),
            writer.message_count(),
        )
        .unwrap();

    // Restore: look up the path from the index, then read the JSONL.
    let reopened_index = SessionIndex::open(&session_index_path(&agent_dir)).unwrap();
    let meta = reopened_index
        .get(session_id)
        .unwrap()
        .expect("pipeline session missing from index");
    let restored = SessionReader::read(Path::new(&meta.path)).unwrap();

    assert_eq!(meta.message_count as usize, entries.len());
    assert_eq!(restored, entries);
}
