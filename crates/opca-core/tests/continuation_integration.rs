//! Integration tests for the continuation pipeline `Continue` outcome.
//!
//! These are black-box tests against the public API of
//! [`ContinuationCoordinator`]. They verify that an `AuditVerdict` flows
//! through the coordinator and produces the expected [`DispatchRequest`]
//! (or termination) without any internal mutation hooks.
//!
//! See `design.md` §D2 for the coordinator contract and task 5.2 for the
//! test coverage matrix.

use std::time::Duration;

use opca_core::audit::{AuditVerdict, Finding};
use opca_core::continuation::chain::ChainStatus;
use opca_core::continuation::{
    BudgetDimension, ChainTerminationReason, ContinuationBudget, ContinuationCoordinator,
    DefaultContinuationPolicy,
};
use opca_core::focus::Severity;

/// Constructs a coordinator with the default policy and a 0.5 confidence
/// cutoff, matching the shape the completion pipeline uses in production.
fn make_coordinator() -> ContinuationCoordinator {
    ContinuationCoordinator::new(Box::new(DefaultContinuationPolicy), 0.5)
}

/// A deterministic finding used across tests so assertions can hard-code the
/// expected substrings.
fn make_finding(location: &str, issue: &str) -> Finding {
    Finding {
        severity: Severity::Warning,
        location: location.to_string(),
        issue: issue.to_string(),
    }
}

/// A generous budget so that the iteration/cost/duration dimensions never
/// trip during a single `evaluate` call.
fn generous_budget() -> ContinuationBudget {
    ContinuationBudget::new(10, 100.0, Duration::from_secs(3600), 10)
}

#[test]
fn coordinator_needs_fix_produces_dispatch_request() {
    let mut coord = make_coordinator();
    let chain_id = coord.start_chain("task-parent".to_string(), generous_budget());

    let verdict = AuditVerdict::NeedsFix;
    let result = coord.evaluate("task-parent", Some(&verdict), 0.9, &[]);

    let request = result.expect("NeedsFix with high confidence should dispatch");

    assert_eq!(request.parent_task_id, "task-parent");
    assert!(
        !request.prompt_seed.is_empty(),
        "prompt seed must carry continuation context"
    );
    assert_eq!(request.chain_id, chain_id);
    // After the first `evaluate`, the budget's iteration counter is bumped
    // from 0 to 1, so the dispatched iteration is #1. (The coordinator.rs
    // unit test `evaluate_with_needs_fix_returns_dispatch_request` asserts
    // the same value.)
    assert_eq!(request.iteration, 1);
}

#[test]
fn coordinator_confirmed_terminates_chain() {
    let mut coord = make_coordinator();
    let chain_id = coord.start_chain("task-parent".to_string(), generous_budget());

    let result = coord.evaluate("task-parent", Some(&AuditVerdict::Confirmed), 0.99, &[]);
    assert!(result.is_none(), "Confirmed must not dispatch");

    let chain = coord
        .get_chain(&chain_id)
        .expect("chain must still be registered");
    assert!(!chain.is_active(), "chain must be inactive after Confirmed");
    assert_eq!(
        chain.status(),
        &ChainStatus::Terminated(ChainTerminationReason::ConfirmedComplete),
        "Confirmed must terminate with ConfirmedComplete"
    );
}

#[test]
fn coordinator_false_positive_continues() {
    let mut coord = make_coordinator();
    let chain_id = coord.start_chain("task-parent".to_string(), generous_budget());

    let result = coord.evaluate("task-parent", Some(&AuditVerdict::FalsePositive), 0.8, &[]);

    let request = result.expect("FalsePositive should dispatch another iteration");
    assert_eq!(request.chain_id, chain_id);
    assert_eq!(request.parent_task_id, "task-parent");
    assert!(request.iteration >= 1);

    let chain = coord
        .get_chain(&chain_id)
        .expect("chain must still be registered");
    assert!(
        chain.is_active(),
        "chain must still be active after FalsePositive"
    );
}

