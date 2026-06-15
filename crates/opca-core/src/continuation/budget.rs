//! Continuation budget — multi-dimensional safety valve for continuation chains.
//!
//! See `design.md` §D5 for the rationale behind four-dimensional budgets.
//! Each dimension is an independent circuit breaker; exhausting any one
//! terminates the chain immediately.

use std::time::{Duration, Instant};

/// Which budget dimension was exhausted.
///
/// Returned by [`ContinuationBudget::exhausted_dimension`] so callers can
/// report *which* limit was hit, not just that a limit was hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetDimension {
    /// Maximum number of continuation iterations reached.
    Iterations,
    /// Maximum accumulated cost (USD) reached.
    Cost,
    /// Maximum wall-clock duration reached.
    Duration,
    /// Maximum consecutive no-progress rounds reached.
    NoProgress,
}

/// Multi-dimensional budget bounding a continuation chain.
///
/// Any single dimension acts as a circuit breaker — exhausting any one
/// terminates the chain. This prevents cost runaway, time sinks, and
/// doom loops that a single-dimension cap (e.g. iteration count alone)
/// cannot catch.
pub struct ContinuationBudget {
    max_iterations: u32,
    max_total_cost_usd: f64,
    max_total_duration: Duration,
    max_no_progress_rounds: u32,
    current_iteration: u32,
    accumulated_cost_usd: f64,
    started_at: Instant,
    consecutive_no_progress: u32,
}

impl ContinuationBudget {
    /// Creates a budget with the specified limits.
    ///
    /// The `started_at` timestamp is captured at construction time so
    /// that [`Self::elapsed`] measures the full chain lifetime.
    #[must_use]
    pub fn new(
        max_iterations: u32,
        max_total_cost_usd: f64,
        max_total_duration: Duration,
        max_no_progress_rounds: u32,
    ) -> Self {
        Self {
            max_iterations,
            max_total_cost_usd,
            max_total_duration,
            max_no_progress_rounds,
            current_iteration: 0,
            accumulated_cost_usd: 0.0,
            started_at: Instant::now(),
            consecutive_no_progress: 0,
        }
    }

    /// Returns `true` when no dimension has been exhausted.
    ///
    /// This is the negation of [`Self::exhausted_dimension`] — convenient
    /// for guard clauses.
    #[must_use]
    pub fn can_continue(&self) -> bool {
        self.exhausted_dimension().is_none()
    }

    /// Records a completed iteration, adding its cost to the accumulator.
    ///
    /// Called after each continuation task finishes. The iteration counter
    /// drives the `Iterations` budget dimension.
    pub fn record_iteration(&mut self, cost_usd: f64) {
        self.current_iteration += 1;
        self.accumulated_cost_usd += cost_usd;
    }

    /// Increments the consecutive no-progress counter.
    pub const fn record_no_progress(&mut self) {
        self.consecutive_no_progress += 1;
    }

    /// Resets the consecutive no-progress counter to zero.
    ///
    /// Called when meaningful progress is detected (diff changes, new
    /// files touched) so that a single productive iteration clears the
    /// doom-loop risk.
    pub const fn reset_no_progress(&mut self) {
        self.consecutive_no_progress = 0;
    }

    /// Returns the first exhausted dimension, or `None` if all are within limits.
    ///
    /// Dimensions are checked in declaration order so that the most
    /// actionable reason surfaces first.
    #[must_use]
    pub fn exhausted_dimension(&self) -> Option<BudgetDimension> {
        if self.current_iteration >= self.max_iterations {
            return Some(BudgetDimension::Iterations);
        }
        if self.accumulated_cost_usd >= self.max_total_cost_usd {
            return Some(BudgetDimension::Cost);
        }
        if self.elapsed() >= self.max_total_duration {
            return Some(BudgetDimension::Duration);
        }
        if self.consecutive_no_progress >= self.max_no_progress_rounds {
            return Some(BudgetDimension::NoProgress);
        }
        None
    }

    /// Current iteration number (zero-based at chain start).
    #[must_use]
    pub const fn current_iteration(&self) -> u32 {
        self.current_iteration
    }

    /// Total cost accumulated across all iterations so far.
    #[must_use]
    pub const fn accumulated_cost_usd(&self) -> f64 {
        self.accumulated_cost_usd
    }

    /// Wall-clock time since the budget (chain) was created.
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        self.started_at.elapsed()
    }

    /// Maximum iterations allowed.
    #[must_use]
    pub const fn max_iterations(&self) -> u32 {
        self.max_iterations
    }

    /// Maximum total cost in USD.
    #[must_use]
    pub const fn max_total_cost_usd(&self) -> f64 {
        self.max_total_cost_usd
    }

    /// Maximum total duration.
    #[must_use]
    pub const fn max_total_duration(&self) -> Duration {
        self.max_total_duration
    }

    /// Maximum consecutive no-progress rounds.
    #[must_use]
    pub const fn max_no_progress_rounds(&self) -> u32 {
        self.max_no_progress_rounds
    }

    /// Current consecutive no-progress count.
    #[must_use]
    pub const fn consecutive_no_progress(&self) -> u32 {
        self.consecutive_no_progress
    }
}

