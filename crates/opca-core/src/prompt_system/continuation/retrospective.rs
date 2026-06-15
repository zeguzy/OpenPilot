//! Continuation retrospective seed builder.
//!
//! Builds the structured prompt seed injected into each continuation
//! iteration Task. The seed carries four sections so the next iteration
//! knows what to fix, how much budget remains, what prior iterations
//! tried, and whether recent iterations stalled:
//!
//! 1. **Audit Findings** — sanitized findings from the most recent audit.
//! 2. **Budget** — iterations / cost / time consumed vs. limits.
//! 3. **Retrospective** — one-line summary of each prior iteration.
//! 4. **No-Progress Warning** *(optional)* — fires when the chain's
//!    consecutive no-progress counter is non-zero.
//!
//! See `design.md` §D9 for the continuation retrospective rationale.
//! Cold Store integration is deferred per D9; the retrospective draws
//! from in-memory `IterationRecord` history only for now.

use std::fmt::Write;
use std::time::Duration;

use crate::audit::{AuditVerdict, Finding};
use crate::continuation::budget::ContinuationBudget;
use crate::continuation::chain::IterationRecord;
use crate::focus::Severity;

/// Prompt template version. Bump on any material wording change so
/// consumers can correlate model responses with the exact template.
pub const PROMPT_VERSION: &str = "continuation-v2";

/// Maximum length of a single finding field in the prompt seed.
///
/// Defends against prompt inflation and pairs with [`sanitize_field`]'s
/// control-character stripping to prevent prompt injection from
/// untrusted audit finding text (design.md §R6).
pub const MAX_FINDING_FIELD_LEN: usize = 500;

/// Maximum number of retrospective entries (prior iterations) to include.
const MAX_RETROSPECTIVE_ENTRIES: usize = 10;

/// Builds the continuation seed prompt for the next iteration.
///
/// The seed assembles four sections — findings, budget, retrospective,
/// and an optional no-progress warning — so the next iteration Task
/// knows what to fix, how much budget remains, what prior iterations
/// tried, and whether recent iterations stalled.
///
/// This function is pure and does not touch chain state, which keeps
/// it unit-testable in isolation.
///
/// # Arguments
/// * `next_iteration` — The 1-based iteration number the seed targets.
/// * `findings` — Audit findings from the most recent iteration.
/// * `budget` — The chain's continuation budget (read-only).
/// * `iteration_history` — Prior iteration records, oldest first.
#[must_use]
pub fn continuation_seed(
    next_iteration: u32,
    findings: &[Finding],
    budget: &ContinuationBudget,
    iteration_history: &[IterationRecord],
) -> String {
    let mut seed = format!("=== Continuation Iteration {next_iteration} ===\n\n");

    // ── Audit Findings ───────────────────────────────────────────
    if !findings.is_empty() {
        seed.push_str("## Audit Findings (from previous iteration)\n");
        for finding in findings.iter().take(10) {
            let severity = sanitize_field(&format_severity(finding.severity));
            let location = sanitize_field(&finding.location);
            let issue = sanitize_field(&finding.issue);
            let _ = writeln!(seed, "- [{severity}] {location}: {issue}");
        }
        seed.push('\n');
    }

    // ── Budget ───────────────────────────────────────────────────
    let current = budget.current_iteration();
    let max_iters = budget.max_iterations();
    let remaining_iters = max_iters.saturating_sub(current);
    let spent = budget.accumulated_cost_usd();
    let max_cost = budget.max_total_cost_usd();
    let remaining_cost = (max_cost - spent).max(0.0);

    seed.push_str("## Budget\n");
    let _ = writeln!(
        seed,
        "Iteration: {current} of {max_iters} ({remaining_iters} remaining)"
    );
    let _ = writeln!(
        seed,
        "Cost: ${spent:.2} of ${max_cost:.2} (${remaining_cost:.2} remaining)"
    );
    let _ = writeln!(
        seed,
        "Time: {} of {}",
        format_duration(budget.elapsed()),
        format_duration(budget.max_total_duration())
    );
    seed.push('\n');

    // ── Retrospective ────────────────────────────────────────────
    if !iteration_history.is_empty() {
        seed.push_str("## Retrospective (previous iterations)\n");
        for record in iteration_history
            .iter()
            .rev()
            .take(MAX_RETROSPECTIVE_ENTRIES)
        {
            let verdict = format_verdict(record.verdict.as_ref());
            let summary = sanitize_field(&record.diff_summary);
            let cost = record.cost_usd;
            let dur = format_duration(record.duration);
            let _ = writeln!(
                seed,
                "Iteration {} ({verdict}): {summary} — cost ${cost:.2}, duration {dur}",
                record.iteration
            );
        }
        seed.push_str("Do not repeat these failed approaches.\n\n");
    }

    // ── No-Progress Warning ──────────────────────────────────────
    let no_progress = budget.consecutive_no_progress();
    if no_progress > 0 {
        seed.push_str("## No-Progress Warning\n");
        let _ = writeln!(
            seed,
            "⚠ The last {no_progress} iteration(s) produced no meaningful \
             progress. Try a fundamentally different approach."
        );
        seed.push('\n');
    }

    seed.push_str("Continue the work, addressing the findings above.\n");
    seed
}