#[test]
fn coordinator_prompt_seed_contains_audit_findings() {
    let mut coord = make_coordinator();
    coord.start_chain("task-parent".to_string(), generous_budget());

    let findings = vec![
        make_finding("src/test.rs", "test_login failure"),
        make_finding("src/lib.rs", "missing implementation"),
    ];

    let result = coord.evaluate("task-parent", Some(&AuditVerdict::NeedsFix), 0.9, &findings);

    let request = result.expect("NeedsFix should dispatch");
    let seed = &request.prompt_seed;

    // Both findings must appear in the seed, including their location and
    // the issue text, so the next iteration has actionable context.
    assert!(
        seed.contains("src/test.rs"),
        "prompt seed must mention the finding location: {seed}"
    );
    assert!(
        seed.contains("test_login failure"),
        "prompt seed must mention the finding issue: {seed}"
    );
    assert!(
        seed.contains("src/lib.rs"),
        "prompt seed must mention the second finding location: {seed}"
    );
    assert!(
        seed.contains("missing implementation"),
        "prompt seed must mention the second finding issue: {seed}"
    );
    assert!(
        seed.contains("Continuation Iteration"),
        "prompt seed must be framed as a continuation: {seed}"
    );
}

#[test]
fn coordinator_budget_exhaustion_terminates() {
    let mut coord = make_coordinator();
    // max_iterations = 1: the first `evaluate` bumps the counter to 1, so a
    // second `evaluate` on the same chain observes the exhausted budget and
    // terminates with `BudgetExhausted(Iterations)`.
    let chain_id = coord.start_chain(
        "task-parent".to_string(),
        ContinuationBudget::new(1, 100.0, Duration::from_secs(3600), 10),
    );

    // First evaluation: budget not yet exhausted (0 < 1), so the policy
    // continues and the coordinator records an iteration internally.
    let first = coord.evaluate("task-parent", Some(&AuditVerdict::NeedsFix), 0.9, &[]);
    assert!(
        first.is_some(),
        "first evaluate should still continue before budget trips"
    );

    // Second evaluation: budget now exhausted (1 >= 1). The coordinator
    // must short-circuit before consulting the policy.
    let result = coord.evaluate("task-parent", Some(&AuditVerdict::NeedsFix), 0.9, &[]);
    assert!(result.is_none(), "exhausted budget must not dispatch");

    let chain = coord
        .get_chain(&chain_id)
        .expect("chain must still be registered");
    assert!(!chain.is_active(), "chain must be terminated");
    assert_eq!(
        chain.status(),
        &ChainStatus::Terminated(ChainTerminationReason::BudgetExhausted(
            opca_core::continuation::budget::BudgetDimension::Iterations,
        )),
        "termination reason must cite the Iterations dimension"
    );
}

#[test]
fn coordinator_unknown_task_returns_none() {
    let mut coord = make_coordinator();
    coord.start_chain("task-parent".to_string(), generous_budget());

    let result = coord.evaluate("nonexistent", Some(&AuditVerdict::Confirmed), 0.99, &[]);
    assert!(result.is_none(), "unknown task id must yield None");
}

#[test]
fn coordinator_needs_human_review_terminates() {
    let mut coord = make_coordinator();
    let chain_id = coord.start_chain("task-parent".to_string(), generous_budget());

    let result = coord.evaluate(
        "task-parent",
        Some(&AuditVerdict::NeedsHumanReview),
        0.4,
        &[],
    );
    assert!(result.is_none(), "NeedsHumanReview must not dispatch");

    let chain = coord
        .get_chain(&chain_id)
        .expect("chain must still be registered");
    assert!(!chain.is_active());
    assert_eq!(
        chain.status(),
        &ChainStatus::Terminated(ChainTerminationReason::NeedsHumanReview)
    );
}

// E2E scenarios (task 5.3–5.6): black-box, multi-`evaluate` flows driven
// through the public coordinator API only, mirroring how the completion
// pipeline uses it.

