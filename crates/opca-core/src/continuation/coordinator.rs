//! Continuation coordinator — orchestrates the continuation loop.
//!
//! See `design.md` §D2. The [`ContinuationCoordinator`] is a pluggable
//! component (analogous to `DependencyGraph`) that the completion pipeline
//! consults after each Task finishes. It manages chain state, applies the
//! continuation policy, and produces [`DispatchRequest`]s for the next
//! iteration.

use std::collections::HashMap;
use std::fmt::Write;

use crate::audit::{AuditVerdict, Finding};

use super::budget::{BudgetDimension, ContinuationBudget};
use super::chain::{ChainId, ContinuationChain, IterationRecord};
use super::no_progress::NoProgressDetector;
use super::policy::{ContinuationPolicy, PolicyDecision};
use super::termination::ChainTerminationReason;

/// Maximum length of a single finding field in the prompt seed.
const MAX_FINDING_FIELD_LEN: usize = 500;

/// Request to dispatch a new continuation iteration.
///
/// Produced by [`ContinuationCoordinator::evaluate`] when the policy
/// decides to continue. The caller is responsible for actually dispatching
/// the Task and calling [`ContinuationChain::set_current_task`] with the
/// new Task ID.
#[derive(Debug, Clone)]
pub struct DispatchRequest {
    /// The parent Task that the new iteration continues from.
    pub parent_task_id: String,
    /// Structured seed prompt carrying prior findings and chain context.
    pub prompt_seed: String,
    /// Which chain this iteration belongs to.
    pub chain_id: ChainId,
    /// Which iteration number this will be (1-based).
    pub iteration: u32,
}

/// Orchestrates continuation chains and applies the continuation policy.
pub struct ContinuationCoordinator {
    policy: Box<dyn ContinuationPolicy>,
    chains: HashMap<ChainId, ContinuationChain>,
    no_progress_detectors: HashMap<ChainId, NoProgressDetector>,
    chain_counter: u64,
    confidence_threshold: f64,
}

impl ContinuationCoordinator {
    /// Creates a coordinator with the given policy and confidence threshold.
    ///
    /// `confidence_threshold` is used by the policy to decide whether a
    /// low-confidence `NeedsFix` verdict should escalate to
    /// `NeedsHumanReview`.
    #[must_use]
    pub fn new(policy: Box<dyn ContinuationPolicy>, confidence_threshold: f64) -> Self {
        Self {
            policy,
            chains: HashMap::new(),
            no_progress_detectors: HashMap::new(),
            chain_counter: 0,
            confidence_threshold,
        }
    }

    /// Starts a new continuation chain and returns its ID.
    ///
    /// The chain's root Task is set to `root_task_id`. A fresh
    /// [`NoProgressDetector`] is created alongside it.
    pub fn start_chain(&mut self, root_task_id: String, budget: ContinuationBudget) -> ChainId {
        let chain = ContinuationChain::new(root_task_id, budget);
        let id = chain.id().clone();
        self.no_progress_detectors
            .insert(id.clone(), NoProgressDetector::default());
        self.chains.insert(id.clone(), chain);
        self.chain_counter += 1;
        id
    }

