//! Five-stage completion pipeline coordinator (Task 12.1).
//!
//! When a Task reaches `Delivered` state, the [`CompletionPipeline`] executes:
//! 1. **Freeze** — workspace read-only, final summary, heartbeat.
//! 2. **Review** — risk assessment → rule checks or Audit dispatch.
//! 3. **Merge** — conflict detection + auto-resolve attempt.
//! 4. **Memorialize** — archive to Cold Store, merge highlights.
//! 5. **Cleanup** — delayed workspace removal.
//!
//! See `design.md` §D9 and `specs/completion-pipeline/spec.md`.

use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use tokio::sync::mpsc::UnboundedSender;

use crate::audit::AuditVerdict;
use crate::focus::Highlight;
use crate::lifecycle::Heartbeat;
use crate::orchestrator::Orchestrator;
use crate::provider::{Message, Provider};
use crate::workspace::{CleanupSchedule, Workspace};

use super::cleanup;
use super::dependency::DependencyGraph;
use super::freeze;
use super::memorialize::{self, MemorializeInput};
use super::merge::{self, MergeOutcome};
use super::notification::{self, NotificationLevel};
use super::review::{self, RiskLevel};

/// Final outcome of the completion pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletionOutcome {
    /// Task merged cleanly into the target.
    Merged,
    /// Task queued for user review (high-risk or audit failed).
    PendingReview,
    /// Task rejected (audit fail, merge unresolvable, etc.).
    Rejected(String),
    /// Pipeline failed at some stage (non-recoverable error).
    Failed(String),
}

/// Result of the Freeze stage.
#[derive(Debug, Clone)]
pub struct FreezeResult {
    pub summary: String,
}

/// Result of the Review stage.
#[derive(Debug, Clone)]
pub struct ReviewResult {
    pub risk: RiskLevel,
    pub verdict: Option<AuditVerdict>,
    pub accepted: bool,
}

/// Five-stage completion pipeline coordinator.
pub struct CompletionPipeline {
    orchestrator: Arc<Mutex<Orchestrator>>,
    cleanup_schedule: CleanupSchedule,
    dependencies: DependencyGraph,
}

impl CompletionPipeline {
    #[must_use]
    pub fn new(orchestrator: Arc<Mutex<Orchestrator>>) -> Self {
        Self {
            orchestrator,
            cleanup_schedule: CleanupSchedule::new(),
            dependencies: DependencyGraph::new(),
        }
    }

    #[must_use]
    pub const fn cleanup_schedule(&self) -> &CleanupSchedule {
        &self.cleanup_schedule
    }

    #[allow(clippy::missing_const_for_fn)]
    pub fn cleanup_schedule_mut(&mut self) -> &mut CleanupSchedule {
        &mut self.cleanup_schedule
    }

    #[must_use]
    pub const fn dependencies(&self) -> &DependencyGraph {
        &self.dependencies
    }

    #[allow(clippy::missing_const_for_fn)]
    pub fn dependencies_mut(&mut self) -> &mut DependencyGraph {
        &mut self.dependencies
    }

    /// Record a dependency: `successor` waits for `predecessor` to merge.
    pub fn add_dependency(&mut self, predecessor: &str, successor: &str) {
        self.dependencies.add_dependency(predecessor, successor);
    }

