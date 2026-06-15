//! Termination reason taxonomy for continuation chains.
//!
//! See `design.md` §D7. Different termination causes drive different user
//! notifications and post-termination cleanup, so classifying the reason
//! is a first-class concern rather than a simple "success/failure" flag.

use super::budget::BudgetDimension;

/// Why a continuation chain stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChainTerminationReason {
    /// Audit returned `Confirmed` — the chain succeeded.
    ConfirmedComplete,
    /// A budget dimension was exhausted.
    BudgetExhausted(BudgetDimension),
    /// Consecutive iterations produced no meaningful progress.
    NoProgress,
    /// User cancelled via `/stop-continuation`.
    UserCancelled,
    /// Audit could not decide and escalated to a human.
    NeedsHumanReview,
    /// The Task hit an unrecoverable error.
    TaskError(String),
}

impl ChainTerminationReason {
    /// Produces a human-readable notification for the given termination.
    ///
    /// `iterations` and `cost` provide runtime context so the message can
    /// report how far the chain got before stopping.
    #[must_use]
    pub fn notification_message(&self, iterations: u32, cost: f64) -> String {
        match self {
            Self::ConfirmedComplete => {
                format!("Continuation chain completed: {iterations} iterations, ${cost:.2}")
            }
            Self::BudgetExhausted(dim) => {
                let dim_name = match dim {
                    BudgetDimension::Iterations => "iteration limit",
                    BudgetDimension::Cost => "cost limit",
                    BudgetDimension::Duration => "duration limit",
                    BudgetDimension::NoProgress => "no-progress limit",
                };
                format!(
                    "Continuation chain stopped — {dim_name} reached after \
                     {iterations} iterations, ${cost:.2}"
                )
            }
            Self::NoProgress => {
                format!(
                    "Continuation chain stopped — no meaningful progress detected after \
                     {iterations} iterations, ${cost:.2}"
                )
            }
            Self::UserCancelled => {
                format!(
                    "Continuation chain cancelled by user after \
                     {iterations} iterations, ${cost:.2}"
                )
            }
            Self::NeedsHumanReview => {
                format!(
                    "Continuation chain paused — needs human review after \
                     {iterations} iterations, ${cost:.2}"
                )
            }
            Self::TaskError(msg) => {
                format!(
                    "Continuation chain failed — task error after \
                     {iterations} iterations, ${cost:.2}: {msg}"
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmed_complete_message() {
        let reason = ChainTerminationReason::ConfirmedComplete;
        let msg = reason.notification_message(5, 2.50);
        assert!(msg.contains("completed"));
        assert!(msg.contains("5 iterations"));
        assert!(msg.contains("$2.50"));
    }

    #[test]
    fn budget_exhausted_iterations_message() {
        let reason = ChainTerminationReason::BudgetExhausted(BudgetDimension::Iterations);
        let msg = reason.notification_message(10, 4.99);
        assert!(msg.contains("iteration limit"));
        assert!(msg.contains("10 iterations"));
    }

    #[test]
    fn budget_exhausted_cost_message() {
        let reason = ChainTerminationReason::BudgetExhausted(BudgetDimension::Cost);
        let msg = reason.notification_message(4, 5.20);
        assert!(msg.contains("cost limit"));
        assert!(msg.contains("$5.20"));
    }

    #[test]
    fn no_progress_message() {
        let reason = ChainTerminationReason::NoProgress;
        let msg = reason.notification_message(3, 1.00);
        assert!(msg.contains("no meaningful progress"));
    }

    #[test]
    fn user_cancelled_message() {
        let reason = ChainTerminationReason::UserCancelled;
        let msg = reason.notification_message(2, 0.50);
        assert!(msg.contains("cancelled by user"));
    }

    #[test]
    fn needs_human_review_message() {
        let reason = ChainTerminationReason::NeedsHumanReview;
        let msg = reason.notification_message(1, 0.25);
        assert!(msg.contains("needs human review"));
    }

    #[test]
    fn task_error_message() {
        let reason = ChainTerminationReason::TaskError("provider timeout".to_string());
        let msg = reason.notification_message(3, 1.50);
        assert!(msg.contains("task error"));
        assert!(msg.contains("provider timeout"));
    }

    // Snapshot tests pin the exact wording of user-facing notifications so
    // unintended text changes surface in review. Update via `cargo insta review`.

    #[test]
    fn snapshot_confirmed_complete_notification() {
        let reason = ChainTerminationReason::ConfirmedComplete;
        insta::assert_snapshot!(reason.notification_message(5, 2.50));
    }

    #[test]
    fn snapshot_budget_exhausted_iterations_notification() {
        let reason = ChainTerminationReason::BudgetExhausted(super::BudgetDimension::Iterations);
        insta::assert_snapshot!(reason.notification_message(5, 2.50));
    }

    #[test]
    fn snapshot_budget_exhausted_cost_notification() {
        let reason = ChainTerminationReason::BudgetExhausted(super::BudgetDimension::Cost);
        insta::assert_snapshot!(reason.notification_message(5, 2.50));
    }

    #[test]
    fn snapshot_no_progress_notification() {
        let reason = ChainTerminationReason::NoProgress;
        insta::assert_snapshot!(reason.notification_message(5, 2.50));
    }

    #[test]
    fn snapshot_user_cancelled_notification() {
        let reason = ChainTerminationReason::UserCancelled;
        insta::assert_snapshot!(reason.notification_message(5, 2.50));
    }

    #[test]
    fn snapshot_needs_human_review_notification() {
        let reason = ChainTerminationReason::NeedsHumanReview;
        insta::assert_snapshot!(reason.notification_message(5, 2.50));
    }

    #[test]
    fn snapshot_task_error_notification() {
        let reason = ChainTerminationReason::TaskError("provider timeout".to_string());
        insta::assert_snapshot!(reason.notification_message(5, 2.50));
    }
}
