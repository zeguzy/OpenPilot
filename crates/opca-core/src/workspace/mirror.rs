//! [`MirrorWorkspace`] — internal git mirror for non-git projects
//! (Tasks 5.8-5.9).
//!
//! For projects that aren't git repositories, we maintain an internal git
//! repository under `<project>/.agent/mirror/` containing an imported,
//! committed baseline of the project files (respecting `.agentignore`).
//! Each task worktree is then created off that mirror via `git worktree add`.
//! Merge into the original project is performed by extracting the worktree's
//! diff against the baseline and applying the file operations directly to the
//! project directory.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::agentignore::AgentIgnore;
use super::r#trait::{ChangeSet, MergeResult, Result, Workspace, WorkspaceError};

/// Subdirectory of a non-git project that holds the internal mirror repo.
pub const MIRROR_DIR_NAME: &str = ".agent/mirror";

/// Branch prefix for mirror worktrees.
pub const BRANCH_PREFIX: &str = "opca-mirror/";

/// Internal-git mirror workspace for non-git projects.
#[derive(Debug)]
pub struct MirrorWorkspace {
    /// The worktree's filesystem root.
    root: PathBuf,
    /// Original (non-git) project root.
    project: PathBuf,
    /// Internal mirror git repo path (`.agent/mirror/`).
    mirror: PathBuf,
    /// Branch name created for this worktree.
    branch: String,
    /// Baseline commit OID the worktree was created from.
    baseline_oid: String,
    /// `.agentignore` matcher loaded from project (if any).
    ignore: AgentIgnore,
    frozen: bool,
}

impl MirrorWorkspace {
    /// Create a mirror workspace for a non-git project.
    ///
    /// On first invocation this initialises `<project>/.agent/mirror/` as a
    /// fresh git repo and imports the project files (respecting
    /// `.agentignore`). Subsequent invocations reuse the existing mirror but
    /// re-sync new files.
    pub fn create(project: &Path, parent: &Path, task_id: &str) -> Result<Self> {
        if !project.is_dir() {
            return Err(WorkspaceError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("project not a directory: {}", project.display()),
            )));
        }
        let ignore = AgentIgnore::load_from_dir(project)?;
        let mirror = project.join(MIRROR_DIR_NAME);
        ensure_mirror_repo(&mirror, project, &ignore)?;
        let baseline_oid = head_oid(&mirror)?;

        let branch = format!("{}{}", BRANCH_PREFIX, sanitize(task_id));
        let root = parent.join(format!("mirror-ws-{}", sanitize(task_id)));
        if root.exists() {
            std::fs::remove_dir_all(&root)?;
        }
        run_git(&mirror, &["branch", &branch, &baseline_oid])?;
        run_git(
            &mirror,
            &["worktree", "add", &root.to_string_lossy(), &branch],
        )?;

        // Symlink agentignored directories that the runtime may need read
        // access to (node_modules/, target/, dist/...).
        for target in ignore.symlink_targets() {
            let src_dir = project.join(&target);
            if src_dir.is_dir() {
                let link = root.join(&target);
                if !link.exists() {
                    let _ = std::fs::create_dir_all(link.parent().unwrap_or(&root));
                    let _ = std::os::unix::fs::symlink(&src_dir, &link);
                }
            }
        }

        Ok(Self {
            root,
            project: project.to_path_buf(),
            mirror,
            branch,
            baseline_oid,
            ignore,
            frozen: false,
        })
    }

    #[must_use]
    pub fn project(&self) -> &Path {
        &self.project
    }

    #[must_use]
    pub fn mirror(&self) -> &Path {
        &self.mirror
    }

    #[must_use]
    pub fn branch(&self) -> &str {
        &self.branch
    }

    /// Re-sync project files (e.g. when called for a follow-up task after the
    /// project changed). Imports new/modified files and removes deleted ones
    /// inside the mirror, then commits.
    pub fn resync(&self) -> Result<()> {
        import_files(&self.mirror, &self.project, &self.ignore)?;
        run_git(&self.mirror, &["add", "-A"])?;
        let status = run_git(&self.mirror, &["status", "--porcelain"])?;
        if !status.stdout.trim().is_empty() {
            run_git(
                &self.mirror,
                &[
                    "-c",
                    "user.email=opca@local",
                    "-c",
                    "user.name=opca",
                    "commit",
                    "-m",
                    "opca: mirror resync",
                ],
            )?;
        }
        Ok(())
    }
}

