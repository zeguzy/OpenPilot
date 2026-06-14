//! Benchmark: Memory recall across the three indexed dimensions.
//!
//! Run with: `cargo bench --bench memory_recall` (harness = false).
//!
//! Loads N items into an in-memory `Memory<String>` archive, each with
//! tags, keywords, and timestamps, then times recall by:
//! - `Keyword` — full-text inverted index lookup.
//! - `Tag` — equality on the tag column.
//! - `TimeRange` — range scan on the timestamp column.
//!
//! The Store uses `SQLite` under the hood, so absolute numbers depend on
//! the bundled `SQLite` build. The benchmark's job is to surface
//! regressions in the query plans (e.g. an accidentally unindexed
//! column), not to hit a specific QPS target.

use std::hint::black_box;
use std::time::{Duration, Instant};

use opca_core::memory::{Memory, MemoryMeta, RecallQuery};

const ITEM_COUNT: i64 = 5_000;
const QUERIES_PER_DIMENSION: usize = 200;

fn main() {
    // Use an on-disk temp file so we exercise the same code path as the
    // Cold Store. in-memory SQLite would not catch file-I/O regressions.
    let tmp = tempfile::tempdir().expect("tempdir");
    let db_file = tmp.path().join("bench.sqlite");
    let mem = Memory::<String>::open(&db_file, 100_000).expect("open memory");

    seed_archive(&mem, ITEM_COUNT);

    println!(
        "memory_recall: {ITEM_COUNT} archived items, {QUERIES_PER_DIMENSION} queries per dimension, file-backed SQLite\n"
    );

    let keyword = bench_keyword(&mem, QUERIES_PER_DIMENSION);
    let tag = bench_tag(&mem, QUERIES_PER_DIMENSION);
    let time_range = bench_time_range(&mem, QUERIES_PER_DIMENSION);

    println!(
        "  recall by Keyword   : {:>8.2} us / query ({} hits total)",
        keyword.elapsed.as_secs_f64() * 1_000_000.0,
        keyword.hits,
    );
    println!(
        "  recall by Tag       : {:>8.2} us / query ({} hits total)",
        tag.elapsed.as_secs_f64() * 1_000_000.0,
        tag.hits,
    );
    println!(
        "  recall by TimeRange : {:>8.2} us / query ({} hits total)",
        time_range.elapsed.as_secs_f64() * 1_000_000.0,
        time_range.hits,
    );
}

struct Sample {
    elapsed: Duration,
    hits: usize,
}

fn bench_keyword(mem: &Memory<String>, n: usize) -> Sample {
    let keywords = ["auth", "session", "config", "workspace", "task"];
    let mut total_hits = 0usize;
    let start = Instant::now();
    for i in 0..n {
        let kw = keywords[i % keywords.len()];
        let hits = mem
            .recall(&RecallQuery::Keyword(kw.to_string()))
            .expect("keyword recall");
        total_hits += hits.len();
        black_box(&hits);
    }
    Sample {
        elapsed: start.elapsed() / n as u32,
        hits: total_hits,
    }
}

fn bench_tag(mem: &Memory<String>, n: usize) -> Sample {
    let tags = ["security", "refactor", "bug", "docs", "test"];
    let mut total_hits = 0usize;
    let start = Instant::now();
    for i in 0..n {
        let tag = tags[i % tags.len()];
        let hits = mem
            .recall(&RecallQuery::Tag(tag.to_string()))
            .expect("tag recall");
        total_hits += hits.len();
        black_box(&hits);
    }
    Sample {
        elapsed: start.elapsed() / n as u32,
        hits: total_hits,
    }
}

fn bench_time_range(mem: &Memory<String>, n: usize) -> Sample {
    // Each query covers a 1-hour window starting (n / 2) hours before "now".
    let now = std::time::SystemTime::now();
    let hour = Duration::from_secs(3600);
    let mut total_hits = 0usize;
    let start = Instant::now();
    for i in 0..n {
        let midpoint = now - hour * ((n / 2) as u32 - i as u32);
        let from = midpoint - hour / 2;
        let to = midpoint + hour / 2;
        let hits = mem
            .recall(&RecallQuery::TimeRange { from, to })
            .expect("time recall");
        total_hits += hits.len();
        black_box(&hits);
    }
    Sample {
        elapsed: start.elapsed() / n as u32,
        hits: total_hits,
    }
}

fn seed_archive(mem: &Memory<String>, n: i64) {
    let bodies = [
        "auth module refactor",
        "session persistence layer",
        "config loader fix",
        "workspace isolation strategy",
        "task lifecycle state machine",
        "audit verdict override",
        "memory compaction strategy",
        "provider streaming event",
        "hook lifecycle interception",
        "plugin packaging format",
    ];
    let tag_sets: &[&[&str]] = &[
        &["security", "auth"],
        &["session", "persistence"],
        &["config", "docs"],
        &["workspace", "refactor"],
        &["task", "lifecycle"],
        &["audit", "security"],
        &["memory", "refactor"],
        &["provider", "task"],
        &["hook", "test"],
        &["plugin", "docs"],
    ];
    let base = std::time::SystemTime::now() - Duration::from_secs(60 * 60 * 24);
    for i in 0..n {
        let bucket = (i as usize) % bodies.len();
        let body = format!("{} #{i}", bodies[bucket]);
        let tags = tag_sets[bucket];
        // Spread timestamps across the last 24h so time-range queries hit.
        let ts = base + Duration::from_secs((i as u64) * 60);
        let meta = MemoryMeta {
            tags: tags.iter().map(|s| (*s).to_string()).collect(),
            searchable_text: body.clone(),
            timestamp: Some(ts),
            task_id: Some(format!("task-{}", i / 7)),
        };
        mem.remember_with(&body, &meta).expect("remember");
    }
}
