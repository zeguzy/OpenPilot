//! Continuation policy — pluggable decision logic for whether to continue.
//!
//! See `design.md` §D4 for the two-layer completion protocol. The default
//! policy implements the mapping from [`AuditVerdict`] to
//! [`PolicyDecision`], but plugins can supply custom logic via the
//! [`ContinuationPolicy`] trait.

use crate::audit::AuditVerdict;

use super::budget::BudgetDimension;
use super::chain::ContinuationChain;
use super::termination::ChainTerminationReason;

/// The policy's verdict: dispatch another iteration or stop the chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyDecision {
    /// Dispatch a new continuation Task.
    Continue(ContinuationReason),
    /// Stop the chain.
    Terminate(ChainTerminationReason),
}

/// Why the policy decided to continue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContinuationReason {
    /// Audit rejected the work — fix the cited findings.
    AuditRejected {
        /// The audit verdict that caused the rejection.
        verdict: AuditVerdict,
        /// How many findings were cited (best-effort; may be 0).
        findings_count: usize,
    },
    /// Tests failed during the iteration.
    TestsFailed {
        /// Names of the failing tests.
        failures: Vec<String>,
    },
    /// The Task self-reported that it is not yet done.
    TaskSelfReportedIncomplete {
        /// Remaining items the Task listed.
        remaining: Vec<String>,
    },
    /// A successor dependency was activated.
    SuccessorActivated {
        /// Number of successors activated.
        successor_count: usize,
    },
}

/// Decision strategy for continuation chains.
///
/// The default implementation follows design.md §D4. Plugins can replace
/// it to customize thresholds, add external CI integration, etc.
pub trait ContinuationPolicy: Send + Sync {
    /// Decides whether to continue or terminate the chain.
    ///
    /// `audit_verdict` is `None` when the Task was low-risk and auto-accepted
    /// without an Audit run. `audit_confidence` is the Audit agent's
    /// confidence score (0.0 when no audit ran). `confidence_threshold`
    /// is the coordinator's configured cutoff.
    #[must_use]
    fn decide(
        &self,
        audit_verdict: Option<&AuditVerdict>,
        audit_confidence: f64,
        chain: &ContinuationChain,
        confidence_threshold: f64,
    ) -> PolicyDecision;
}

/// Default policy implementing the two-layer completion protocol (§D4).
pub struct DefaultContinuationPolicy;