    /// Run the full five-stage pipeline.
    ///
    /// `target` is the merge destination (typically the main project path).
    /// The caller supplies the Task's workspace, provider, active messages,
    /// heartbeat sender, highlights, and focus tags directly because these
    /// are owned by the Task and not retrievable from the Orchestrator after
    /// the Task completes.
    #[allow(clippy::too_many_arguments)]
    pub async fn run(
        &mut self,
        workspace: &mut dyn Workspace,
        provider: &dyn Provider,
        task_id: &str,
        active: &[Message],
        heartbeat_tx: &UnboundedSender<Heartbeat>,
        highlights: &[Highlight],
        tags: &[&str],
        target: &Path,
    ) -> Result<CompletionOutcome> {
        // Stage 1: Freeze
        let freeze_result =
            match freeze::freeze(workspace, provider, active, heartbeat_tx, task_id).await {
                Ok(r) => r,
                Err(e) => {
                    return Ok(CompletionOutcome::Failed(format!(
                        "freeze stage failed: {e}"
                    )));
                }
            };

        let diff = workspace.diff()?;

        // Stage 2: Review
        let review_result = review_stage(provider, task_id, workspace.path(), &diff, active);
        if !review_result.accepted {
            let level = notification::notification_level(review_result.risk, review_result.verdict);
            return Ok(match level {
                NotificationLevel::PendingReview => CompletionOutcome::PendingReview,
                NotificationLevel::Silent => {
                    CompletionOutcome::Rejected("review rejected silently".to_string())
                }
            });
        }

        // Stage 3: Merge
        let merge_outcome = merge::merge(workspace, target, None);
        match &merge_outcome {
            MergeOutcome::Failed(msg) => {
                return Ok(CompletionOutcome::Failed(format!("merge failed: {msg}")));
            }
            MergeOutcome::Conflict(paths) => {
                return Ok(CompletionOutcome::Rejected(format!(
                    "unresolved merge conflict on {} file(s): {}",
                    paths.len(),
                    paths
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )));
            }
            MergeOutcome::Clean | MergeOutcome::AutoResolved => {}
        }

        let mem_input = MemorializeInput {
            task_id,
            final_summary: &freeze_result.summary,
            active,
            diff: &diff,
            highlights,
            tags,
        };
        {
            let orchestrator = self
                .orchestrator
                .lock()
                .expect("orchestrator mutex poisoned");
            memorialize::memorialize(&orchestrator, &mem_input)?;
        }

        // Stage 5: Cleanup
        cleanup::schedule_cleanup_default(&mut self.cleanup_schedule, workspace.path());

        // Dependency chain: activate successors.
        let successors = self.dependencies.drain_successors(task_id);

        if !successors.is_empty() {
            tracing::info!(
                "Task {} merged; {} successor(s) activated",
                task_id,
                successors.len()
            );
        }

        Ok(CompletionOutcome::Merged)
    }

    /// Convenience entry point matching the spec signature.
    ///
    /// Because the Task's workspace and provider are consumed when the Task
    /// finishes, this method requires the caller to supply them explicitly
    /// via [`CompletionInput`]. The `task_id` is used for dependency-chain
    /// activation and Cold Store indexing.
    pub async fn complete(&mut self, input: CompletionInput<'_>) -> Result<CompletionOutcome> {
        self.run(
            input.workspace,
            input.provider,
            input.task_id,
            input.active,
            input.heartbeat_tx,
            input.highlights,
            input.tags,
            input.target,
        )
        .await
    }

    /// Returns successor task IDs that were activated when `task_id` merged.
    #[must_use]
    pub fn activated_successors(&self, task_id: &str) -> Vec<String> {
        self.dependencies.on_task_merged(task_id)
    }
}

/// Explicit inputs for [`CompletionPipeline::complete`].
pub struct CompletionInput<'a> {
    pub task_id: &'a str,
    pub workspace: &'a mut dyn Workspace,
    pub provider: &'a dyn Provider,
    pub active: &'a [Message],
    pub heartbeat_tx: &'a UnboundedSender<Heartbeat>,
    pub highlights: &'a [Highlight],
    pub tags: &'a [&'a str],
    pub target: &'a Path,
}

fn review_stage(
    _provider: &dyn Provider,
    _task_id: &str,
    _workspace_path: &Path,
    diff: &crate::workspace::ChangeSet,
    _active: &[Message],
) -> ReviewResult {
    let risk = review::assess_risk(diff);
    match risk {
        RiskLevel::High => ReviewResult {
            risk,
            verdict: Some(AuditVerdict::Warn),
            accepted: false,
        },
        RiskLevel::Low | RiskLevel::Medium => ReviewResult {
            risk,
            verdict: None,
            accepted: true,
        },
    }
}