    /// Evaluates a completed Task and optionally requests the next iteration.
    ///
    /// Returns `Some(DispatchRequest)` when the policy decides to continue,
    /// or `None` when the chain terminates (for any reason).
    ///
    /// # Arguments
    /// * `task_id` — The Task that just completed.
    /// * `audit_verdict` — The Audit verdict (if an audit ran).
    /// * `audit_confidence` — Audit confidence (0.0 if no audit).
    /// * `audit_findings` — Findings from the audit report.
    pub fn evaluate(
        &mut self,
        task_id: &str,
        audit_verdict: Option<&AuditVerdict>,
        audit_confidence: f64,
        audit_findings: &[Finding],
    ) -> Option<DispatchRequest> {
        // 1. Find the active chain whose current_task_id matches.
        let chain_id = self
            .chains
            .iter()
            .find(|(_, c)| c.current_task_id() == task_id && c.is_active())
            .map(|(id, _)| id.clone())?;

        // 2. Record the iteration in the chain history.
        let iteration_number = self
            .chains
            .get(&chain_id)
            .map_or(0, |c| c.budget().current_iteration() + 1);
        let record = IterationRecord {
            task_id: task_id.to_string(),
            iteration: iteration_number,
            verdict: audit_verdict.copied(),
            cost_usd: 0.0,
            duration: std::time::Duration::ZERO,
            diff_summary: String::new(),
        };

        // 3. Feed audit findings to the no-progress detector.
        let finding_repeated = self
            .no_progress_detectors
            .get_mut(&chain_id)
            .is_some_and(|d| d.record_audit_findings(audit_findings));

        let chain = self.chains.get_mut(&chain_id)?;
        chain.append_iteration(record);

        // When the detector signals a repeated finding, record it as a
        // no-progress event in the budget so the budget's NoProgress
        // dimension can catch it.
        if finding_repeated {
            chain.budget_mut().record_no_progress();
        }

        // 4. Check budget — if exhausted, terminate and return None.
        if let Some(dim) = chain.budget().exhausted_dimension() {
            let reason = match dim {
                BudgetDimension::NoProgress => ChainTerminationReason::NoProgress,
                other => ChainTerminationReason::BudgetExhausted(other),
            };
            chain.terminate(reason);
            return None;
        }

        // 5. Check no-progress detector (diff-based, independent of budget).
        if let Some(detector) = self.no_progress_detectors.get(&chain_id) {
            if detector.is_no_progress() {
                chain.terminate(ChainTerminationReason::NoProgress);
                return None;
            }
        }

        // 6. Consult the policy.
        let decision = {
            let chain_ref = self.chains.get(&chain_id)?;
            self.policy.decide(
                audit_verdict,
                audit_confidence,
                chain_ref,
                self.confidence_threshold,
            )
        };

        match decision {
            PolicyDecision::Continue(_) => {
                // 7. Construct the prompt seed and increment the budget.
                let prompt_seed = self.prompt_seed_for(&chain_id, audit_findings);
                let chain = self.chains.get_mut(&chain_id)?;
                chain.budget_mut().record_iteration(0.0);
                let iteration = chain.budget().current_iteration();
                let parent_task_id = chain.current_task_id().to_string();
                Some(DispatchRequest {
                    parent_task_id,
                    prompt_seed,
                    chain_id: chain_id.clone(),
                    iteration,
                })
            }
            PolicyDecision::Terminate(reason) => {
                let chain = self.chains.get_mut(&chain_id)?;
                chain.terminate(reason);
                None
            }
        }
    }

    /// Manually terminates a chain with the given reason.
    pub fn terminate(&mut self, chain_id: &ChainId, reason: ChainTerminationReason) {
        if let Some(chain) = self.chains.get_mut(chain_id) {
            chain.terminate(reason);
        }
    }

    pub fn set_current_task(&mut self, chain_id: &ChainId, task_id: String) {
        if let Some(chain) = self.chains.get_mut(chain_id) {
            chain.set_current_task(task_id);
        }
    }

    /// Returns the chain with the given ID, if it exists.
    #[must_use]
    pub fn get_chain(&self, chain_id: &ChainId) -> Option<&ContinuationChain> {
        self.chains.get(chain_id)
    }

    /// Returns all currently active chains.
    #[must_use]
    pub fn list_active_chains(&self) -> Vec<&ContinuationChain> {
        self.chains.values().filter(|c| c.is_active()).collect()
    }

