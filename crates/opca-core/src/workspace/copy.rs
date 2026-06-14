//! [`CopyWorkspace`] — full directory copy fallback (Tasks 5.2-5.3).
//!
//! On create, the entire project directory is copied to a temporary location
//! and a baseline snapshot of `(relative_path, hash)` is recorded. Diffing
//! walks the workspace and compares against the baseline. Merge applies the
//! `ChangeSet` onto a target directory (`CopyWorkspace` has no native conflict
//! detection, so a non-existent target file is treated as add and an existing
//! divergent file is treated as conflict if `detect_conflicts` is enabled).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::cow::copy_dir_cow;
use super::r#trait::{ChangeSet, MergeResult, Result, Workspace, WorkspaceError};

/// Baseline snapshot entry: file path → content hash.
type Baseline = HashMap<PathBuf, u64>;

/// Full-copy fallback workspace.
#[derive(Debug)]
pub struct CopyWorkspace {
    root: PathBuf,
    source: PathBuf,
    baseline: Baseline,
    frozen: bool,
    detect_conflicts: bool,
}

impl CopyWorkspace {
    /// Create a new `CopyWorkspace` by copying `source` into a fresh directory
    /// inside `parent` (typically a tempdir). The baseline is captured after
    /// the copy.
    pub fn create(source: &Path, parent: &Path, task_id: &str) -> Result<Self> {
        if !source.is_dir() {
            return Err(WorkspaceError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("source is not a directory: {}", source.display()),
            )));
        }
        let root = parent.join(format!("copy-ws-{}", sanitize(task_id)));
        if root.exists() {
            std::fs::remove_dir_all(&root)?;
        }
        copy_dir_cow(source, &root)?;
        let git_dir = root.join(".git");
        if git_dir.exists() {
            std::fs::remove_dir_all(&git_dir)?;
        }
        let agent_dir = root.join(".agent");
        if agent_dir.exists() {
            std::fs::remove_dir_all(&agent_dir)?;
        }
        let baseline = snapshot(&root)?;
        Ok(Self {
            root,
            source: source.to_path_buf(),
            baseline,
            frozen: false,
            detect_conflicts: true,
        })
    }

    /// Test helper: enable/disable target-side conflict detection during merge.
    pub const fn with_conflict_detection(mut self, enabled: bool) -> Self {
        self.detect_conflicts = enabled;
        self
    }

    /// Original source path the workspace was copied from.
    #[must_use]
    pub fn source(&self) -> &Path {
        &self.source
    }

    /// Read the captured baseline (path → hash). Exposed for inspection in
    /// tests and audit.
    #[must_use]
    pub const fn baseline(&self) -> &HashMap<PathBuf, u64> {
        &self.baseline
    }
}

impl Workspace for CopyWorkspace {
    fn path(&self) -> &Path {
        &self.root
    }

    fn freeze(&mut self) -> Result<()> {
        self.frozen = true;
        Ok(())
    }

    fn diff(&self) -> Result<ChangeSet> {
        let current = snapshot(&self.root)?;
        let mut added = Vec::new();
        let mut modified = Vec::new();
        let mut deleted = Vec::new();

        for (rel, hash) in &current {
            match self.baseline.get(rel) {
                None => added.push(rel.clone()),
                Some(prev) if prev != hash => modified.push(rel.clone()),
                _ => {}
            }
        }
        for rel in self.baseline.keys() {
            if !current.contains_key(rel) {
                deleted.push(rel.clone());
            }
        }
        added.sort();
        modified.sort();
        deleted.sort();
        Ok(ChangeSet {
            added,
            modified,
            deleted,
        })
    }

    fn merge_into(&self, target: &Path) -> Result<MergeResult> {
        let changes = self.diff()?;
        let mut conflicts = Vec::new();
        for rel in &changes.added {
            let src = self.root.join(rel);
            let dst = target.join(rel);
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&src, &dst)?;
        }
        for rel in &changes.modified {
            let src = self.root.join(rel);
            let dst = target.join(rel);
            if self.detect_conflicts {
                // Conflict heuristic: target file content differs from the
                // workspace baseline AND from the workspace's current version.
                if let Some(prev_hash) = self.baseline.get(rel) {
                    if let Ok(target_bytes) = std::fs::read(&dst) {
                        let target_hash = hash_bytes(&target_bytes);
                        if target_hash != *prev_hash {
                            // target was independently modified; check whether
                            // it equals the workspace's current content.
                            let src_bytes = std::fs::read(&src)?;
                            if src_bytes != target_bytes {
                                conflicts.push(rel.clone());
                                continue;
                            }
                        }
                    }
                }
            }
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&src, &dst)?;
        }
        for rel in &changes.deleted {
            let dst = target.join(rel);
            if dst.exists() {
                std::fs::remove_file(&dst)?;
            }
        }
        if !conflicts.is_empty() {
            conflicts.sort();
            return Ok(MergeResult::Conflict(conflicts));
        }
        Ok(MergeResult::Clean)
    }

    fn cleanup(&mut self) -> Result<()> {
        if self.root.exists() {
            std::fs::remove_dir_all(&self.root)?;
        }
        Ok(())
    }

    fn is_frozen(&self) -> bool {
        self.frozen
    }
}

