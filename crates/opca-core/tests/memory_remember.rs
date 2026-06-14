//! Task 3.3 TDD: `remember` stores with tags, `recall` finds by keyword.
//!
//! Tests the exact scenario from the memory-system spec:
//! remember an item with tags, then recall by a keyword that matches either
//! the tags or the auto-extracted keywords.

use opca_core::memory::{Memory, RecallQuery};

#[test]
fn remember_stores_and_recall_finds_by_keyword() {
    let mem = Memory::<String>::new_in_memory(1000).unwrap();
    mem.remember(&"auth refactor summary".to_string(), &["auth", "security"])
        .unwrap();
    let results = mem.recall(&RecallQuery::Keyword("auth".into())).unwrap();
    assert_eq!(results.len(), 1);
}

#[test]
fn remember_with_tag_makes_item_recallable_by_tag() {
    let mem = Memory::<String>::new_in_memory(1000).unwrap();
    mem.remember(&"session handle cleanup".to_string(), &["auth", "oauth2"])
        .unwrap();

    let by_tag = mem.recall(&RecallQuery::Tag("auth".into())).unwrap();
    assert_eq!(by_tag.len(), 1);

    let by_tag2 = mem.recall(&RecallQuery::Tag("oauth2".into())).unwrap();
    assert_eq!(by_tag2.len(), 1);
}

#[test]
fn remember_auto_extracts_keywords_from_text() {
    let mem = Memory::<String>::new_in_memory(1000).unwrap();
    mem.remember(&"database connection pool exhausted".to_string(), &[])
        .unwrap();

    let hits = mem
        .recall(&RecallQuery::Keyword("database".into()))
        .unwrap();
    assert_eq!(hits.len(), 1);

    let hits2 = mem.recall(&RecallQuery::Keyword("pool".into())).unwrap();
    assert_eq!(hits2.len(), 1);
}

#[test]
fn recall_keyword_or_semantics_no_duplicates() {
    let mem = Memory::<String>::new_in_memory(1000).unwrap();
    mem.remember(&"auth refactor".to_string(), &[]).unwrap();
    let hits = mem
        .recall(&RecallQuery::Keyword("auth refactor".into()))
        .unwrap();
    assert_eq!(hits.len(), 1);
}

#[test]
fn remember_does_not_add_to_active_region() {
    let mem = Memory::<String>::new_in_memory(1000).unwrap();
    mem.remember(&"archived item".to_string(), &["tag"])
        .unwrap();
    assert!(mem.active_slice().is_empty());
    assert_eq!(mem.archive_len().unwrap(), 1);
}

#[test]
fn push_does_not_add_to_archive() {
    let mut mem = Memory::<String>::new_in_memory(1000).unwrap();
    mem.push("active item".to_string());
    assert_eq!(mem.active_slice().len(), 1);
    assert_eq!(mem.archive_len().unwrap(), 0);
}
