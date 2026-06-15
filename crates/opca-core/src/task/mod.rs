//! Task Agent Loop (Tasks 9.1-9.9).
//!
//! The [`Task`] struct holds everything an agent needs to run: a [`Provider`],
//! a [`Workspace`], a focus contract, a tool registry, lifecycle tracker, and
//! the steering/heartbeat/highlight channels it uses to communicate with the
//! Orchestrator.
//!
//! See `design.md` §D2 (single-process multi-task), §D5 (three-layer context
//! + Focus Contract) and §D12 (steering + follow-up dual queues).

pub mod channels;
pub mod evidence_gate;
pub mod run;
#[allow(clippy::module_inception)]
pub mod task;

pub use channels::{
    ChannelSet, FollowupMessage, FollowupQueue, HeartbeatRx, HeartbeatTx, HighlightRx, HighlightTx,
    OutputRx, OutputTx, SteeringMessage, SteeringRx, SteeringTx, TaskHandle, TaskOutput,
    create_channels,
};
pub use run::{
    Assessment, AssessmentState, IssueSignature, Phase, RunState, TodoItem, normalize_error_msg,
};
pub use task::{Task, TaskOutcome};
