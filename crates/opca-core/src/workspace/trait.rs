//! Workspace trait and supporting types (Task 5.1).
//!
//! Abstracts task workspace creation, freezing, diffing, merging and cleanup.
//! Implementations live in [`crate::workspace::copy`], [`crate::workspace::git`]
//! and [`crate::workspace::mirror`].

use std::path::{Path, PathBuf};

use thiserror::Error;

/// Specialised [`Result`] for workspace operations.
pub type Result<T> = std::result::Result<T, WorkspaceError>;

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error("workspace is frozen: {0}")]
    Frozen(PathBuf),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("git error: {0}")]
    Git(String),
    #[error("not a git repository: {0}")]
    NotAGitRepo(PathBuf),
    #[error("merge failed: {0}")]
    MergeFailed(String),
    #[error("merge conflict on: {0}")]
    MergeConflict(String),
    #[error("other: {0}")]
    Other(String),
}

impl From<git2::Error> for WorkspaceError {
    fn from(err: git2::Error) -> Self {
        Self::Git(err.message().to_string())
    }
}

/// Set of file changes relative to a workspace baseline.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChangeSet {
    pub added: Vec<PathBuf>,
    pub modified: Vec<PathBuf>,
    pub deleted: Vec<PathBuf>,
}

impl ChangeSet {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.modified.is_empty() && self.deleted.is_empty()
    }

    #[must_use]
    pub fn total(&self) -> usize {
        self.added.len() + self.modified.len() + self.deleted.len()
    }
}

/// Outcome of merging a workspace's changes into a target.
#[derive(Debug, Clone)]
pub enum MergeResult {
    /// Merge applied cleanly.
    Clean,
    /// Merge produced conflicts in the listed paths (relative to target).
    Conflict(Vec<PathBuf>),
    /// Merge could not be performed.
    Failed(String),
}

/// Abstract workspace isolated per Task.
///
/// Implementations:
/// - [`crate::workspace::copy::CopyWorkspace`]: full directory copy fallback.
/// - [`crate::workspace::git::GitWorkspace`]: native `git worktree`.
/// - [`crate::workspace::mirror::MirrorWorkspace`]: internal git mirror for
///   non-git projects.
pub trait Workspace: Send + Sync {
    /// Filesystem path the agent operates inside.
    fn path(&self) -> &Path;

    /// Freeze the workspace: no further write operations allowed.
    fn freeze(&mut self) -> Result<()>;

    /// Diff working tree against the baseline captured at creation.
    fn diff(&self) -> Result<ChangeSet>;

    /// Merge this workspace's changes into `target`.
    fn merge_into(&self, target: &Path) -> Result<MergeResult>;

    /// Release workspace resources (delayed schedule is implementation
    /// defined; for immediate removal see [`CleanupSchedule::cleanup_now`]).
    fn cleanup(&mut self) -> Result<()>;

    /// Whether [`Workspace::freeze`] has been called.
    fn is_frozen(&self) -> bool;
}
