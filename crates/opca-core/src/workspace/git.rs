//! [`GitWorkspace`] — native `git worktree` for git projects (Tasks 5.4-5.7).
//!
//! Strategy: native [`git2`] for repo introspection (HEAD, diff), subprocess
//! `git` CLI for the worktree/merge operations whose libgit2 surface is
//! cumbersome or absent (e.g. `git worktree add`, `git merge`).

use std::path::{Path, PathBuf};
use std::process::Command;

use super::r#trait::{ChangeSet, MergeResult, Result, Workspace, WorkspaceError};

/// Branch prefix used for task worktrees.
pub const BRANCH_PREFIX: &str = "opca-task/";

/// Git-backed workspace using `git worktree`.
#[derive(Debug)]
pub struct GitWorkspace {
    /// The worktree's filesystem root.
    root: PathBuf,
    /// Path to the original repository (the one passed at create time).
    origin_repo: PathBuf,
    /// Branch name created for this worktree.
    branch: String,
    /// Baseline commit OID at worktree creation (stringified for simplicity).
    baseline_oid: String,
    frozen: bool,
}

impl GitWorkspace {
    /// Create a worktree off `origin_repo` (a directory inside a git repo or
    /// the repo's root). The worktree and its branch are created at
    /// `parent/<sanitized task_id>`.
    pub fn create(origin_repo: &Path, parent: &Path, task_id: &str) -> Result<Self> {
        let repo = open_repo(origin_repo)?;
        let head = repo.head()?;
        let target_commit = head
            .peel_to_commit()
            .map_err(|e| WorkspaceError::Git(format!("head peel: {e}")))?;
        let baseline_oid = target_commit.id().to_string();

        let branch = format!("{}{}", BRANCH_PREFIX, sanitize(task_id));
        let root = parent.join(format!("git-ws-{}", sanitize(task_id)));
        if root.exists() {
            std::fs::remove_dir_all(&root)?;
        }

        // Create a branch at HEAD via CLI (libgit2 branch creation behaves
        // differently across versions); then add the worktree.
        run_git(origin_repo, &["branch", &branch, &baseline_oid])?;
        run_git(
            origin_repo,
            &["worktree", "add", &root.to_string_lossy(), &branch],
        )?;

        Ok(Self {
            root,
            origin_repo: discover_root(&repo, origin_repo),
            branch,
            baseline_oid,
            frozen: false,
        })
    }

    /// The branch this workspace lives on.
    #[must_use]
    pub fn branch(&self) -> &str {
        &self.branch
    }

    #[must_use]
    pub fn origin_repo(&self) -> &Path {
        &self.origin_repo
    }

    #[must_use]
    pub fn baseline_oid(&self) -> &str {
        &self.baseline_oid
    }

    fn require_unfrozen(&self) -> Result<()> {
        if self.frozen {
            Err(WorkspaceError::Frozen(self.root.clone()))
        } else {
            Ok(())
        }
    }
}

impl Workspace for GitWorkspace {
    fn path(&self) -> &Path {
        &self.root
    }