/// Truncates a string to [`MAX_FINDING_FIELD_LEN`] characters and strips
/// control characters.
///
/// This sanitization prevents prompt injection from untrusted audit
/// finding text (design.md §R6).
#[must_use]
pub fn sanitize_field(s: &str) -> String {
    let truncated = if s.len() > MAX_FINDING_FIELD_LEN {
        &s[..MAX_FINDING_FIELD_LEN]
    } else {
        s
    };
    truncated.chars().filter(|c| !c.is_control()).collect()
}

/// Renders a [`Severity`] as a short lowercase label for the seed.
fn format_severity(severity: Severity) -> String {
    match severity {
        Severity::Blocking => "blocking",
        Severity::Warning => "warning",
        Severity::Info => "info",
    }
    .to_string()
}

/// Renders an [`AuditVerdict`] (or its absence) as a short label.
const fn format_verdict(verdict: Option<&AuditVerdict>) -> &'static str {
    match verdict {
        Some(AuditVerdict::Confirmed) => "Confirmed",
        Some(AuditVerdict::FalsePositive) => "FalsePositive",
        Some(AuditVerdict::NeedsFix) => "NeedsFix",
        Some(AuditVerdict::NeedsHumanReview) => "NeedsHumanReview",
        None => "Unknown",
    }
}