/// E2E: a full chain lifecycle that starts with `NeedsFix`, dispatches a
/// second iteration, then terminates when the second iteration is
/// `Confirmed`.
#[test]
fn e2e_chain_lifecycle_needs_fix_then_confirmed() {
    let mut coord = make_coordinator();
    let chain_id = coord.start_chain("task-iter1".to_string(), generous_budget());

    // Given: iteration 1 returns NeedsFix with high confidence.
    let req1 = coord
        .evaluate(
            "task-iter1",
            Some(&AuditVerdict::NeedsFix),
            0.9,
            &[make_finding("src/lib.rs", "missing impl")],
        )
        .expect("NeedsFix with high confidence should dispatch");
    assert_eq!(req1.chain_id, chain_id);
    assert_eq!(req1.iteration, 1);
    assert!(
        coord
            .get_chain(&chain_id)
            .expect("chain exists")
            .is_active(),
        "chain must remain active after NeedsFix"
    );

    // When: the dispatcher advances the chain to the next Task, which then
    // returns Confirmed.
    coord.set_current_task(&chain_id, "task-iter2".to_string());
    let result = coord.evaluate("task-iter2", Some(&AuditVerdict::Confirmed), 0.99, &[]);
    assert!(result.is_none(), "Confirmed must terminate the chain");

    // Then: chain is terminated with ConfirmedComplete, both iterations are
    // recorded, and the chain is absent from the active list.
    let chain = coord
        .get_chain(&chain_id)
        .expect("chain must still be registered");
    assert!(!chain.is_active(), "chain must be inactive after Confirmed");
    assert_eq!(
        chain.status(),
        &ChainStatus::Terminated(ChainTerminationReason::ConfirmedComplete),
        "termination reason must be ConfirmedComplete"
    );
    assert_eq!(
        chain.iterations().len(),
        2,
        "chain history must hold both iteration records"
    );
    assert_eq!(chain.iterations()[0].task_id, "task-iter1");
    assert_eq!(chain.iterations()[1].task_id, "task-iter2");
    assert!(
        !coord
            .list_active_chains()
            .iter()
            .any(|c| c.id() == &chain_id),
        "terminated chain must not be listed as active"
    );
}

/// E2E: exhausting the `Iterations` budget dimension terminates the chain
/// with `BudgetExhausted(Iterations)` and the notification message cites
/// the iteration limit.
#[test]
fn e2e_budget_exhaustion_terminates_with_correct_reason() {
    let mut coord = make_coordinator();
    // max_iterations = 3: three productive iterations, then the fourth
    // evaluate observes the exhausted budget and terminates.
    let chain_id = coord.start_chain(
        "task-iter1".to_string(),
        ContinuationBudget::new(3, 100.0, Duration::from_secs(3600), 10),
    );

    let mut current_task = "task-iter1".to_string();

    // Drive three continuing iterations.
    for expected in 1..=3 {
        let req = coord
            .evaluate(
                &current_task,
                Some(&AuditVerdict::NeedsFix),
                0.9,
                &[make_finding("src/wip.rs", "still missing impl")],
            )
            .unwrap_or_else(|| panic!("iteration {expected} should continue before budget trips"));
        assert_eq!(req.iteration, expected, "iteration counter must advance");
        assert_eq!(req.chain_id, chain_id);

        // Advance to the next Task ID as the dispatcher would.
        current_task = format!("task-iter{}", expected + 1);
        coord.set_current_task(&chain_id, current_task.clone());
    }

    // Fourth evaluate: budget now exhausted (current_iteration == 3 >= 3).
    let result = coord.evaluate(&current_task, Some(&AuditVerdict::NeedsFix), 0.9, &[]);
    assert!(result.is_none(), "exhausted budget must not dispatch");

    let chain = coord
        .get_chain(&chain_id)
        .expect("chain must still be registered");
    assert!(!chain.is_active(), "chain must be terminated");
    assert_eq!(
        chain.status(),
        &ChainStatus::Terminated(ChainTerminationReason::BudgetExhausted(
            BudgetDimension::Iterations,
        )),
        "termination reason must cite the Iterations dimension"
    );

    // The user-facing notification must mention the iteration limit so the
    // operator knows why the chain stopped.
    let ChainStatus::Terminated(reason) = chain.status() else {
        panic!("chain must be terminated");
    };
    let msg = reason.notification_message(chain.iterations().len() as u32, 0.0);
    assert!(
        msg.contains("iteration limit"),
        "notification must mention 'iteration limit': {msg}"
    );
}

