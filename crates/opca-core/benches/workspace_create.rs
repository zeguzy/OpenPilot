//! Benchmark: Copy-on-Write directory copy vs full recursive copy.
//!
//! Run with: `cargo bench --bench workspace_create` (harness = false).
//!
//! Builds a synthetic project tree of text files, then measures two
//! paths offered by `opca_core::workspace::cow`:
//! - `copy_dir_cow` — attempts `CoW` first (`clonefile` on APFS,
//!   `--reflink=auto` on btrfs/xfs), falls back to a full copy.
//! - `full_copy` — same module's plain recursive copy, used as the
//!   baseline.
//!
//! On `CoW`-capable filesystems the first path should be dramatically
//! faster on large trees because the data blocks are not actually
//! copied. On filesystems without `CoW` support the two paths collapse
//! to the same cost, which is itself a useful regression signal.

use std::hint::black_box;
use std::time::{Duration, Instant};

use opca_core::workspace;

const FILE_COUNT: usize = 2_000;
const FILES_PER_DIR: usize = 50;
const FILE_BYTES: usize = 4_096;
const ITERATIONS: usize = 5;

fn main() {
    let project = tempfile::tempdir().expect("tempdir for source project");
    let src = project.path();
    seed_project(src, FILE_COUNT, FILE_BYTES);

    let cow = workspace::detect_cow(src);
    println!(
        "workspace_create: {FILE_COUNT} files ({FILE_BYTES} bytes each), {ITERATIONS} iterations, CoW detection = {cow:?}\n"
    );

    let cow_ns = time_copy(src, workspace::copy_dir_cow, "copy_dir_cow");
    let full_ns = time_copy(src, full_copy_wrap, "full_copy");

    println!(
        "  copy_dir_cow : {:>8.2} ms / copy",
        cow_ns.as_secs_f64() * 1_000.0
    );
    println!(
        "  full_copy    : {:>8.2} ms / copy",
        full_ns.as_secs_f64() * 1_000.0
    );
    if full_ns > cow_ns {
        println!(
            "  speedup      : {:>8.2}x",
            full_ns.as_secs_f64() / cow_ns.as_secs_f64()
        );
    } else {
        println!("  (CoW unavailable or slower than full copy — both paths identical)");
    }
}

fn time_copy(
    src: &std::path::Path,
    copier: fn(&std::path::Path, &std::path::Path) -> Result<(), workspace::WorkspaceError>,
    label: &str,
) -> Duration {
    let mut samples = Vec::with_capacity(ITERATIONS);
    for _ in 0..ITERATIONS {
        let dst_parent = tempfile::tempdir().expect("tempdir for destination");
        let dst = dst_parent.path().join("out");
        let start = Instant::now();
        copier(src, &dst).unwrap_or_else(|e| panic!("{label} failed: {e:?}"));
        let elapsed = start.elapsed();
        black_box(&elapsed);
        samples.push(elapsed);
        // dst_parent drops here and cleans up both dst and dst_parent.
    }
    // Report median to dampen the OS cache warm-up on the first run.
    samples.sort();
    samples[samples.len() / 2]
}

/// Wrapper so both paths share the same fn signature for `time_copy`.
fn full_copy_wrap(
    src: &std::path::Path,
    dst: &std::path::Path,
) -> Result<(), workspace::WorkspaceError> {
    // The CoW module's `copy_dir_cow` already falls back to full_copy when
    // CoW is unavailable, so the two paths converge naturally. To force the
    // full-copy baseline regardless of CoW support, we shell out to plain
    // `cp -R` here. This keeps the benchmark honest about the cost the CoW
    // path avoids.
    let status = std::process::Command::new("cp")
        .arg("-R")
        .arg(src)
        .arg(dst)
        .status()
        .map_err(|e| workspace::WorkspaceError::Other(format!("cp failed: {e}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(workspace::WorkspaceError::Other(format!(
            "cp exited {status}"
        )))
    }
}

fn seed_project(root: &std::path::Path, total_files: usize, file_bytes: usize) {
    let dirs = total_files.div_ceil(FILES_PER_DIR);
    let payload = "a".repeat(file_bytes);
    for d in 0..dirs {
        let dir = root.join(format!("d{d:04}"));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let files_in_this_dir = FILES_PER_DIR.min(total_files - d * FILES_PER_DIR);
        for f in 0..files_in_this_dir {
            std::fs::write(dir.join(format!("f{f:04}.txt")), &payload).expect("write");
        }
    }
}