impl Drop for CopyWorkspace {
    fn drop(&mut self) {
        // Best-effort cleanup if the consumer forgot to call cleanup().
        if self.root.exists() {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
}

fn sanitize(task_id: &str) -> String {
    task_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// Snapshot every regular file under `root` as a map of relative path → hash.
fn snapshot(root: &Path) -> Result<Baseline> {
    let mut out = HashMap::new();
    walk(root, root, &mut out)?;
    Ok(out)
}

fn walk(root: &Path, current: &Path, out: &mut Baseline) -> Result<()> {
    for entry in std::fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        // Skip well-known VCS / agent directories.
        if name == ".git" || name == ".agent" {
            continue;
        }
        let ft = entry.file_type()?;
        if ft.is_dir() {
            walk(root, &path, out)?;
        } else if ft.is_file() {
            let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
            let bytes = std::fs::read(&path)?;
            out.insert(rel, hash_bytes(&bytes));
        } else if ft.is_symlink() {
            if let Ok(target) = std::fs::read_link(&path) {
                let bytes = target.to_string_lossy().as_bytes().to_vec();
                let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
                out.insert(rel, hash_bytes(&bytes));
            }
        }
    }
    Ok(())
}

/// Fast non-cryptographic hash (FNV-1a 64-bit). Good enough for diff baseline
/// comparisons; collisions are astronomically unlikely for this use case.
fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_project() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        std::fs::write(root.join("README.md"), b"hello world").expect("write");
        std::fs::create_dir_all(root.join("src")).expect("mkdir");
        std::fs::write(root.join("src/main.rs"), b"fn main() {}").expect("write");
        std::fs::write(root.join("src/lib.rs"), b"pub fn lib() {}").expect("write");
        tmp
    }

    #[test]
    fn create_copies_files_and_snapshots_baseline() {
        let project = make_project();
        let parent = tempfile::tempdir().expect("tempdir");
        let ws = CopyWorkspace::create(project.path(), parent.path(), "task-1").expect("create");
        assert!(ws.path().is_dir());
        assert!(ws.path().join("README.md").exists());
        assert!(ws.path().join("src/main.rs").exists());
        assert!(!ws.baseline().is_empty());
        assert!(!ws.is_frozen());
    }

    #[test]
    fn diff_detects_modified_file() {
        let project = make_project();
        let parent = tempfile::tempdir().expect("tempdir");
        let ws = CopyWorkspace::create(project.path(), parent.path(), "task-2").expect("create");
        std::fs::write(
            ws.path().join("src/main.rs"),
            b"fn main() { println!(\"hi\"); }",
        )
        .expect("write");
        let cs = ws.diff().expect("diff");
        assert_eq!(cs.added, Vec::<PathBuf>::new());
        assert_eq!(cs.modified, vec![PathBuf::from("src/main.rs")]);
        assert_eq!(cs.deleted, Vec::<PathBuf>::new());
    }

    #[test]
    fn diff_detects_added_file() {
        let project = make_project();
        let parent = tempfile::tempdir().expect("tempdir");
        let ws = CopyWorkspace::create(project.path(), parent.path(), "task-3").expect("create");
        std::fs::write(ws.path().join("src/oauth.rs"), b"pub fn oauth() {}").expect("write");
        let cs = ws.diff().expect("diff");
        assert_eq!(cs.added, vec![PathBuf::from("src/oauth.rs")]);
        assert!(cs.modified.is_empty());
        assert!(cs.deleted.is_empty());
    }

    #[test]
    fn diff_detects_deleted_file() {
        let project = make_project();
        let parent = tempfile::tempdir().expect("tempdir");
        let ws = CopyWorkspace::create(project.path(), parent.path(), "task-4").expect("create");
        std::fs::remove_file(ws.path().join("src/lib.rs")).expect("remove");
        let cs = ws.diff().expect("diff");
        assert_eq!(cs.deleted, vec![PathBuf::from("src/lib.rs")]);
        assert!(cs.added.is_empty());
        assert!(cs.modified.is_empty());
    }

    #[test]
    fn diff_empty_when_unchanged() {
        let project = make_project();
        let parent = tempfile::tempdir().expect("tempdir");
        let ws = CopyWorkspace::create(project.path(), parent.path(), "task-5").expect("create");
        let cs = ws.diff().expect("diff");
        assert!(cs.is_empty());
    }