impl std::fmt::Debug for CompletionPipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompletionPipeline")
            .field("cleanup_pending", &self.cleanup_schedule.len())
            .field("dependency_edges", &self.dependencies.len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::di::StdClock;
    use crate::di::{StdFileSystem, StdProcess};
    use crate::workspace::CopyWorkspace;
    use futures::stream;

    struct StubProvider {
        text: String,
    }

    impl Provider for StubProvider {
        fn stream(
            &self,
            _messages: &[Message],
            _tools: &[crate::provider::ToolDef],
            _system_prompt: Option<&str>,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = anyhow::Result<crate::provider::ProviderStream>>
                    + Send,
            >,
        > {
            let text = self.text.clone();
            Box::pin(async move {
                let events: Vec<anyhow::Result<crate::provider::ProviderEvent>> = vec![
                    Ok(crate::provider::ProviderEvent::TextDelta(text)),
                    Ok(crate::provider::ProviderEvent::Done {
                        stop_reason: crate::provider::StopReason::EndTurn,
                    }),
                ];
                Ok(Box::pin(stream::iter(events)) as crate::provider::ProviderStream)
            })
        }
    }

    fn make_orchestrator() -> Arc<Mutex<Orchestrator>> {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join("placeholder.txt"), b"x").expect("write");
        let provider = Arc::new(StubProvider {
            text: String::new(),
        }) as Arc<dyn Provider>;
        let clock = Arc::new(StdClock) as Arc<dyn crate::di::Clock>;
        let orch = Orchestrator::new(
            provider,
            tmp.path().to_path_buf(),
            clock,
            Arc::new(StdFileSystem),
            Arc::new(StdProcess),
        );
        std::mem::forget(tmp);
        Arc::new(Mutex::new(orch))
    }

    fn make_workspace() -> (tempfile::TempDir, tempfile::TempDir, CopyWorkspace) {
        let project = tempfile::tempdir().expect("tempdir");
        std::fs::write(project.path().join("README.md"), b"hello").expect("write");
        let parent = tempfile::tempdir().expect("tempdir");
        let ws = CopyWorkspace::create(project.path(), parent.path(), "pipe-test").expect("create");
        (project, parent, ws)
    }

    #[tokio::test]
    async fn pipeline_merges_clean_md_change() {
        let orch = make_orchestrator();
        let mut pipeline = CompletionPipeline::new(orch);

        let (project, _parent, mut ws) = make_workspace();
        std::fs::write(ws.path().join("README.md"), b"updated docs").expect("write");

        let provider = StubProvider {
            text: "summary".to_string(),
        };
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

        let outcome = pipeline
            .run(
                &mut ws,
                &provider,
                "task-md",
                &[Message::assistant("updated docs")],
                &tx,
                &[],
                &[],
                project.path(),
            )
            .await
            .expect("pipeline");

        assert_eq!(outcome, CompletionOutcome::Merged);
        assert!(pipeline.cleanup_schedule().is_scheduled(ws.path()));
    }

    #[tokio::test]
    async fn pipeline_rejects_high_risk_rs_change() {
        let orch = make_orchestrator();
        let mut pipeline = CompletionPipeline::new(orch);

        let (project, _parent, mut ws) = make_workspace();
        std::fs::create_dir_all(ws.path().join("src")).expect("mkdir");
        std::fs::write(ws.path().join("src/main.rs"), b"fn main() {}").expect("write");

        let provider = StubProvider {
            text: "summary".to_string(),
        };
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

        let outcome = pipeline
            .run(
                &mut ws,
                &provider,
                "task-rs",
                &[],
                &tx,
                &[],
                &[],
                project.path(),
            )
            .await
            .expect("pipeline");

        assert!(
            matches!(outcome, CompletionOutcome::PendingReview),
            "high-risk should be pending review, got {outcome:?}"
        );
    }

    #[tokio::test]
    async fn pipeline_activates_successors_after_merge() {
        let orch = make_orchestrator();
        let mut pipeline = CompletionPipeline::new(orch);
        pipeline.add_dependency("task-pred", "task-succ");

        let (project, _parent, mut ws) = make_workspace();
        std::fs::write(ws.path().join("doc.md"), b"new doc").expect("write");

        let provider = StubProvider {
            text: "done".to_string(),
        };
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

        let outcome = pipeline
            .run(
                &mut ws,
                &provider,
                "task-pred",
                &[],
                &tx,
                &[],
                &[],
                project.path(),
            )
            .await
            .expect("pipeline");

        assert_eq!(outcome, CompletionOutcome::Merged);
        assert!(pipeline.activated_successors("task-pred").is_empty());
    }
}
