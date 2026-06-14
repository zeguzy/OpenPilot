//! [`WorkspaceManager`] — auto-detect project type and dispatch (Task 5.13).
//!
//! Detection order:
//! 1. If `<project>/.git` exists → [`GitWorkspace`].
//! 2. Otherwise → [`MirrorWorkspace`] (creates `.agent/mirror/` on demand).
//! 3. If mirror creation fails → fall back to [`CopyWorkspace`].

use std::path::{Path, PathBuf};

use super::copy::CopyWorkspace;
use super::git::GitWorkspace;
use super::mirror::MirrorWorkspace;
use super::r#trait::{Result, Workspace, WorkspaceError};

/// Strategy for workspace isolation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IsolationStrategy {
    /// Detect automatically (default).
    #[default]
    Auto,
    /// Use native git worktree (project must be a git repo).
    Git,
    /// Use the internal git mirror.
    Mirror,
    /// Use full directory copy (last resort).
    Copy,
}

/// Auto-detecting workspace factory.
#[derive(Debug, Default)]
pub struct WorkspaceManager {
    strategy: IsolationStrategy,
    /// Parent directory under which workspaces live. If `None`, defaults to
    /// the system's tempdir at create time.
    workspace_parent: Option<PathBuf>,
}

impl WorkspaceManager {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Override the auto-detection strategy.
    #[must_use]
    pub const fn with_strategy(mut self, strategy: IsolationStrategy) -> Self {
        self.strategy = strategy;
        self
    }

    /// Set the parent directory under which workspaces are created.
    #[must_use]
    pub fn with_workspace_parent(mut self, parent: impl Into<PathBuf>) -> Self {
        self.workspace_parent = Some(parent.into());
        self
    }

    #[must_use]
    pub const fn strategy(&self) -> IsolationStrategy {
        self.strategy
    }

    /// Detect which isolation strategy would be used for `project` under the
    /// current configuration. Does **not** create anything on disk.
    #[must_use]
    pub fn detect(&self, project: &Path) -> IsolationStrategy {
        match self.strategy {
            IsolationStrategy::Auto => {
                if is_git_project(project) {
                    IsolationStrategy::Git
                } else {
                    IsolationStrategy::Mirror
                }
            }
            other => other,
        }
    }

    /// Create a workspace for `task_id` from `project`.
    pub fn create(&self, project: &Path, task_id: &str) -> Result<Box<dyn Workspace>> {
        let parent = self
            .workspace_parent
            .clone()
            .unwrap_or_else(std::env::temp_dir);
        let resolved = self.detect(project);
        match resolved {
            IsolationStrategy::Git => {
                if !is_git_project(project) {
                    return Err(WorkspaceError::Other(format!(
                        "GitWorkspace requested but {} is not a git repo",
                        project.display()
                    )));
                }
                let ws = GitWorkspace::create(project, &parent, task_id)?;
                Ok(Box::new(ws))
            }
            IsolationStrategy::Mirror => match MirrorWorkspace::create(project, &parent, task_id) {
                Ok(ws) => Ok(Box::new(ws)),
                Err(e) => {
                    tracing::warn!(
                        "MirrorWorkspace creation failed ({:?}); falling back to CopyWorkspace",
                        e
                    );
                    let ws = CopyWorkspace::create(project, &parent, task_id)?;
                    Ok(Box::new(ws))
                }
            },
            IsolationStrategy::Copy | IsolationStrategy::Auto => {
                let ws = CopyWorkspace::create(project, &parent, task_id)?;
                Ok(Box::new(ws))
            }
        }
    }
}

/// Returns `true` if `path` is inside (or is) a git repository.
pub fn is_git_project(path: &Path) -> bool {
    git2::Repository::discover(path).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_git_project() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        std::fs::write(root.join("a.txt"), b"a").expect("write");
        let _ = std::process::Command::new("git")
            .arg("init")
            .arg("--quiet")
            .current_dir(root)
            .output();
        let _ = std::process::Command::new("git")
            .args(["symbolic-ref", "HEAD", "refs/heads/main"])
            .current_dir(root)
            .output();
        let _ = std::process::Command::new("git")
            .args([
                "-c",
                "user.email=opca@local",
                "-c",
                "user.name=opca",
                "add",
                "-A",
            ])
            .current_dir(root)
            .output();
        let _ = std::process::Command::new("git")
            .args([
                "-c",
                "user.email=opca@local",
                "-c",
                "user.name=opca",
                "commit",
                "-m",
                "init",
                "--quiet",
            ])
            .current_dir(root)
            .output();
        tmp
    }

    #[test]
    fn detect_returns_git_for_git_project() {
        let project = make_git_project();
        let mgr = WorkspaceManager::new();
        assert_eq!(mgr.detect(project.path()), IsolationStrategy::Git);
    }

    #[test]
    fn detect_returns_mirror_for_plain_project() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join("a.txt"), b"a").expect("write");
        let mgr = WorkspaceManager::new();
        assert_eq!(mgr.detect(tmp.path()), IsolationStrategy::Mirror);
    }

    #[test]
    fn explicit_copy_strategy_bypasses_detection() {
        let project = make_git_project();
        let mgr = WorkspaceManager::new().with_strategy(IsolationStrategy::Copy);
        assert_eq!(mgr.detect(project.path()), IsolationStrategy::Copy);
    }

    #[test]
    fn create_for_git_project_yields_git_workspace() {
        let project = make_git_project();
        let parent = tempfile::tempdir().expect("tempdir");
        let mgr = WorkspaceManager::new().with_workspace_parent(parent.path());
        let ws = mgr.create(project.path(), "mgr-1").expect("create");
        assert!(ws.path().exists());
        assert!(ws.path().join("a.txt").exists());
        // Worktree branch should be a git workspace (has .git file, not dir).
        let git_marker = ws.path().join(".git");
        assert!(git_marker.exists());
    }

    #[test]
    fn create_for_plain_project_yields_mirror_workspace() {
        let project = tempfile::tempdir().expect("tempdir");
        std::fs::write(project.path().join("a.txt"), b"a").expect("write");
        let parent = tempfile::tempdir().expect("tempdir");
        let mgr = WorkspaceManager::new().with_workspace_parent(parent.path());
        let ws = mgr.create(project.path(), "mgr-2").expect("create");
        assert!(ws.path().exists());
        assert!(ws.path().join("a.txt").exists());
        assert!(
            project
                .path()
                .join(super::super::mirror::MIRROR_DIR_NAME)
                .exists()
        );
    }

    #[test]
    fn create_with_copy_strategy_yields_copy_workspace() {
        let project = make_git_project();
        let parent = tempfile::tempdir().expect("tempdir");
        let mgr = WorkspaceManager::new()
            .with_strategy(IsolationStrategy::Copy)
            .with_workspace_parent(parent.path());
        let ws = mgr.create(project.path(), "mgr-3").expect("create");
        assert!(ws.path().exists());
        // CopyWorkspace does not have a .git marker.
        assert!(!ws.path().join(".git").exists());
    }

    #[test]
    fn explicit_git_strategy_errors_on_non_git() {
        let project = tempfile::tempdir().expect("tempdir");
        let parent = tempfile::tempdir().expect("tempdir");
        let mgr = WorkspaceManager::new()
            .with_strategy(IsolationStrategy::Git)
            .with_workspace_parent(parent.path());
        let res = mgr.create(project.path(), "mgr-4");
        assert!(res.is_err());
    }
}