    /// Constructs a structured continuation prompt from the chain context.
    ///
    /// Findings are sanitized: each field is truncated to
    /// [`MAX_FINDING_FIELD_LEN`] characters and control characters are
    /// stripped to prevent prompt injection.
    fn prompt_seed_for(&self, chain_id: &ChainId, findings: &[Finding]) -> String {
        let Some(chain) = self.chains.get(chain_id) else {
            return String::new();
        };

        let next_iteration = chain.budget().current_iteration() + 1;
        let mut seed = format!("=== Continuation Iteration {next_iteration} ===\n\n");

        if !findings.is_empty() {
            seed.push_str("Prior audit findings to address:\n");
            for finding in findings.iter().take(10) {
                let severity = sanitize_field(&format!("{:?}", finding.severity));
                let location = sanitize_field(&finding.location);
                let issue = sanitize_field(&finding.issue);
                let _ = writeln!(seed, "- [{severity}] {location}: {issue}");
            }
            seed.push('\n');
        }

        let completed = chain.iterations().len();
        let _ = writeln!(seed, "Chain context: {completed} iteration(s) completed.");
        seed.push_str("Continue the work, addressing the findings above.\n");

        seed
    }
}

/// Truncates a string to `max_len` characters and strips control characters.
///
/// This sanitization prevents prompt injection from untrusted audit finding
/// text (design.md §R6).
fn sanitize_field(s: &str) -> String {
    let truncated = if s.len() > MAX_FINDING_FIELD_LEN {
        &s[..MAX_FINDING_FIELD_LEN]
    } else {
        s
    };
    truncated.chars().filter(|c| !c.is_control()).collect()
}