/// E2E: repeated audit findings across iterations trip the no-progress
/// detector and terminate the chain with `NoProgress`.
#[test]
fn e2e_no_progress_detection_terminates_chain() {
    let mut coord = make_coordinator();
    // max_no_progress_rounds = 1 so a single repeated-finding signal from
    // the NoProgressDetector (which fires on the 3rd consecutive identical
    // finding) immediately exhausts the budget's NoProgress dimension.
    let chain_id = coord.start_chain(
        "task-iter1".to_string(),
        ContinuationBudget::new(100, 100.0, Duration::from_secs(3600), 1),
    );

    // Given: the exact same finding signature (location + category) is fed
    // to the detector across three iterations.
    let make_repeated_finding = || make_finding("src/auth.rs", "test_login fails");

    // When: iterations 1 and 2 — the detector has not yet seen 3 consecutive
    // identical findings, so the chain continues.
    let req1 = coord
        .evaluate(
            "task-iter1",
            Some(&AuditVerdict::NeedsFix),
            0.9,
            &[make_repeated_finding()],
        )
        .expect("iteration 1 should continue (no repetition yet)");
    assert_eq!(req1.iteration, 1);
    coord.set_current_task(&chain_id, "task-iter2".to_string());

    let req2 = coord
        .evaluate(
            "task-iter2",
            Some(&AuditVerdict::NeedsFix),
            0.9,
            &[make_repeated_finding()],
        )
        .expect("iteration 2 should continue (no repetition yet)");
    assert_eq!(req2.iteration, 2);
    coord.set_current_task(&chain_id, "task-iter3".to_string());

    // Iteration 3: the 3rd consecutive identical finding trips the detector,
    // which calls budget.record_no_progress(); with max_no_progress_rounds=1
    // the NoProgress dimension is now exhausted → chain terminates.
    let result = coord.evaluate(
        "task-iter3",
        Some(&AuditVerdict::NeedsFix),
        0.9,
        &[make_repeated_finding()],
    );
    assert!(
        result.is_none(),
        "repeated finding must terminate the chain via NoProgress"
    );

    let chain = coord
        .get_chain(&chain_id)
        .expect("chain must still be registered");
    assert!(!chain.is_active(), "chain must be terminated");
    assert_eq!(
        chain.status(),
        &ChainStatus::Terminated(ChainTerminationReason::NoProgress),
        "termination reason must be NoProgress"
    );
    assert_eq!(
        chain.iterations().len(),
        3,
        "all three iterations must be recorded"
    );
    assert!(
        !coord
            .list_active_chains()
            .iter()
            .any(|c| c.id() == &chain_id),
        "no-progress-terminated chain must not be listed as active"
    );
}

/// E2E: a user-initiated `/stop-continuation` terminates the chain
/// immediately and freezes out further dispatch.
#[test]
fn e2e_user_cancellation_stops_chain() {
    let mut coord = make_coordinator();
    let chain_id = coord.start_chain("task-iter1".to_string(), generous_budget());

    // Iteration 1: NeedsFix → dispatch (chain stays active).
    let req = coord
        .evaluate(
            "task-iter1",
            Some(&AuditVerdict::NeedsFix),
            0.9,
            &[make_finding("src/lib.rs", "missing impl")],
        )
        .expect("NeedsFix should dispatch before cancellation");
    assert_eq!(req.iteration, 1);
    assert!(
        coord
            .get_chain(&chain_id)
            .expect("chain exists")
            .is_active()
    );

    // Advance to the next Task ID, then cancel.
    coord.set_current_task(&chain_id, "task-iter2".to_string());
    coord.terminate(&chain_id, ChainTerminationReason::UserCancelled);

    let chain = coord
        .get_chain(&chain_id)
        .expect("chain must still be registered");
    assert!(
        !chain.is_active(),
        "chain must be inactive after cancellation"
    );
    assert_eq!(
        chain.status(),
        &ChainStatus::Terminated(ChainTerminationReason::UserCancelled),
        "termination reason must be UserCancelled"
    );

    // Further evaluate calls on the cancelled chain's task must be no-ops:
    // the coordinator only matches active chains, so a terminated chain is
    // invisible to evaluate.
    let result = coord.evaluate("task-iter2", Some(&AuditVerdict::NeedsFix), 0.9, &[]);
    assert!(
        result.is_none(),
        "terminated chain must not produce further dispatch requests"
    );

    // The cancelled chain must not appear in the active list.
    assert!(
        !coord
            .list_active_chains()
            .iter()
            .any(|c| c.id() == &chain_id),
        "cancelled chain must not be listed as active"
    );
}