impl Workspace for MirrorWorkspace {
    fn path(&self) -> &Path {
        &self.root
    }

    fn freeze(&mut self) -> Result<()> {
        if self.frozen {
            return Ok(());
        }
        let status = run_git(&self.root, &["status", "--porcelain"])?;
        if !status.stdout.trim().is_empty() {
            run_git(&self.root, &["add", "-A"])?;
            run_git(
                &self.root,
                &[
                    "-c",
                    "user.email=opca@local",
                    "-c",
                    "user.name=opca",
                    "commit",
                    "-m",
                    "opca: freeze workspace",
                ],
            )?;
        }
        let head = run_git(&self.root, &["rev-parse", "HEAD"])?;
        self.baseline_oid = head.stdout.trim().to_string();
        self.frozen = true;
        Ok(())
    }

    fn diff(&self) -> Result<ChangeSet> {
        let repo = git2::Repository::open(&self.root)?;
        let baseline = repo
            .revparse_single(&self.baseline_oid)
            .map_err(|e| WorkspaceError::Git(format!("baseline parse: {e}")))?;
        let baseline_tree = baseline
            .peel_to_tree()
            .map_err(|e| WorkspaceError::Git(format!("baseline peel: {e}")))?;

        let mut opts = git2::DiffOptions::new();
        opts.include_untracked(true)
            .recurse_untracked_dirs(true)
            .ignore_submodules(true);
        let diff = repo.diff_tree_to_workdir_with_index(Some(&baseline_tree), Some(&mut opts))?;

        let mut added = Vec::new();
        let mut modified = Vec::new();
        let mut deleted = Vec::new();
        diff.foreach(
            &mut |delta, _progress| {
                match delta.status() {
                    git2::Delta::Added | git2::Delta::Untracked => {
                        if let Some(p) = delta.new_file().path() {
                            added.push(p.to_path_buf());
                        }
                    }
                    git2::Delta::Modified => {
                        if let Some(p) = delta.new_file().path() {
                            modified.push(p.to_path_buf());
                        }
                    }
                    git2::Delta::Deleted => {
                        if let Some(p) = delta.old_file().path() {
                            deleted.push(p.to_path_buf());
                        }
                    }
                    _ => {}
                }
                true
            },
            None,
            None,
            None,
        )?;
        // Filter out paths that match `.agentignore` (e.g. modifications to a
        // symlinked target dir would otherwise show up).
        added.retain(|p| !path_ignored(&self.ignore, p));
        modified.retain(|p| !path_ignored(&self.ignore, p));
        deleted.retain(|p| !path_ignored(&self.ignore, p));
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
            // Conflict heuristic: project file differs from baseline AND from
            // the workspace's version.
            if let Some(baseline_bytes) = read_baseline_file(&self.mirror, &self.baseline_oid, rel)?
            {
                if let Ok(target_bytes) = std::fs::read(&dst) {
                    if target_bytes != baseline_bytes {
                        let src_bytes = std::fs::read(&src)?;
                        if src_bytes != target_bytes {
                            conflicts.push(rel.clone());
                            continue;
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
            let _ = run_git(
                &self.mirror,
                &[
                    "worktree",
                    "remove",
                    "--force",
                    &self.root.to_string_lossy(),
                ],
            );
            if self.root.exists() {
                std::fs::remove_dir_all(&self.root)?;
            }
        }
        let _ = run_git(&self.mirror, &["branch", "-D", &self.branch]);
        Ok(())
    }

    fn is_frozen(&self) -> bool {
        self.frozen
    }
}

impl Drop for MirrorWorkspace {
    fn drop(&mut self) {
        if self.root.exists() {
            let _ = run_git(
                &self.mirror,
                &[
                    "worktree",
                    "remove",
                    "--force",
                    &self.root.to_string_lossy(),
                ],
            );
            if self.root.exists() {
                let _ = std::fs::remove_dir_all(&self.root);
            }
        }
        let _ = run_git(&self.mirror, &["branch", "-D", &self.branch]);
    }
}

fn ensure_mirror_repo(mirror: &Path, project: &Path, ignore: &AgentIgnore) -> Result<()> {
    if !mirror.exists() {
        std::fs::create_dir_all(mirror)?;
        run_git(mirror, &["init", "--quiet"])?;
        run_git(mirror, &["symbolic-ref", "HEAD", "refs/heads/main"])?;
    }
    // (Re)import project files.
    import_files(mirror, project, ignore)?;
    run_git(mirror, &["add", "-A"])?;
    let status = run_git(mirror, &["status", "--porcelain"])?;
    if status.stdout.trim().is_empty() {
        // Nothing new — ensure HEAD exists. If repo is empty, commit a stub.
        if head_oid(mirror).is_err() {
            std::fs::write(mirror.join(".opca-mirror-marker"), b"")?;
            run_git(mirror, &["add", "-A"])?;
            run_git(
                mirror,
                &[
                    "-c",
                    "user.email=opca@local",
                    "-c",
                    "user.name=opca",
                    "commit",
                    "-m",
                    "opca: mirror init (empty)",
                ],
            )?;
        }
        return Ok(());
    }
    run_git(
        mirror,
        &[
            "-c",
            "user.email=opca@local",
            "-c",
            "user.name=opca",
            "commit",
            "-m",
            "opca: mirror sync",
        ],
    )?;
    Ok(())
}

fn import_files(mirror: &Path, project: &Path, ignore: &AgentIgnore) -> Result<()> {
    // Clear existing tracked content (except .git) to allow re-sync.
    if mirror.join(".git").exists() {
        for entry in std::fs::read_dir(mirror)? {
            let entry = entry?;
            let name = entry.file_name();
            if name == ".git" {
                continue;
            }
            let path = entry.path();
            if path.is_dir() {
                std::fs::remove_dir_all(&path)?;
            } else {
                std::fs::remove_file(&path)?;
            }
        }
    }
    copy_filtered(project, project, mirror, ignore)?;
    Ok(())
}

fn copy_filtered(root: &Path, current: &Path, dst_root: &Path, ignore: &AgentIgnore) -> Result<()> {
    for entry in std::fs::read_dir(current)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == ".agent" || name == ".git" {
            continue;
        }
        let path = entry.path();
        let rel = path.strip_prefix(root).unwrap_or(&path);
        let ft = entry.file_type()?;
        if ft.is_dir() {
            if ignore.is_path_ignored(rel, true) {
                continue;
            }
            let dst = dst_root.join(rel);
            std::fs::create_dir_all(&dst)?;
            copy_filtered(root, &path, dst_root, ignore)?;
        } else if ft.is_file() {
            if ignore.is_path_ignored(rel, false) {
                continue;
            }
            let dst = dst_root.join(rel);
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&path, &dst)?;
        }
    }
    Ok(())
}

fn head_oid(repo: &Path) -> Result<String> {
    let out = run_git(repo, &["rev-parse", "HEAD"])?;
    if out.success() {
        Ok(out.stdout.trim().to_string())
    } else {
        Err(WorkspaceError::Git(format!(
            "no HEAD in {}: {}",
            repo.display(),
            out.stderr.trim()
        )))
    }
}

fn read_baseline_file(repo_path: &Path, oid: &str, rel: &Path) -> Result<Option<Vec<u8>>> {
    let repo = git2::Repository::open(repo_path)?;
    let commit = repo
        .revparse_single(oid)
        .map_err(|e| WorkspaceError::Git(format!("revparse {oid}: {e}")))?;
    let tree = commit
        .peel_to_tree()
        .map_err(|e| WorkspaceError::Git(format!("peel: {e}")))?;
    let entry = tree.get_path(rel)?;
    let blob = entry.to_object(&repo)?.peel_to_blob()?;
    Ok(Some(blob.content().to_vec()))
}

fn path_ignored(ignore: &AgentIgnore, p: &Path) -> bool {
    ignore.is_path_ignored(p, false) || {
        let mut prefix = p.to_path_buf();
        while prefix.pop() {
            if ignore.is_path_ignored(&prefix, true) {
                return true;
            }
        }
        false
    }
}

fn run_git(cwd: &Path, args: &[&str]) -> Result<super::super::di::ProcessOutput> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|e| WorkspaceError::Git(format!("spawn git: {e}")))?;
    Ok(super::super::di::ProcessOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        exit_code: output.status.code().unwrap_or(-1),
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_plain_project() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        std::fs::write(root.join("README.md"), b"plain project").expect("write");
        std::fs::create_dir_all(root.join("src")).expect("mkdir");
        std::fs::write(root.join("src/main.rs"), b"fn main() {}").expect("write");
        std::fs::create_dir_all(root.join("node_modules/pkg")).expect("mkdir");
        std::fs::write(
            root.join("node_modules/pkg/index.js"),
            b"module.exports = 1;",
        )
        .expect("write");
        std::fs::write(root.join(".agentignore"), "node_modules/\n").expect("write");
        tmp
    }

    #[test]
    fn create_initialises_mirror_and_worktree() {
        let project = make_plain_project();
        let parent = tempfile::tempdir().expect("tempdir");
        let ws = MirrorWorkspace::create(project.path(), parent.path(), "m-1").expect("create");
        assert!(ws.path().is_dir());
        assert!(ws.path().join("README.md").exists());
        assert!(ws.path().join("src/main.rs").exists());
        assert!(project.path().join(MIRROR_DIR_NAME).join(".git").exists());
        // node_modules is excluded but symlinked back: the link itself exists,
        // is a symlink, and resolves to the original directory.
        let link = ws.path().join("node_modules");
        assert!(link.exists(), "symlink should exist");
        assert!(
            std::fs::symlink_metadata(&link)
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false),
            "node_modules should be a symlink, not a real directory"
        );
        // Reading through the symlink reaches the original file.
        assert_eq!(
            std::fs::read(link.join("pkg/index.js")).unwrap(),
            b"module.exports = 1;"
        );
    }

    #[test]
    fn diff_detects_modified_in_mirror_workspace() {
        let project = make_plain_project();
        let parent = tempfile::tempdir().expect("tempdir");
        let ws = MirrorWorkspace::create(project.path(), parent.path(), "m-2").expect("create");
        std::fs::write(ws.path().join("src/main.rs"), b"fn main() { modified }").expect("write");
        std::fs::write(ws.path().join("src/new.rs"), b"pub fn new() {}").expect("write");
        let cs = ws.diff().expect("diff");
        assert!(cs.modified.contains(&PathBuf::from("src/main.rs")));
        assert!(cs.added.contains(&PathBuf::from("src/new.rs")));
    }

    #[test]
    fn diff_detects_deleted() {
        let project = make_plain_project();
        let parent = tempfile::tempdir().expect("tempdir");
        let ws = MirrorWorkspace::create(project.path(), parent.path(), "m-3").expect("create");
        std::fs::remove_file(ws.path().join("src/main.rs")).expect("remove");
        let cs = ws.diff().expect("diff");
        assert!(cs.deleted.contains(&PathBuf::from("src/main.rs")));
    }

    #[test]
    fn diff_excludes_ignored_paths() {
        let project = make_plain_project();
        let parent = tempfile::tempdir().expect("tempdir");
        let ws = MirrorWorkspace::create(project.path(), parent.path(), "m-4").expect("create");
        // Writing inside the symlinked node_modules dir should NOT show in diff.
        std::fs::write(ws.path().join("node_modules/pkg/new.js"), b"// new").expect("write");
        let cs = ws.diff().expect("diff");
        assert!(cs.added.iter().all(|p| !p.starts_with("node_modules")));
        assert!(cs.modified.iter().all(|p| !p.starts_with("node_modules")));
    }

    #[test]
    fn merge_into_clean_target_applies_changes() {
        let project = make_plain_project();
        let parent = tempfile::tempdir().expect("tempdir");
        let ws = MirrorWorkspace::create(project.path(), parent.path(), "m-5").expect("create");
        std::fs::write(ws.path().join("src/main.rs"), b"fn main() { merged }").expect("write");
        std::fs::write(ws.path().join("src/oauth.rs"), b"pub fn oauth() {}").expect("write");

        let res = ws.merge_into(project.path()).expect("merge");
        assert!(matches!(res, MergeResult::Clean), "got {res:?}");
        assert_eq!(
            std::fs::read_to_string(project.path().join("src/main.rs")).unwrap(),
            "fn main() { merged }"
        );
        assert!(project.path().join("src/oauth.rs").exists());
    }

    #[test]
    fn merge_into_conflict_detected() {
        let project = make_plain_project();
        let parent = tempfile::tempdir().expect("tempdir");
        let ws = MirrorWorkspace::create(project.path(), parent.path(), "m-6").expect("create");
        std::fs::write(ws.path().join("src/main.rs"), b"fn main() { ws_version }").expect("write");

        // Diverge the project independently.
        std::fs::write(
            project.path().join("src/main.rs"),
            b"fn main() { project_version }",
        )
        .expect("write");

        let res = ws.merge_into(project.path()).expect("merge");
        match res {
            MergeResult::Conflict(files) => {
                assert_eq!(files, vec![PathBuf::from("src/main.rs")]);
            }
            other => panic!("expected conflict, got {other:?}"),
        }
    }

    #[test]
    fn freeze_marks_frozen_and_commits_changes() {
        let project = make_plain_project();
        let parent = tempfile::tempdir().expect("tempdir");
        let mut ws = MirrorWorkspace::create(project.path(), parent.path(), "m-7").expect("create");
        std::fs::write(ws.path().join("notes.md"), b"frozen note").expect("write");
        assert!(!ws.is_frozen());
        ws.freeze().expect("freeze");
        assert!(ws.is_frozen());
        let cs = ws.diff().expect("diff");
        assert!(cs.is_empty(), "got non-empty diff {cs:?}");
    }

    #[test]
    fn cleanup_removes_worktree_and_branch() {
        let project = make_plain_project();
        let parent = tempfile::tempdir().expect("tempdir");
        let mut ws = MirrorWorkspace::create(project.path(), parent.path(), "m-8").expect("create");
        let path = ws.path().to_path_buf();
        let branch = ws.branch().to_string();
        ws.cleanup().expect("cleanup");
        assert!(!path.exists());
        let branches =
            run_git(&project.path().join(MIRROR_DIR_NAME), &["branch", "--list"]).expect("list");
        assert!(!branches.stdout.contains(&branch));
    }

    #[test]
    fn resync_picks_up_new_project_files() {
        let project = make_plain_project();
        let parent = tempfile::tempdir().expect("tempdir");
        let ws = MirrorWorkspace::create(project.path(), parent.path(), "m-9").expect("create");
        // Drop the workspace; re-create after adding a new file to the project.
        drop(ws);
        std::fs::write(project.path().join("new_file.txt"), b"new").expect("write");
        let ws2 = MirrorWorkspace::create(project.path(), parent.path(), "m-9b").expect("recreate");
        assert!(ws2.path().join("new_file.txt").exists());
    }
}