    #[test]
    fn freeze_marks_workspace_frozen() {
        let project = make_project();
        let parent = tempfile::tempdir().expect("tempdir");
        let mut ws =
            CopyWorkspace::create(project.path(), parent.path(), "task-6").expect("create");
        assert!(!ws.is_frozen());
        ws.freeze().expect("freeze");
        assert!(ws.is_frozen());
    }

    #[test]
    fn merge_into_clean_target_returns_clean() {
        let project = make_project();
        let parent = tempfile::tempdir().expect("tempdir");
        let ws = CopyWorkspace::create(project.path(), parent.path(), "task-7").expect("create");
        // Make changes in the workspace.
        std::fs::write(ws.path().join("src/main.rs"), b"fn main() { new }").expect("write");
        std::fs::write(ws.path().join("src/new.rs"), b"pub fn new() {}").expect("write");

        // Target is a fresh copy of the original project.
        let target = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(target.path().join("src")).expect("mkdir");
        std::fs::write(target.path().join("README.md"), b"hello world").expect("write");
        std::fs::write(target.path().join("src/main.rs"), b"fn main() {}").expect("write");
        std::fs::write(target.path().join("src/lib.rs"), b"pub fn lib() {}").expect("write");

        let res = ws.merge_into(target.path()).expect("merge");
        assert!(matches!(res, MergeResult::Clean), "got {res:?}");
        assert_eq!(
            std::fs::read_to_string(target.path().join("src/main.rs")).unwrap(),
            "fn main() { new }"
        );
        assert!(target.path().join("src/new.rs").exists());
    }

    #[test]
    fn merge_into_detects_conflict_when_target_diverged() {
        let project = make_project();
        let parent = tempfile::tempdir().expect("tempdir");
        let ws = CopyWorkspace::create(project.path(), parent.path(), "task-8").expect("create");
        std::fs::write(
            ws.path().join("src/main.rs"),
            b"fn main() { workspace_version }",
        )
        .expect("write");

        // Target independently modified the same file.
        let target = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(target.path().join("src")).expect("mkdir");
        std::fs::write(target.path().join("README.md"), b"hello world").expect("write");
        std::fs::write(
            target.path().join("src/main.rs"),
            b"fn main() { target_version }",
        )
        .expect("write");
        std::fs::write(target.path().join("src/lib.rs"), b"pub fn lib() {}").expect("write");

        let res = ws.merge_into(target.path()).expect("merge");
        match res {
            MergeResult::Conflict(files) => {
                assert_eq!(files, vec![PathBuf::from("src/main.rs")]);
            }
            other => panic!("expected conflict, got {other:?}"),
        }
    }

    #[test]
    fn merge_into_no_conflict_when_target_matches_workspace() {
        let project = make_project();
        let parent = tempfile::tempdir().expect("tempdir");
        let ws = CopyWorkspace::create(project.path(), parent.path(), "task-9").expect("create");
        std::fs::write(ws.path().join("src/main.rs"), b"fn main() { same_change }").expect("write");

        let target = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(target.path().join("src")).expect("mkdir");
        std::fs::write(target.path().join("README.md"), b"hello world").expect("write");
        std::fs::write(
            target.path().join("src/main.rs"),
            b"fn main() { same_change }",
        )
        .expect("write");
        std::fs::write(target.path().join("src/lib.rs"), b"pub fn lib() {}").expect("write");

        let res = ws.merge_into(target.path()).expect("merge");
        assert!(matches!(res, MergeResult::Clean), "got {res:?}");
    }

    #[test]
    fn cleanup_removes_workspace_dir() {
        let project = make_project();
        let parent = tempfile::tempdir().expect("tempdir");
        let mut ws =
            CopyWorkspace::create(project.path(), parent.path(), "task-10").expect("create");
        let path = ws.path().to_path_buf();
        ws.cleanup().expect("cleanup");
        assert!(!path.exists());
    }

    #[test]
    fn create_rejects_nonexistent_source() {
        let parent = tempfile::tempdir().expect("tempdir");
        let res =
            CopyWorkspace::create(Path::new("/definitely/not/here"), parent.path(), "task-11");
        assert!(res.is_err());
    }

    #[test]
    fn create_replaces_existing_workspace_dir() {
        let project = make_project();
        let parent = tempfile::tempdir().expect("tempdir");
        {
            let _ws =
                CopyWorkspace::create(project.path(), parent.path(), "task-12").expect("create");
        }
        // Drop removed the dir; create again should still work.
        let ws = CopyWorkspace::create(project.path(), parent.path(), "task-12").expect("create");
        assert!(ws.path().is_dir());
    }
}
