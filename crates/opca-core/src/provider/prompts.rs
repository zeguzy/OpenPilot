//! Backward-compatibility re-exports.
//!
//! The canonical home for these prompts is now
//! [`crate::prompt_system`](crate::prompt_system). These accessors remain
//! so existing call sites (`opca_cli::real.rs`, `task/task.rs`) keep
//! compiling without churn.

pub use crate::prompt_system::orchestrator::orchestrator_prompt;
pub use crate::prompt_system::task::task_prompt;
