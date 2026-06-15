//! Continuation chain — tracks an iteration sequence from root to termination.
//!
//! See `design.md` §D9 for the chain registry rationale and
//! [`ContinuationChain`] for the primary type.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::audit::AuditVerdict;

use super::budget::ContinuationBudget;
use super::termination::ChainTerminationReason;

/// Unique identifier for a continuation chain.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChainId(String);

impl ChainId {
    /// Generates a new random chain ID (UUID v4).
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    /// Returns the underlying string representation.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for ChainId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ChainId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Whether a chain is still dispatching new iterations or has stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChainStatus {
    /// The chain is active and may dispatch further iterations.
    Active,
    /// The chain has stopped; no further iterations will be dispatched.
    Terminated(ChainTerminationReason),
}

/// Diagnostic record of a single completed iteration within a chain.
#[derive(Debug, Clone)]
pub struct IterationRecord {
    /// The Task ID that executed this iteration.
    pub task_id: String,
    /// Which iteration number this was (1-based).
    pub iteration: u32,
    /// The audit verdict for this iteration's output, if audited.
    pub verdict: Option<AuditVerdict>,
    /// Cost of this iteration in USD.
    pub cost_usd: f64,
    /// How long this iteration took.
    pub duration: Duration,
    /// Human-readable summary of the diff produced.
    pub diff_summary: String,
}

/// A continuation chain — the full lifecycle of a multi-iteration task.
///
/// Sisyphus metaphor: each iteration is a new boulder push. Only when
/// Audit returns `Confirmed` does the boulder reach the summit and the
/// chain terminates with [`ChainTerminationReason::ConfirmedComplete`].
pub struct ContinuationChain {
    id: ChainId,
    root_task_id: String,
    current_task_id: String,
    budget: ContinuationBudget,
    status: ChainStatus,
    iterations: Vec<IterationRecord>,
}

impl ContinuationChain {
    /// Creates a new active chain rooted at `root_task_id`.
    #[must_use]
    pub fn new(root_task_id: String, budget: ContinuationBudget) -> Self {
        let id = ChainId::new();
        let current_task_id = root_task_id.clone();
        Self {
            id,
            root_task_id,
            current_task_id,
            budget,
            status: ChainStatus::Active,
            iterations: Vec::new(),
        }
    }

    /// Returns the chain's unique ID.
    #[must_use]
    pub const fn id(&self) -> &ChainId {
        &self.id
    }

    /// Returns the root task ID (the first iteration).
    #[must_use]
    pub fn root_task_id(&self) -> &str {
        &self.root_task_id
    }

    /// Returns the currently active task ID.
    #[must_use]
    pub fn current_task_id(&self) -> &str {
        &self.current_task_id
    }

    /// Convenience: delegates to the budget's iteration counter.
    #[must_use]
    pub const fn current_iteration(&self) -> u32 {
        self.budget.current_iteration()
    }

    /// Appends a completed iteration record and updates `current_task_id`.
    pub fn append_iteration(&mut self, record: IterationRecord) {
        self.current_task_id.clone_from(&record.task_id);
        self.iterations.push(record);
    }

    /// Sets the currently active task without appending a record.
    ///
    /// Called by the coordinator when a new continuation Task is dispatched.
    pub fn set_current_task(&mut self, task_id: String) {
        self.current_task_id = task_id;
    }

    /// Returns the chain's budget.
    #[must_use]
    pub const fn budget(&self) -> &ContinuationBudget {
        &self.budget
    }

    /// Returns a mutable reference to the chain's budget.
    pub const fn budget_mut(&mut self) -> &mut ContinuationBudget {
        &mut self.budget
    }

    /// Returns the chain's status.
    #[must_use]
    pub const fn status(&self) -> &ChainStatus {
        &self.status
    }

    /// Returns `true` if the chain is still active.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        matches!(self.status, ChainStatus::Active)
    }

    /// Marks the chain as terminated with the given reason.
    ///
    /// This only updates the status; it does **not** dispatch notifications
    /// or cleanup. The caller is responsible for those side effects.
    pub fn terminate(&mut self, reason: ChainTerminationReason) {
        self.status = ChainStatus::Terminated(reason);
    }

    /// Returns the iteration history.
    #[must_use]
    pub fn iterations(&self) -> &[IterationRecord] {
        &self.iterations
    }
}