    fn freeze(&mut self) -> Result<()> {
        if self.frozen {
            return Ok(());
        }
        let workdir = &self.root;
        let status = run_git(workdir, &["status", "--porcelain"])?;
        if !status.stdout.trim().is_empty() {
            run_git(workdir, &["add", "-A"])?;
            run_git(
                workdir,
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
        let head = run_git(workdir, &["rev-parse", "HEAD"])?;
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
        self.require_unfrozen().ok();
        // First ensure all workspace changes are committed on its branch.
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
                    "opca: auto-commit before merge",
                ],
            )?;
        }
        // Attempt to merge the workspace branch into target repo.
        let merge_out = run_git(
            target,
            &[
                "-c",
                "user.email=opca@local",
                "-c",
                "user.name=opca",
                "merge",
                "--no-commit",
                "--no-ff",
                &self.branch,
            ],
        )?;
        if merge_out.success() {
            // Clean merge: finalise it.
            run_git(
                target,
                &[
                    "-c",
                    "user.email=opca@local",
                    "-c",
                    "user.name=opca",
                    "commit",
                    "--no-edit",
                ],
            )?;
            return Ok(MergeResult::Clean);
        }
        // On failure check whether conflicts are present.
        let conflicts = list_conflicted_files(target)?;
        if conflicts.is_empty() {
            // Some other error.
            return Ok(MergeResult::Failed(format!(
                "git merge failed: {}",
                merge_out.stderr.trim()
            )));
        }
        // Abort the in-progress merge so the target repo isn't left half-merged.
        let _ = run_git(target, &["merge", "--abort"]);
        Ok(MergeResult::Conflict(conflicts))
    }

    fn cleanup(&mut self) -> Result<()> {
        // Remove the worktree.
        if self.root.exists() {
            let _ = run_git(
                &self.origin_repo,
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
        // Remove the branch.
        let _ = run_git(&self.origin_repo, &["branch", "-D", &self.branch]);
        Ok(())
    }

    fn is_frozen(&self) -> bool {
        self.frozen
    }
}

impl Drop for GitWorkspace {
    fn drop(&mut self) {
        if self.root.exists() {
            let _ = run_git(
                &self.origin_repo,
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
        let _ = run_git(&self.origin_repo, &["branch", "-D", &self.branch]);
    }
}

fn open_repo(path: &Path) -> Result<git2::Repository> {
    git2::Repository::discover(path).map_err(WorkspaceError::from)
}

fn discover_root(repo: &git2::Repository, fallback: &Path) -> PathBuf {
    repo.workdir()
        .map_or_else(|| fallback.to_path_buf(), std::path::Path::to_path_buf)
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

fn list_conflicted_files(repo: &Path) -> Result<Vec<PathBuf>> {
    let out = run_git(repo, &["diff", "--name-only", "--diff-filter=U"])?;
    let files = out
        .stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .collect();
    Ok(files)
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

    fn make_git_project() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        std::fs::write(root.join("README.md"), b"hello").expect("write");
        std::fs::create_dir_all(root.join("src")).expect("mkdir");
        std::fs::write(root.join("src/main.rs"), b"fn main() {}").expect("write");
        // init + commit
        run_git(root, &["init"]).expect("init");
        run_git(
            root,
            &[
                "-c",
                "user.email=opca@local",
                "-c",
                "user.name=opca",
                "add",
                "-A",
            ],
        )
        .expect("add");
        run_git(
            root,
            &[
                "-c",
                "user.email=opca@local",
                "-c",
                "user.name=opca",
                "commit",
                "-m",
                "init",
            ],
        )
        .expect("commit");
        // Set default branch name to avoid CI differences.
        let _ = run_git(root, &["symbolic-ref", "HEAD", "refs/heads/main"]);
        tmp
    }

    #[test]
    fn create_worktree_from_git_repo() {
        let project = make_git_project();
        let parent = tempfile::tempdir().expect("tempdir");
        let ws = GitWorkspace::create(project.path(), parent.path(), "g-1").expect("create");
        assert!(ws.path().is_dir());
        assert!(ws.path().join("README.md").exists());
        assert!(ws.path().join(".git").exists()); // worktree has a .git file
        assert!(ws.branch().starts_with(BRANCH_PREFIX));
    }

    #[test]
    fn two_workspaces_are_isolated() {
        let project = make_git_project();
        let parent = tempfile::tempdir().expect("tempdir");
        let ws_a = GitWorkspace::create(project.path(), parent.path(), "iso-a").expect("create");
        let ws_b = GitWorkspace::create(project.path(), parent.path(), "iso-b").expect("create");

        std::fs::write(ws_a.path().join("only_a.txt"), b"a").expect("write");
        std::fs::write(ws_b.path().join("only_b.txt"), b"b").expect("write");

        assert!(ws_a.path().join("only_a.txt").exists());
        assert!(!ws_a.path().join("only_b.txt").exists());
        assert!(ws_b.path().join("only_b.txt").exists());
        assert!(!ws_b.path().join("only_a.txt").exists());
    }

    #[test]
    fn diff_against_baseline_detects_modified_and_added() {
        let project = make_git_project();
        let parent = tempfile::tempdir().expect("tempdir");
        let ws = GitWorkspace::create(project.path(), parent.path(), "diff-1").expect("create");
        std::fs::write(ws.path().join("src/main.rs"), b"fn main() { modified }").expect("write");
        std::fs::write(ws.path().join("src/new.rs"), b"pub fn new() {}").expect("write");
        let cs = ws.diff().expect("diff");
        assert!(cs.modified.contains(&PathBuf::from("src/main.rs")));
        assert!(cs.added.contains(&PathBuf::from("src/new.rs")));
    }

    #[test]
    fn diff_detects_deleted() {
        let project = make_git_project();
        let parent = tempfile::tempdir().expect("tempdir");
        let ws = GitWorkspace::create(project.path(), parent.path(), "diff-2").expect("create");
        std::fs::remove_file(ws.path().join("src/main.rs")).expect("remove");
        let cs = ws.diff().expect("diff");
        assert!(cs.deleted.contains(&PathBuf::from("src/main.rs")));
    }

    #[test]
    fn diff_empty_when_unchanged() {
        let project = make_git_project();
        let parent = tempfile::tempdir().expect("tempdir");
        let ws = GitWorkspace::create(project.path(), parent.path(), "diff-3").expect("create");
        let cs = ws.diff().expect("diff");
        assert!(cs.is_empty(), "expected empty, got {cs:?}");
    }

    #[test]
    fn merge_into_clean_target_returns_clean() {
        let project = make_git_project();
        let parent = tempfile::tempdir().expect("tempdir");
        let ws = GitWorkspace::create(project.path(), parent.path(), "merge-1").expect("create");
        std::fs::write(ws.path().join("src/oauth.rs"), b"pub fn oauth() {}").expect("write");
        std::fs::write(ws.path().join("src/main.rs"), b"fn main() { ws_change }").expect("write");
        // Commit on workspace branch.
        run_git(ws.path(), &["add", "-A"]).expect("add");
        run_git(
            ws.path(),
            &[
                "-c",
                "user.email=opca@local",
                "-c",
                "user.name=opca",
                "commit",
                "-m",
                "ws",
            ],
        )
        .expect("commit");

        let res = ws.merge_into(project.path()).expect("merge");
        assert!(matches!(res, MergeResult::Clean), "got {res:?}");
        assert!(project.path().join("src/oauth.rs").exists());
    }

    #[test]
    fn merge_into_conflict_detected() {
        let project = make_git_project();
        let parent = tempfile::tempdir().expect("tempdir");
        let ws = GitWorkspace::create(project.path(), parent.path(), "merge-2").expect("create");
        std::fs::write(
            ws.path().join("src/main.rs"),
            b"fn main() { workspace_change }\n",
        )
        .expect("write");
        run_git(ws.path(), &["add", "-A"]).expect("add");
        run_git(
            ws.path(),
            &[
                "-c",
                "user.email=opca@local",
                "-c",
                "user.name=opca",
                "commit",
                "-m",
                "ws",
            ],
        )
        .expect("commit");

        // Diverge the target independently on the same lines.
        std::fs::write(
            project.path().join("src/main.rs"),
            b"fn main() { target_change }\n",
        )
        .expect("write");
        run_git(project.path(), &["add", "-A"]).expect("add");
        run_git(
            project.path(),
            &[
                "-c",
                "user.email=opca@local",
                "-c",
                "user.name=opca",
                "commit",
                "-m",
                "target",
            ],
        )
        .expect("commit");

        let res = ws.merge_into(project.path()).expect("merge");
        match res {
            MergeResult::Conflict(files) => {
                assert!(files.contains(&PathBuf::from("src/main.rs")));
            }
            other => panic!("expected conflict, got {other:?}"),
        }
    }

    #[test]
    fn freeze_commits_pending_changes_and_marks_frozen() {
        let project = make_git_project();
        let parent = tempfile::tempdir().expect("tempdir");
        let mut ws =
            GitWorkspace::create(project.path(), parent.path(), "freeze-1").expect("create");
        std::fs::write(ws.path().join("notes.md"), b"frozen state").expect("write");
        assert!(!ws.is_frozen());
        ws.freeze().expect("freeze");
        assert!(ws.is_frozen());
        let cs = ws.diff().expect("diff after freeze");
        assert!(
            cs.is_empty(),
            "diff should be empty after freeze, got {cs:?}"
        );
    }

    #[test]
    fn cleanup_removes_worktree_and_branch() {
        let project = make_git_project();
        let parent = tempfile::tempdir().expect("tempdir");
        let mut ws =
            GitWorkspace::create(project.path(), parent.path(), "cleanup-1").expect("create");
        let path = ws.path().to_path_buf();
        let branch = ws.branch().to_string();
        ws.cleanup().expect("cleanup");
        assert!(!path.exists());
        let branches = run_git(project.path(), &["branch", "--list"]).expect("list");
        assert!(
            !branches.stdout.contains(&branch),
            "branch {} should be gone, got: {}",
            branch,
            branches.stdout
        );
    }
}