/// Formats a [`Duration`] compactly: seconds for sub-minute spans,
/// minutes otherwise.
fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    if secs >= 60 {
        let mins = secs / 60;
        let rem = secs % 60;
        if rem == 0 {
            format!("{mins}m")
        } else {
            format!("{mins}m{rem}s")
        }
    } else {
        format!("{secs}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_finding(location: &str, issue: &str) -> Finding {
        Finding {
            severity: Severity::Warning,
            location: location.to_string(),
            issue: issue.to_string(),
        }
    }

    fn make_record(iteration: u32, verdict: AuditVerdict, summary: &str) -> IterationRecord {
        IterationRecord {
            task_id: format!("task-{iteration}"),
            iteration,
            verdict: Some(verdict),
            cost_usd: 0.05 * f64::from(iteration),
            duration: Duration::from_secs(u64::from(iteration) * 10),
            diff_summary: summary.to_string(),
        }
    }

    // ── Seed structure tests ─────────────────────────────────────

    #[test]
    fn seed_contains_header() {
        let budget = ContinuationBudget::default();
        let seed = continuation_seed(1, &[], &budget, &[]);
        assert!(
            seed.contains("=== Continuation Iteration 1 ==="),
            "seed must contain iteration header: {seed}"
        );
    }

    #[test]
    fn seed_contains_budget_numbers() {
        let budget = ContinuationBudget::new(10, 5.0, Duration::from_secs(1800), 3);
        let seed = continuation_seed(3, &[], &budget, &[]);
        assert!(
            seed.contains("## Budget"),
            "seed must contain Budget section: {seed}"
        );
        assert!(
            seed.contains("Iteration: 0 of 10 (10 remaining)"),
            "seed must show iteration budget: {seed}"
        );
        assert!(
            seed.contains("$0.00 of $5.00"),
            "seed must show cost budget: {seed}"
        );
        assert!(seed.contains("Time:"), "seed must show time budget: {seed}");
    }

    #[test]
    fn seed_contains_retrospective_entries() {
        let budget = ContinuationBudget::default();
        let history = vec![
            make_record(1, AuditVerdict::NeedsFix, "fixed auth module"),
            make_record(2, AuditVerdict::NeedsFix, "restructured login flow"),
        ];
        let seed = continuation_seed(3, &[], &budget, &history);

        assert!(
            seed.contains("## Retrospective"),
            "seed must contain Retrospective section: {seed}"
        );
        assert!(
            seed.contains("Iteration 1 (NeedsFix)"),
            "seed must show iteration 1 verdict: {seed}"
        );
        assert!(
            seed.contains("fixed auth module"),
            "seed must show iteration 1 summary: {seed}"
        );
        assert!(
            seed.contains("Iteration 2 (NeedsFix)"),
            "seed must show iteration 2 verdict: {seed}"
        );
        assert!(
            seed.contains("restructured login flow"),
            "seed must show iteration 2 summary: {seed}"
        );
        assert!(
            seed.contains("Do not repeat these failed approaches"),
            "seed must contain do-not-repeat instruction: {seed}"
        );
    }

    #[test]
    fn seed_omits_retrospective_when_no_history() {
        let budget = ContinuationBudget::default();
        let seed = continuation_seed(1, &[], &budget, &[]);
        assert!(
            !seed.contains("## Retrospective"),
            "seed must not contain Retrospective when empty history: {seed}"
        );
    }

    #[test]
    fn seed_shows_no_progress_warning_when_counter_nonzero() {
        let mut budget = ContinuationBudget::default();
        budget.record_no_progress();
        budget.record_no_progress();
        let seed = continuation_seed(3, &[], &budget, &[]);

        assert!(
            seed.contains("## No-Progress Warning"),
            "seed must contain No-Progress Warning: {seed}"
        );
        assert!(
            seed.contains("last 2 iteration"),
            "seed must show no-progress count: {seed}"
        );
    }

    #[test]
    fn seed_omits_no_progress_warning_when_counter_zero() {
        let budget = ContinuationBudget::default();
        let seed = continuation_seed(1, &[], &budget, &[]);
        assert!(
            !seed.contains("## No-Progress Warning"),
            "seed must not contain No-Progress Warning when counter is zero: {seed}"
        );
    }

    #[test]
    fn seed_preserves_finding_sanitization() {
        let budget = ContinuationBudget::default();
        let finding = make_finding("src/main.rs", "error\x00\x01injection");
        let seed = continuation_seed(1, &[finding], &budget, &[]);

        assert!(!seed.contains('\x00'), "seed must strip NUL: {seed}");
        assert!(!seed.contains('\x01'), "seed must strip SOH: {seed}");
    }

    #[test]
    fn seed_truncates_long_finding_fields() {
        let budget = ContinuationBudget::default();
        let long_issue = "x".repeat(1000);
        let finding = make_finding("src/main.rs", &long_issue);
        let seed = continuation_seed(1, &[finding], &budget, &[]);

        // The issue field is truncated to MAX_FINDING_FIELD_LEN (500).
        assert!(
            seed.len() < 1200,
            "seed must be truncated: len={}",
            seed.len()
        );
    }

    #[test]
    fn seed_sanitizes_retrospective_summary() {
        let budget = ContinuationBudget::default();
        let record = IterationRecord {
            task_id: "task-1".to_string(),
            iteration: 1,
            verdict: Some(AuditVerdict::NeedsFix),
            cost_usd: 0.01,
            duration: Duration::from_secs(5),
            diff_summary: "bad\x00summary".to_string(),
        };
        let seed = continuation_seed(2, &[], &budget, &[record]);
        assert!(
            !seed.contains('\x00'),
            "retrospective summary must be sanitized: {seed}"
        );
    }

    // ── Snapshot test ────────────────────────────────────────────

    #[test]
    fn snapshot_continuation_seed_full() {
        let mut budget = ContinuationBudget::new(10, 5.0, Duration::from_secs(1800), 2);
        // Simulate two completed iterations.
        budget.record_iteration(0.05);
        budget.record_iteration(0.08);
        budget.record_no_progress();

        let history = vec![
            make_record(1, AuditVerdict::NeedsFix, "attempted fix in auth.rs"),
            make_record(2, AuditVerdict::NeedsFix, "restructured login flow"),
        ];

        let findings = vec![
            make_finding("src/auth.rs", "test_login still fails"),
            make_finding("src/session.rs", "session timeout not handled"),
        ];

        let seed = continuation_seed(3, &findings, &budget, &history);
        insta::assert_snapshot!(seed);
    }
}
