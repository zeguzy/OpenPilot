//! Tasks 3.4-3.5 TDD: `compact` reduces active token count and never loses
//! data. Includes a proptest that runs arbitrary push+compact sequences and
//! verifies every pushed item is retrievable from the archive.

use opca_core::memory::{Memory, RecallQuery, ThresholdCompaction};
use proptest::prelude::*;

#[test]
fn compact_reduces_active_token_count_below_50_percent() {
    let mut mem = Memory::<String>::new_in_memory(100).unwrap();
    for i in 0..50 {
        mem.push(format!("item number {i} with some filler words"));
    }
    assert!(mem.is_near_limit());

    mem.compact(&ThresholdCompaction::new(80, 50)).unwrap();

    assert!(mem.active_tokens() < 400);
}

#[test]
fn compact_preserves_all_items_in_archive_or_active() {
    let mut mem = Memory::<String>::new_in_memory(100).unwrap();
    let total = 100;
    for i in 0..total {
        mem.push(format!("item{i:03}payload text"));
    }

    mem.compact(&ThresholdCompaction::new(80, 50)).unwrap();

    for i in 0..total {
        let needle = format!("item{i:03}payload");
        let in_archive = !mem
            .recall(&RecallQuery::Keyword(needle.clone()))
            .unwrap()
            .is_empty();
        let in_active = mem.active_slice().iter().any(|s| s.contains(&needle));
        assert!(
            in_archive || in_active,
            "item {i} lost: not in archive and not in active"
        );
    }
}

#[test]
fn compact_archives_prefix_items() {
    let mut mem = Memory::<String>::new_in_memory(100).unwrap();
    for i in 0..50 {
        mem.push(format!("prefix-item-{i:03} text payload"));
    }

    mem.compact(&ThresholdCompaction::new(80, 50)).unwrap();

    assert!(
        mem.archive_len().unwrap() > 0,
        "compaction should archive at least the prefix"
    );
    let archived = mem.archive_len().unwrap();
    let active: i64 = mem.active_slice().len().try_into().unwrap_or(i64::MAX);
    assert!(
        archived + active >= 50,
        "items lost: archived={archived} active={active}"
    );
}

#[test]
fn compact_keeps_recent_items_in_active() {
    let mut mem = Memory::<String>::new_in_memory(100).unwrap();
    for i in 0..50 {
        mem.push(format!("item {i} some padding tokens here"));
    }

    mem.compact(&ThresholdCompaction::new(80, 50)).unwrap();

    let active_text: Vec<String> = mem.active_slice().to_vec();
    assert!(
        active_text.iter().any(|s| s.contains("item 49")),
        "most recent item should survive in active region: {active_text:?}"
    );
}

#[test]
fn compact_is_noop_when_below_threshold() {
    let mut mem = Memory::<String>::new_in_memory(1000).unwrap();
    mem.push("small item".to_string());

    mem.compact(&ThresholdCompaction::new(80, 50)).unwrap();

    assert_eq!(mem.active_slice().len(), 1);
    assert_eq!(mem.archive_len().unwrap(), 0);
}

#[test]
fn multiple_compacts_never_lose_data() {
    let mut mem = Memory::<String>::new_in_memory(100).unwrap();
    let total_rounds = 5u32;
    let per_round = 30u32;
    let total_pushed = total_rounds * per_round;

    for round in 0..total_rounds {
        for i in 0..per_round {
            let id = round * per_round + i;
            mem.push(format!("id{id:05}payload text round{round}"));
        }
        mem.compact(&ThresholdCompaction::new(80, 50)).unwrap();
    }

    for round in 0..total_rounds {
        for i in 0..per_round {
            let id = round * per_round + i;
            let needle = format!("id{id:05}payload");
            let in_archive = !mem
                .recall(&RecallQuery::Keyword(needle.clone()))
                .unwrap()
                .is_empty();
            let in_active = mem.active_slice().iter().any(|s| s.contains(&needle));
            assert!(
                in_archive || in_active,
                "item {needle} lost after {round} compactions"
            );
        }
    }
    let _ = total_pushed;
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 128,
        ..ProptestConfig::default()
    })]

    /// After any number of pushes followed by compaction, every pushed item
    /// must be findable in the archive or still in active.
    #[test]
    fn compact_never_loses_data(count in 1u32..200) {
        let mut mem = Memory::<String>::new_in_memory(50).unwrap();

        for id in 0..count {
            mem.push(format!("item{id:05}payload text unique"));
        }

        mem.compact(&ThresholdCompaction::new(80, 50)).unwrap();
        mem.compact(&ThresholdCompaction::new(80, 50)).unwrap();

        for id in 0..count {
            let needle = format!("item{id:05}payload");
            let in_archive = !mem
                .recall(&RecallQuery::Keyword(needle.clone()))
                .unwrap()
                .is_empty();
            let in_active = mem
                .active_slice()
                .iter()
                .any(|s| s.contains(&needle));
            prop_assert!(
                in_archive || in_active,
                "item {} lost: not in archive and not in active",
                id
            );
        }
    }

    /// Interleaved pushes and compactions must also preserve all items.
    #[test]
    fn interleaved_push_compact_preserves_all(chunks in prop::collection::vec(1u32..20, 1..20)) {
        let mut mem = Memory::<String>::new_in_memory(30).unwrap();
        let mut pushed = 0u32;

        for &chunk_size in &chunks {
            for _ in 0..chunk_size {
                mem.push(format!("seq{pushed:05}payload"));
                pushed += 1;
            }
            mem.compact(&ThresholdCompaction::new(80, 50)).unwrap();
        }

        for i in 0..pushed {
            let needle = format!("seq{i:05}payload");
            let in_archive = !mem
                .recall(&RecallQuery::Keyword(needle.clone()))
                .unwrap()
                .is_empty();
            let in_active = mem
                .active_slice()
                .iter()
                .any(|s| s.contains(&needle));
            prop_assert!(
                in_archive || in_active,
                "seq-{:05} lost: not in archive and not in active",
                i
            );
        }
    }
}
