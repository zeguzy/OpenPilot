//! Workspace isolation (Tasks 5.1-5.13).
//!
//! Provides the [`Workspace`] trait and three implementations:
//! - [`copy::CopyWorkspace`] — full directory copy fallback.
//! - [`git::GitWorkspace`] — native `git worktree` for git projects.
//! - [`mirror::MirrorWorkspace`] — internal git mirror for non-git projects.
//!
//! Use [`WorkspaceManager`] to auto-detect the appropriate implementation for
//! a given project.

pub mod agentignore;
pub mod cleanup;
pub mod copy;
pub mod cow;
pub mod git;
pub mod manager;
pub mod mirror;
pub mod r#trait;

pub use agentignore::AgentIgnore;
pub use cleanup::{CleanupSchedule, DEFAULT_CLEANUP_DELAY};
pub use copy::CopyWorkspace;
pub use cow::{CowSupport, copy_dir_cow, detect_cow};
pub use git::GitWorkspace;
pub use manager::{IsolationStrategy, WorkspaceManager, is_git_project};
pub use mirror::{MIRROR_DIR_NAME, MirrorWorkspace};
pub use r#trait::{ChangeSet, MergeResult, Result, Workspace, WorkspaceError};
