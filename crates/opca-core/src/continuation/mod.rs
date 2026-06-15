//! Continuation loop — auto-continue Tasks until Audit confirms completion.
//!
//! See `design.md` §D1–§D7 for the full design rationale.
//!
//! # The Sisyphus metaphor
//!
//! Each continuation iteration is a **new boulder push** — a fresh Task with
//! its own workspace, provider, and lifecycle. The chain only terminates when
//! Audit returns [`Confirmed`](crate::audit::AuditVerdict::Confirmed), at
//! which point the boulder reaches the summit. Budget exhaustion, no-progress
//! detection, or user cancellation all stop the chain early.
//!
//! # Module map
//!
//! - [`budget`] — multi-dimensional safety valve (iterations, cost, duration,
//!   no-progress).
//! - [`chain`] — chain identity, status, and iteration history.
//! - [`no_progress`] — doom-loop detection via diff signatures and finding
//!   repetition.
//! - [`policy`] — pluggable decision logic (continue vs. terminate).
//! - [`coordinator`] — orchestrates the loop, produces dispatch requests.
//! - [`termination`] — reason taxonomy for chain termination.

pub mod budget;
pub mod chain;
pub mod coordinator;
pub mod no_progress;
pub mod policy;
pub mod termination;

pub use budget::{BudgetDimension, ContinuationBudget};
pub use chain::{ChainId, ChainStatus, ContinuationChain, IterationRecord};
pub use coordinator::{ContinuationCoordinator, DispatchRequest};
pub use no_progress::NoProgressDetector;
pub use policy::{
    ContinuationPolicy, ContinuationReason, DefaultContinuationPolicy, PolicyDecision,
};
pub use termination::ChainTerminationReason;
