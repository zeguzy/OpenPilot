//! Audit Agent — read-only Task specialization for black-box auditing.
//!
//! See `design.md` §D8 for the Audit Agent rationale and
//! `specs/audit-agent/spec.md` for the requirement contracts.
//!
//! The Audit Agent is NOT a full [`crate::task::Task`]. It has a simplified
//! spawn-die lifecycle: spawn → inspect diff → judge → report → die. It holds
//! read-only access to the audited task's workspace path, frozen diff, and
//! task memory (for optional deep dive).

pub mod agent;
pub mod focus;
pub mod report;

pub use agent::{AuditAgent, ModelTier};
pub use focus::{build_audit_focus, is_diff_suspicious};
pub use report::{AuditDecision, AuditManifest, AuditReport, AuditVerdict, Finding};
