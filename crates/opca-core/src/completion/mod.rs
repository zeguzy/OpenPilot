//! Completion Pipeline (Tasks 12.1-12.12).
//!
//! Five-stage completion pipeline executed when a Task reaches `Delivered`:
//! 1. **Freeze** — workspace read-only, final summary, heartbeat.
//! 2. **Review** — risk assessment → rule checks or Audit dispatch.
//! 3. **Merge** — conflict detection + auto-resolve attempt.
//! 4. **Memorialize** — archive to Cold Store, merge highlights.
//! 5. **Cleanup** — delayed workspace removal.
//!
//! See `design.md` §D9 and `specs/completion-pipeline/spec.md`.

pub mod cleanup;
pub mod dependency;
pub mod freeze;
pub mod memorialize;
pub mod merge;
#[allow(clippy::module_inception)]
pub mod notification;
pub mod pipeline;
pub mod review;

pub use cleanup::{schedule_cleanup, schedule_cleanup_default};
pub use dependency::DependencyGraph;
pub use freeze::freeze;
pub use memorialize::{MemorializeInput, archive_summary, memorialize, recall_by_task_id};
pub use merge::{MergeOutcome, merge};
pub use notification::{NotificationLevel, notification_level};
pub use pipeline::{
    CompletionInput, CompletionOutcome, CompletionPipeline, FreezeResult, ReviewResult,
};
pub use review::{RiskLevel, assess_risk};