// ── Continuation seed enrichment tests (task 10.6) ──────────────
//
// These tests exercise the enriched seed through the public coordinator
// API, verifying that budget visibility, retrospective entries, and the
// no-progress warning appear in the dispatch request's prompt_seed.

#[test]
fn seed_contains_budget_numbers() {
    let mut coord = make_coordinator();
    coord.start_chain("task-a".to_string(), generous_budget());

    let result = coord.evaluate("task-a", Some(&AuditVerdict::NeedsFix), 0.9, &[]);
    let request = result.expect("NeedsFix should dispatch");

    let seed = &request.prompt_seed;
    assert!(
        seed.contains("## Budget"),
        "seed must contain Budget section: {seed}"
    );
    assert!(
        seed.contains("of 10 ("),
        "seed must show max iterations: {seed}"
    );
    assert!(
        seed.contains("$0.00 of $100.00"),
        "seed must show cost budget: {seed}"
    );
}

#[test]
fn seed_contains_retrospective_after_multiple_iterations() {
    let mut coord = make_coordinator();
    let chain_id = coord.start_chain("task-iter1".to_string(), generous_budget());

    let finding1 = make_finding("src/auth.rs", "test_login fails");
    let req1 = coord
        .evaluate(
            "task-iter1",
            Some(&AuditVerdict::NeedsFix),
            0.9,
            &[finding1],
        )
        .expect("iteration 1 should continue");
    assert_eq!(req1.iteration, 1);

    coord.set_current_task(&chain_id, "task-iter2".to_string());

    let finding2 = make_finding("src/auth.rs", "test_login still fails");
    let req2 = coord
        .evaluate(
            "task-iter2",
            Some(&AuditVerdict::NeedsFix),
            0.9,
            &[finding2],
        )
        .expect("iteration 2 should continue");

    let seed = &req2.prompt_seed;
    assert!(
        seed.contains("## Retrospective"),
        "seed for iteration 2 must contain Retrospective section: {seed}"
    );
    assert!(
        seed.contains("Iteration 1 (NeedsFix)"),
        "seed must show iteration 1 in retrospective: {seed}"
    );
    assert!(
        seed.contains("Do not repeat these failed approaches"),
        "seed must contain do-not-repeat instruction: {seed}"
    );
}

#[test]
fn seed_shows_no_progress_warning() {
    let mut coord = make_coordinator();
    let chain_id = coord.start_chain(
        "task-iter1".to_string(),
        ContinuationBudget::new(100, 100.0, Duration::from_secs(3600), 10),
    );

    let make_repeated_finding = || make_finding("src/auth.rs", "same issue persists");

    let req1 = coord
        .evaluate(
            "task-iter1",
            Some(&AuditVerdict::NeedsFix),
            0.9,
            &[make_repeated_finding()],
        )
        .expect("iteration 1 continues");
    assert!(
        !req1.prompt_seed.contains("## No-Progress Warning"),
        "first iteration seed must not have no-progress warning"
    );

    coord.set_current_task(&chain_id, "task-iter2".to_string());
    let req2 = coord
        .evaluate(
            "task-iter2",
            Some(&AuditVerdict::NeedsFix),
            0.9,
            &[make_repeated_finding()],
        )
        .expect("iteration 2 continues");
    assert!(
        !req2.prompt_seed.contains("## No-Progress Warning"),
        "second iteration seed must not have no-progress warning yet"
    );

    coord.set_current_task(&chain_id, "task-iter3".to_string());
    let req3 = coord
        .evaluate(
            "task-iter3",
            Some(&AuditVerdict::NeedsFix),
            0.9,
            &[make_repeated_finding()],
        )
        .expect("iteration 3 continues");

    let seed = &req3.prompt_seed;
    assert!(
        seed.contains("## No-Progress Warning"),
        "third iteration seed must have no-progress warning after repeated findings: {seed}"
    );
    assert!(
        seed.contains("last 1 iteration"),
        "seed must show no-progress count: {seed}"
    );
}