impl ContinuationPolicy for DefaultContinuationPolicy {
    fn decide(
        &self,
        audit_verdict: Option<&AuditVerdict>,
        audit_confidence: f64,
        chain: &ContinuationChain,
        confidence_threshold: f64,
    ) -> PolicyDecision {
        // Budget is checked FIRST — it overrides any verdict.
        if let Some(dim) = chain.budget().exhausted_dimension() {
            let reason = match dim {
                BudgetDimension::NoProgress => ChainTerminationReason::NoProgress,
                other => ChainTerminationReason::BudgetExhausted(other),
            };
            return PolicyDecision::Terminate(reason);
        }

        match audit_verdict {
            Some(AuditVerdict::Confirmed) => {
                PolicyDecision::Terminate(ChainTerminationReason::ConfirmedComplete)
            }
            Some(AuditVerdict::FalsePositive) => {
                PolicyDecision::Continue(ContinuationReason::AuditRejected {
                    verdict: AuditVerdict::FalsePositive,
                    findings_count: 0,
                })
            }
            Some(AuditVerdict::NeedsFix) => {
                // Low confidence in a NeedsFix verdict means we cannot trust
                // the specific fix target — escalate to a human instead.
                if audit_confidence < confidence_threshold {
                    PolicyDecision::Terminate(ChainTerminationReason::NeedsHumanReview)
                } else {
                    PolicyDecision::Continue(ContinuationReason::AuditRejected {
                        verdict: AuditVerdict::NeedsFix,
                        findings_count: 0,
                    })
                }
            }
            Some(AuditVerdict::NeedsHumanReview) => {
                PolicyDecision::Terminate(ChainTerminationReason::NeedsHumanReview)
            }
            // No audit ran (low-risk auto-accept). Treat as confirmed —
            // the continuation chain has no reason to keep going.
            None => PolicyDecision::Terminate(ChainTerminationReason::ConfirmedComplete),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::continuation::budget::ContinuationBudget;
    use crate::continuation::chain::ContinuationChain;
    use std::time::Duration;

    fn make_chain() -> ContinuationChain {
        ContinuationChain::new(
            "task-root".to_string(),
            ContinuationBudget::new(10, 5.0, Duration::from_secs(1800), 2),
        )
    }

    fn make_exhausted_chain() -> ContinuationChain {
        ContinuationChain::new(
            "task-root".to_string(),
            ContinuationBudget::new(0, 100.0, Duration::from_secs(3600), 10),
        )
    }

    #[test]
    fn confirmed_terminates() {
        let chain = make_chain();
        let decision =
            DefaultContinuationPolicy.decide(Some(&AuditVerdict::Confirmed), 0.99, &chain, 0.7);
        assert_eq!(
            decision,
            PolicyDecision::Terminate(ChainTerminationReason::ConfirmedComplete)
        );
    }

    #[test]
    fn false_positive_continues() {
        let chain = make_chain();
        let decision =
            DefaultContinuationPolicy.decide(Some(&AuditVerdict::FalsePositive), 0.8, &chain, 0.7);
        assert!(matches!(
            decision,
            PolicyDecision::Continue(ContinuationReason::AuditRejected {
                verdict: AuditVerdict::FalsePositive,
                ..
            })
        ));
    }

    #[test]
    fn needs_fix_with_high_confidence_continues() {
        let chain = make_chain();
        let decision =
            DefaultContinuationPolicy.decide(Some(&AuditVerdict::NeedsFix), 0.9, &chain, 0.7);
        assert!(matches!(
            decision,
            PolicyDecision::Continue(ContinuationReason::AuditRejected {
                verdict: AuditVerdict::NeedsFix,
                ..
            })
        ));
    }

    #[test]
    fn needs_fix_with_low_confidence_escalates() {
        let chain = make_chain();
        let decision =
            DefaultContinuationPolicy.decide(Some(&AuditVerdict::NeedsFix), 0.5, &chain, 0.7);
        assert_eq!(
            decision,
            PolicyDecision::Terminate(ChainTerminationReason::NeedsHumanReview)
        );
    }

    #[test]
    fn needs_human_review_terminates() {
        let chain = make_chain();
        let decision = DefaultContinuationPolicy.decide(
            Some(&AuditVerdict::NeedsHumanReview),
            0.6,
            &chain,
            0.7,
        );
        assert_eq!(
            decision,
            PolicyDecision::Terminate(ChainTerminationReason::NeedsHumanReview)
        );
    }

    #[test]
    fn no_audit_terminates_as_confirmed() {
        let chain = make_chain();
        let decision = DefaultContinuationPolicy.decide(None, 0.0, &chain, 0.7);
        assert_eq!(
            decision,
            PolicyDecision::Terminate(ChainTerminationReason::ConfirmedComplete)
        );
    }

    #[test]
    fn budget_exhausted_overrides_verdict() {
        let chain = make_exhausted_chain();
        let decision =
            DefaultContinuationPolicy.decide(Some(&AuditVerdict::NeedsFix), 0.9, &chain, 0.7);
        // Budget is exhausted (0 iterations allowed), so terminate even
        // though the verdict would normally continue.
        assert!(matches!(
            decision,
            PolicyDecision::Terminate(ChainTerminationReason::BudgetExhausted(
                BudgetDimension::Iterations
            ))
        ));
    }

    #[test]
    fn budget_checked_before_verdict() {
        let chain = make_exhausted_chain();
        // Even Confirmed is overridden by budget exhaustion.
        let decision =
            DefaultContinuationPolicy.decide(Some(&AuditVerdict::Confirmed), 0.99, &chain, 0.7);
        assert!(matches!(
            decision,
            PolicyDecision::Terminate(ChainTerminationReason::BudgetExhausted(_))
        ));
    }
}
