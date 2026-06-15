//! Backward-compatibility re-export.
//!
//! The canonical home for the focus prompt builder is now
//! [`crate::prompt_system::task::focus`]. This file remains so the
//! `pub use prompt::build_focus_prompt` in `focus/mod.rs` keeps
//! resolving for callers that import from `opca_core::focus`.

pub use crate::prompt_system::task::focus::build_focus_prompt;