impl std::fmt::Debug for ContinuationChain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContinuationChain")
            .field("id", &self.id)
            .field("root_task_id", &self.root_task_id)
            .field("current_task_id", &self.current_task_id)
            .field("status", &self.status)
            .field("iteration_count", &self.iterations.len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_budget() -> ContinuationBudget {
        ContinuationBudget::new(10, 5.0, Duration::from_secs(1800), 2)
    }

    #[test]
    fn new_chain_is_active() {
        let chain = ContinuationChain::new("task-root".to_string(), sample_budget());
        assert!(chain.is_active());
        assert_eq!(chain.status(), &ChainStatus::Active);
        assert_eq!(chain.root_task_id(), "task-root");
        assert_eq!(chain.current_task_id(), "task-root");
        assert!(chain.iterations().is_empty());
    }

    #[test]
    fn chain_id_is_unique() {
        let id1 = ChainId::new();
        let id2 = ChainId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn append_iteration_updates_current_task() {
        let mut chain = ContinuationChain::new("task-a".to_string(), sample_budget());
        chain.append_iteration(IterationRecord {
            task_id: "task-a".to_string(),
            iteration: 1,
            verdict: Some(AuditVerdict::NeedsFix),
            cost_usd: 1.0,
            duration: Duration::from_secs(30),
            diff_summary: "2 files changed".to_string(),
        });
        assert_eq!(chain.iterations().len(), 1);
        assert_eq!(chain.current_task_id(), "task-a");
    }

    #[test]
    fn set_current_task_changes_active_task() {
        let mut chain = ContinuationChain::new("task-a".to_string(), sample_budget());
        chain.set_current_task("task-b".to_string());
        assert_eq!(chain.current_task_id(), "task-b");
    }

    #[test]
    fn terminate_transitions_to_terminated() {
        let mut chain = ContinuationChain::new("task-a".to_string(), sample_budget());
        assert!(chain.is_active());
        chain.terminate(ChainTerminationReason::ConfirmedComplete);
        assert!(!chain.is_active());
        assert_eq!(
            chain.status(),
            &ChainStatus::Terminated(ChainTerminationReason::ConfirmedComplete)
        );
    }

    #[test]
    fn terminated_chain_still_holds_history() {
        let mut chain = ContinuationChain::new("task-a".to_string(), sample_budget());
        chain.append_iteration(IterationRecord {
            task_id: "task-a".to_string(),
            iteration: 1,
            verdict: Some(AuditVerdict::Confirmed),
            cost_usd: 1.0,
            duration: Duration::from_secs(30),
            diff_summary: "done".to_string(),
        });
        chain.terminate(ChainTerminationReason::ConfirmedComplete);
        // History is preserved after termination.
        assert_eq!(chain.iterations().len(), 1);
        assert!(!chain.is_active());
    }

    #[test]
    fn current_iteration_delegates_to_budget() {
        let mut chain = ContinuationChain::new("task-a".to_string(), sample_budget());
        assert_eq!(chain.current_iteration(), 0);
        chain.budget_mut().record_iteration(1.0);
        assert_eq!(chain.current_iteration(), 1);
    }

    #[test]
    fn lifecycle_active_to_terminated() {
        let mut chain = ContinuationChain::new("root".to_string(), sample_budget());
        // Iteration 1
        chain.append_iteration(IterationRecord {
            task_id: "root".to_string(),
            iteration: 1,
            verdict: Some(AuditVerdict::NeedsFix),
            cost_usd: 0.5,
            duration: Duration::from_secs(10),
            diff_summary: "wip".to_string(),
        });
        chain.set_current_task("iter-2".to_string());
        assert!(chain.is_active());

        // Iteration 2 — confirmed
        chain.append_iteration(IterationRecord {
            task_id: "iter-2".to_string(),
            iteration: 2,
            verdict: Some(AuditVerdict::Confirmed),
            cost_usd: 0.3,
            duration: Duration::from_secs(20),
            diff_summary: "done".to_string(),
        });
        chain.terminate(ChainTerminationReason::ConfirmedComplete);
        assert!(!chain.is_active());
        assert_eq!(chain.iterations().len(), 2);
    }

    #[test]
    fn chain_id_display_and_as_str() {
        let id = ChainId::new();
        assert!(!id.as_str().is_empty());
        assert_eq!(id.as_str(), format!("{id}"));
    }
}
