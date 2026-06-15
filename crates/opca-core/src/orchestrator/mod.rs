//! Orchestrator — routes user messages, dispatches Tasks, aggregates state.
//!
//! See `design.md` §D2 (single-process multi-task), §D5 (Focus Contract),
//! §D6 (fractal memory).
//!
//! # Quick tour
//!
//! ```
//! use opca_core::orchestrator::{route, RouteDecision};
//!
//! let decision = route("refactor the auth module", "");
//! assert!(matches!(decision, RouteDecision::Background { .. }));
//!
//! let decision = route("what does this function do?", "");
//! assert_eq!(decision, RouteDecision::Foreground);
//! ```

mod conflict;
pub mod dispatch_gate;
#[allow(clippy::module_inception)]
mod orchestrator;
mod registry;
mod routing;

pub use conflict::predict_conflict;
pub use dispatch_gate::{DispatchRejection, can_dispatch};
pub use orchestrator::Orchestrator;
pub use registry::{ContextSnapshot, SubTaskRecord, TaskEntry, TaskRegistry};
pub use routing::{RouteDecision, route};
