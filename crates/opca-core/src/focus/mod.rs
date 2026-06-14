//! Focus Contract system — constrains what a Task reports to the Orchestrator.
//!
//! See `design.md` §D5 for the three-layer context + Focus Contract rationale
//! and `specs/task-context-layering/spec.md` for the requirement contracts.
//!
//! # Quick tour
//!
//! ```
//! use opca_core::focus::{FocusContract, FocusUpdate};
//!
//! let mut contract = FocusContract::empty();
//! contract.add("security risks").unwrap();
//! contract.add("breaking changes").unwrap();
//!
//! // Dynamic update via steering
//! let update = FocusUpdate::new()
//!     .with_remove(vec!["breaking changes".to_string()])
//!     .with_add(vec!["performance".to_string()]);
//! update.apply(&mut contract).unwrap();
//! assert!(!contract.contains("breaking changes"));
//! assert!(contract.contains("performance"));
//! ```

mod contract;
mod highlight;
mod prompt;
mod steering;

pub use contract::{FocusContract, FocusError};
pub use highlight::{Highlight, ReportHighlightTool, Severity};
pub use prompt::build_focus_prompt;
pub use steering::FocusUpdate;
