//! Sub-Agent system — enables a Task to dispatch child Tasks for
//! parallelizable sub-problems or independent units of work.
//!
//! This module is gated behind the `sub-agents` Cargo feature. When
//! disabled, none of these types exist and the `dispatch_subtask` tool
//! is not registered.
//!
//! # Architecture
//!
//! ```text
//! Parent Task
//!   ├─ calls dispatch_subtask tool
//!   ├─ sends SubTaskRequest to Orchestrator
//!   ├─ enters Waiting state
//!   │      ↓
//!   │  Orchestrator dispatches child Task (parent_task_id set)
//!   │      ↓
//!   │  Child Task runs (abbreviated lifecycle: Phase 2 start)
//!   │      ↓
//!   │  Child completes → SubTaskResult injected into parent
//!   │      ↓
//!   ├─ receives result, transitions to OnIt
//!   └─ continues execution
//! ```
//!
//! See `design.md` §D7 for workspace inheritance rationale and
//! `specs/sub-agent-system/spec.md` for the full requirement contracts.

pub mod aggregation;
pub mod dispatch;
pub mod lifecycle;

pub use aggregation::{aggregate_subtask_heartbeats, escalate_summary, should_escalate_highlight};
pub use dispatch::{
    DispatchLimits, DispatchSubtaskTool, DispatchSubtaskToolDef, SubTaskRequest, SubTaskResult,
    SubTaskScope, SubTaskTicket,
};
pub use lifecycle::{
    SubTaskConfig, initial_phase_for_depth, is_within_depth_limit, is_within_parallel_limit,
    should_skip_phase_zero_one,
};

#[cfg(all(test, feature = "sub-agents"))]
mod integration_tests;