impl std::fmt::Debug for ContinuationCoordinator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContinuationCoordinator")
            .field("active_chains", &self.list_active_chains().len())
            .field("total_chains", &self.chain_counter)
            .field("confidence_threshold", &self.confidence_threshold)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::AuditVerdict;
    use crate::continuation::policy::DefaultContinuationPolicy;
    use crate::focus::Severity;
    use std::time::Duration;

    fn make_coordinator() -> ContinuationCoordinator {
        ContinuationCoordinator::new(Box::new(DefaultContinuationPolicy), 0.7)
    }

    fn make_finding(location: &str, issue: &str) -> Finding {
        Finding {
            severity: Severity::Warning,
            location: location.to_string(),
            issue: issue.to_string(),
        }
    }

    #[test]
    fn start_chain_creates_active_chain() {
        let mut coord = make_coordinator();
        let budget = ContinuationBudget::default();
        let id = coord.start_chain("task-root".to_string(), budget);
        let chain = coord.get_chain(&id).expect("chain exists");
        assert!(chain.is_active());
        assert_eq!(chain.root_task_id(), "task-root");
        assert_eq!(chain.current_task_id(), "task-root");
    }

    #[test]
    fn evaluate_with_confirmed_terminates() {
        let mut coord = make_coordinator();
        let id = coord.start_chain("task-a".to_string(), ContinuationBudget::default());

        let result = coord.evaluate("task-a", Some(&AuditVerdict::Confirmed), 0.99, &[]);
        assert!(result.is_none());

        let chain = coord.get_chain(&id).expect("chain exists");
        assert!(!chain.is_active());
        assert_eq!(
            chain.status(),
            &crate::continuation::chain::ChainStatus::Terminated(
                ChainTerminationReason::ConfirmedComplete
            )
        );
    }

    #[test]
    fn evaluate_with_needs_fix_returns_dispatch_request() {
        let mut coord = make_coordinator();
        coord.start_chain("task-a".to_string(), ContinuationBudget::default());

        let finding = make_finding("src/auth.rs", "test_login fails");
        let result = coord.evaluate("task-a", Some(&AuditVerdict::NeedsFix), 0.9, &[finding]);

        let request = result.expect("should dispatch");
        assert_eq!(request.parent_task_id, "task-a");
        assert!(request.prompt_seed.contains("test_login"));
        assert!(request.prompt_seed.contains("Continuation Iteration"));
        assert_eq!(request.iteration, 1);
    }

    #[test]
    fn evaluate_with_false_positive_returns_dispatch_request() {
        let mut coord = make_coordinator();
        coord.start_chain("task-a".to_string(), ContinuationBudget::default());

        let result = coord.evaluate("task-a", Some(&AuditVerdict::FalsePositive), 0.8, &[]);
        assert!(result.is_some());
    }

    #[test]
    fn evaluate_with_needs_human_review_terminates() {
        let mut coord = make_coordinator();
        let id = coord.start_chain("task-a".to_string(), ContinuationBudget::default());

        let result = coord.evaluate("task-a", Some(&AuditVerdict::NeedsHumanReview), 0.6, &[]);
        assert!(result.is_none());

        let chain = coord.get_chain(&id).expect("chain exists");
        assert!(!chain.is_active());
    }

    #[test]
    fn evaluate_with_exhausted_budget_terminates() {
        let mut coord = make_coordinator();
        let id = coord.start_chain(
            "task-a".to_string(),
            ContinuationBudget::new(0, 100.0, Duration::from_secs(3600), 10),
        );

        let result = coord.evaluate("task-a", Some(&AuditVerdict::NeedsFix), 0.9, &[]);
        assert!(result.is_none());

        let chain = coord.get_chain(&id).expect("chain exists");
        assert!(!chain.is_active());
    }

    #[test]
    fn terminate_marks_chain_terminated() {
        let mut coord = make_coordinator();
        let id = coord.start_chain("task-a".to_string(), ContinuationBudget::default());

        coord.terminate(&id, ChainTerminationReason::UserCancelled);

        let chain = coord.get_chain(&id).expect("chain exists");
        assert!(!chain.is_active());
    }

    #[test]
    fn list_active_chains_filters_terminated() {
        let mut coord = make_coordinator();
        let id1 = coord.start_chain("task-a".to_string(), ContinuationBudget::default());
        let _id2 = coord.start_chain("task-b".to_string(), ContinuationBudget::default());

        assert_eq!(coord.list_active_chains().len(), 2);

        coord.terminate(&id1, ChainTerminationReason::UserCancelled);
        assert_eq!(coord.list_active_chains().len(), 1);
    }

    #[test]
    fn prompt_seed_truncates_long_findings() {
        let mut coord = make_coordinator();
        coord.start_chain("task-a".to_string(), ContinuationBudget::default());

        let long_issue = "x".repeat(1000);
        let finding = make_finding("src/main.rs", &long_issue);

        let result = coord.evaluate("task-a", Some(&AuditVerdict::NeedsFix), 0.9, &[finding]);

        let request = result.expect("should dispatch");
        // The issue field should be truncated in the prompt seed.
        assert!(request.prompt_seed.len() < 1200);
    }

    #[test]
    fn prompt_seed_strips_control_characters() {
        let mut coord = make_coordinator();
        coord.start_chain("task-a".to_string(), ContinuationBudget::default());

        let finding = make_finding("src/main.rs", "error\x00\x01injection");
        let result = coord.evaluate("task-a", Some(&AuditVerdict::NeedsFix), 0.9, &[finding]);

        let request = result.expect("should dispatch");
        assert!(!request.prompt_seed.contains('\x00'));
        assert!(!request.prompt_seed.contains('\x01'));
    }

    #[test]
    fn evaluate_unknown_task_returns_none() {
        let mut coord = make_coordinator();
        coord.start_chain("task-a".to_string(), ContinuationBudget::default());

        let result = coord.evaluate("nonexistent", Some(&AuditVerdict::Confirmed), 0.99, &[]);
        assert!(result.is_none());
    }

    #[test]
    fn multiple_iterations_increment_counter() {
        let mut coord = make_coordinator();
        let id = coord.start_chain(
            "task-a".to_string(),
            ContinuationBudget::new(10, 100.0, Duration::from_secs(3600), 10),
        );

        // Iteration 1: NeedsFix → continue.
        let req1 = coord
            .evaluate("task-a", Some(&AuditVerdict::NeedsFix), 0.9, &[])
            .expect("continue");
        assert_eq!(req1.iteration, 1);

        // Simulate the next task being set as current.
        coord
            .chains
            .get_mut(&id)
            .unwrap()
            .set_current_task("task-b".to_string());

        // Iteration 2: NeedsFix → continue.
        let req2 = coord
            .evaluate("task-b", Some(&AuditVerdict::NeedsFix), 0.9, &[])
            .expect("continue");
        assert_eq!(req2.iteration, 2);
    }
}