impl Default for ContinuationBudget {
    /// Conservative engineering defaults per design.md §D5.
    fn default() -> Self {
        Self::new(10, 5.0, Duration::from_secs(30 * 60), 2)
    }
}

impl std::fmt::Debug for ContinuationBudget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContinuationBudget")
            .field("max_iterations", &self.max_iterations)
            .field("max_total_cost_usd", &self.max_total_cost_usd)
            .field("max_total_duration", &self.max_total_duration)
            .field("max_no_progress_rounds", &self.max_no_progress_rounds)
            .field("current_iteration", &self.current_iteration)
            .field("accumulated_cost_usd", &self.accumulated_cost_usd)
            .field("consecutive_no_progress", &self.consecutive_no_progress)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_values() {
        let b = ContinuationBudget::default();
        assert_eq!(b.max_iterations(), 10);
        assert!((b.max_total_cost_usd() - 5.0).abs() < f64::EPSILON);
        assert_eq!(b.max_total_duration(), Duration::from_secs(30 * 60));
        assert_eq!(b.max_no_progress_rounds(), 2);
        assert_eq!(b.current_iteration(), 0);
        assert!(b.accumulated_cost_usd().abs() < f64::EPSILON);
        assert_eq!(b.consecutive_no_progress(), 0);
    }

    #[test]
    fn can_continue_when_under_all_limits() {
        let b = ContinuationBudget::new(10, 5.0, Duration::from_secs(1800), 2);
        assert!(b.can_continue());
        assert!(b.exhausted_dimension().is_none());
    }

    #[test]
    fn iterations_exhaustion() {
        let mut b = ContinuationBudget::new(3, 100.0, Duration::from_secs(3600), 10);
        b.record_iteration(1.0);
        b.record_iteration(1.0);
        b.record_iteration(1.0);
        assert_eq!(b.exhausted_dimension(), Some(BudgetDimension::Iterations));
        assert!(!b.can_continue());
    }

    #[test]
    fn cost_exhaustion() {
        let mut b = ContinuationBudget::new(100, 5.0, Duration::from_secs(3600), 10);
        b.record_iteration(3.0);
        assert!(b.can_continue());
        b.record_iteration(3.0);
        assert_eq!(b.exhausted_dimension(), Some(BudgetDimension::Cost));
    }

    #[test]
    fn duration_exhaustion() {
        let b = ContinuationBudget::new(100, 100.0, Duration::ZERO, 10);
        assert_eq!(b.exhausted_dimension(), Some(BudgetDimension::Duration));
    }

    #[test]
    fn no_progress_exhaustion() {
        let mut b = ContinuationBudget::new(100, 100.0, Duration::from_secs(3600), 2);
        b.record_no_progress();
        assert!(b.can_continue());
        b.record_no_progress();
        assert_eq!(b.exhausted_dimension(), Some(BudgetDimension::NoProgress));
    }

    #[test]
    fn reset_no_progress_clears_counter() {
        let mut b = ContinuationBudget::new(100, 100.0, Duration::from_secs(3600), 2);
        b.record_no_progress();
        b.record_no_progress();
        assert_eq!(b.exhausted_dimension(), Some(BudgetDimension::NoProgress));
        b.reset_no_progress();
        assert!(b.can_continue());
    }

    #[test]
    fn record_iteration_accumulates_cost() {
        let mut b = ContinuationBudget::new(100, 100.0, Duration::from_secs(3600), 10);
        b.record_iteration(1.5);
        b.record_iteration(2.5);
        assert_eq!(b.current_iteration(), 2);
        assert!((b.accumulated_cost_usd() - 4.0).abs() < f64::EPSILON);
    }

    #[test]
    fn exhausted_dimension_checks_in_order() {
        let mut b = ContinuationBudget::new(1, 0.0, Duration::ZERO, 1);
        b.record_iteration(1.0);
        b.record_no_progress();
        // Iterations is checked first.
        assert_eq!(b.exhausted_dimension(), Some(BudgetDimension::Iterations));
    }

    // Property tests verify invariants that must hold for *any* sequence of
    // budget operations. A failure means the budget's accounting is subtly
    // wrong, which would silently let continuation chains run past their
    // safety limits — the budget is the only circuit breaker, so its math
    // must be exact.

    use proptest::prelude::*;

    fn budget_strategy() -> impl Strategy<Value = ContinuationBudget> {
        (
            1u32..20,                                           // max_iterations
            1.0f64..1_000.0,                                    // max_total_cost_usd
            (1u64..u64::MAX / 2).prop_map(Duration::from_secs), // max_total_duration (huge → never trips)
            1u32..10,                                           // max_no_progress_rounds
        )
            .prop_map(|(iters, cost, dur, np)| ContinuationBudget::new(iters, cost, dur, np))
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            cases: 256,
            ..ProptestConfig::default()
        })]

        /// After N record_iteration calls (each with `cost`), the accumulated
        /// cost is exactly the sum of recorded costs. This is the fundamental
        /// accounting invariant — if it fails, the Cost dimension cannot be
        /// trusted.
        #[test]
        fn accumulated_cost_equals_sum_of_recorded_costs(
            mut budget in budget_strategy(),
            costs in proptest::collection::vec(0.0f64..50.0, 0..15),
        ) {
            let expected_total: f64 = costs.iter().sum();
            for &c in &costs {
                budget.record_iteration(c);
            }
            prop_assert!(
                (budget.accumulated_cost_usd() - expected_total).abs() < 1e-9,
                "accumulated {} != expected {} for costs {:?}",
                budget.accumulated_cost_usd(),
                expected_total,
                costs,
            );
            prop_assert_eq!(budget.current_iteration() as usize, costs.len());
        }

        /// After any sequence of record_iteration calls, if the budget
        /// reports Iterations exhaustion then current_iteration must be at
        /// least max_iterations. The contrapositive guards against false
        /// negatives (terminating early).
        #[test]
        fn iterations_exhaustion_implies_counter_at_max(
            mut budget in budget_strategy(),
            num_records in 0u32..25,
        ) {
            for _ in 0..num_records {
                budget.record_iteration(0.0);
            }
            if budget.exhausted_dimension() == Some(BudgetDimension::Iterations) {
                prop_assert!(
                    budget.current_iteration() >= budget.max_iterations(),
                    "Iterations exhausted but current {} < max {}",
                    budget.current_iteration(),
                    budget.max_iterations(),
                );
            } else {
                prop_assert!(
                    budget.current_iteration() < budget.max_iterations(),
                    "Iterations not exhausted but current {} >= max {}",
                    budget.current_iteration(),
                    budget.max_iterations(),
                );
            }
        }

        /// `can_continue()` must be the exact negation of
        /// `exhausted_dimension().is_some()` for every reachable state.
        /// The two methods must never disagree — a divergence would mean
        /// either a false-green (chain runs past a limit) or a false-red
        /// (chain stops prematurely).
        #[test]
        fn can_continue_is_negation_of_exhausted_dimension(
            mut budget in budget_strategy(),
            ops in proptest::collection::vec(
                (0u32..5, 0.0f64..10.0, any::<bool>()),
                0..30,
            ),
        ) {
            for (np_rounds, cost, reset) in ops {
                for _ in 0..np_rounds {
                    budget.record_no_progress();
                }
                budget.record_iteration(cost);
                if reset {
                    budget.reset_no_progress();
                }
                let exhausted = budget.exhausted_dimension();
                prop_assert_eq!(
                    budget.can_continue(),
                    exhausted.is_none(),
                    "can_continue disagrees with exhausted_dimension={:?}",
                    exhausted,
                );
            }
        }

        /// Resetting the no-progress counter must restore `can_continue`
        /// when no other dimension is the bottleneck. This is the property
        /// the coordinator relies on when a productive iteration clears the
        /// doom-loop flag without restarting the whole budget.
        #[test]
        fn reset_no_progress_restores_can_continue_when_other_dims_ok(
            max_np in 1u32..5,
            extra_np in 0u32..3,
            cost in 0.0f64..10.0,
        ) {
            // Large caps on iterations / cost / duration so only NoProgress
            // can trip.
            let mut budget = ContinuationBudget::new(
                100,
                1_000.0,
                Duration::from_secs(u64::MAX / 2),
                max_np,
            );
            // Record one iteration so current_iteration advances — this
            // also means the Iterations dimension can never be the blocker.
            budget.record_iteration(cost);

            for _ in 0..(max_np + extra_np) {
                budget.record_no_progress();
            }
            prop_assert_eq!(
                budget.exhausted_dimension(),
                Some(BudgetDimension::NoProgress),
                "precondition: NoProgress must be the exhausted dimension"
            );
            prop_assert!(!budget.can_continue());

            budget.reset_no_progress();
            prop_assert!(
                budget.can_continue(),
                "reset_no_progress must restore can_continue when other dims are within limits"
            );
            prop_assert_eq!(budget.exhausted_dimension(), None);
        }
    }
}
