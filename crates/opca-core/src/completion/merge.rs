//! Merge stage (Task 12.5).
//!
//! After Review accepts the Task's changes, the Merge stage applies them to
//! the main project. Conflicts are detected and the Orchestrator attempts
//! auto-resolution via `deep_dive` of both contexts; if that fails the
//! conflict is escalated to the user.
//!
//! See `design.md` §D9 (③ Merge) and `specs/completion-pipeline/spec.md`.

use std::path::{Path, PathBuf};

use crate::workspace::{MergeResult, Workspace};

/// Outcome of the Merge stage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeOutcome {
    /// Merge applied cleanly to the target.
    Clean,
    /// A conflict was auto-resolved by the Orchestrator.
    AutoResolved,
    /// A conflict could not be auto-resolved and is escalated to the user.
    Conflict(Vec<PathBuf>),
    /// The merge operation itself failed (I/O error, etc.).
    Failed(String),
}

impl MergeOutcome {
    #[must_use]
    pub const fn is_success(&self) -> bool {
        matches!(self, Self::Clean | Self::AutoResolved)
    }
}

impl std::fmt::Display for MergeOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Clean => write!(f, "clean"),
            Self::AutoResolved => write!(f, "auto-resolved"),
            Self::Conflict(paths) => {
                write!(f, "conflict on {} file(s)", paths.len())
            }
            Self::Failed(msg) => write!(f, "failed: {msg}"),
        }
    }
}

/// Merge `task_workspace`'s changes into `target`.
///
/// Wraps [`Workspace::merge_into`] and maps the workspace-layer
/// [`MergeResult`] to the pipeline-layer [`MergeOutcome`]. The
/// `auto_resolve` callback is invoked for conflicts and may attempt to
/// resolve them (returning `true` if resolution succeeded).
type ConflictResolver<'a> = &'a dyn Fn(&[PathBuf]) -> bool;

#[allow(clippy::type_complexity)]
pub fn merge(
    task_workspace: &dyn Workspace,
    target: &Path,
    auto_resolve: Option<ConflictResolver<'_>>,
) -> MergeOutcome {
    match task_workspace.merge_into(target) {
        Ok(MergeResult::Clean) => MergeOutcome::Clean,
        Ok(MergeResult::Conflict(paths)) => {
            if let Some(resolver) = auto_resolve {
                if resolver(&paths) {
                    return MergeOutcome::AutoResolved;
                }
            }
            MergeOutcome::Conflict(paths)
        }
        Ok(MergeResult::Failed(msg)) => MergeOutcome::Failed(msg),
        Err(e) => MergeOutcome::Failed(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::CopyWorkspace;

    fn make_project() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join("README.md"), b"hello").expect("write");
        tmp
    }

    #[test]
    fn merge_clean_returns_clean() {
        let project = make_project();
        let parent = tempfile::tempdir().expect("tempdir");
        let ws =
            CopyWorkspace::create(project.path(), parent.path(), "merge-clean").expect("create");
        std::fs::write(ws.path().join("README.md"), b"updated").expect("write");

        let target = make_project();

        let outcome = merge(&ws, target.path(), None);
        assert_eq!(outcome, MergeOutcome::Clean);
        assert_eq!(
            std::fs::read_to_string(target.path().join("README.md")).unwrap(),
            "updated"
        );
    }

    #[test]
    fn merge_conflict_without_resolver_returns_conflict() {
        let project = make_project();
        let parent = tempfile::tempdir().expect("tempdir");
        let ws =
            CopyWorkspace::create(project.path(), parent.path(), "merge-conflict").expect("create");
        std::fs::write(ws.path().join("README.md"), b"workspace version of README").expect("write");

        let target = make_project();
        std::fs::write(target.path().join("README.md"), b"target version of README")
            .expect("write");

        let outcome = merge(&ws, target.path(), None);
        match outcome {
            MergeOutcome::Conflict(paths) => {
                assert_eq!(paths, vec![PathBuf::from("README.md")]);
            }
            other => panic!("expected conflict, got {other:?}"),
        }
    }

    #[test]
    fn merge_conflict_auto_resolved() {
        let project = make_project();
        let parent = tempfile::tempdir().expect("tempdir");
        let ws =
            CopyWorkspace::create(project.path(), parent.path(), "merge-auto").expect("create");
        std::fs::write(ws.path().join("README.md"), b"ws").expect("write");

        let target = make_project();
        std::fs::write(target.path().join("README.md"), b"target").expect("write");

        let resolver = |_paths: &[PathBuf]| true;
        let outcome = merge(&ws, target.path(), Some(&resolver));
        assert_eq!(outcome, MergeOutcome::AutoResolved);
    }

    #[test]
    fn merge_conflict_resolver_fails_returns_conflict() {
        let project = make_project();
        let parent = tempfile::tempdir().expect("tempdir");
        let ws = CopyWorkspace::create(project.path(), parent.path(), "merge-resolver-fail")
            .expect("create");
        std::fs::write(ws.path().join("README.md"), b"ws").expect("write");

        let target = make_project();
        std::fs::write(target.path().join("README.md"), b"target").expect("write");

        let resolver = |_paths: &[PathBuf]| false;
        let outcome = merge(&ws, target.path(), Some(&resolver));
        assert!(matches!(outcome, MergeOutcome::Conflict(_)));
    }
}
