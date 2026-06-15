//! Abbreviated lifecycle for sub-tasks.
//!
//! Sub-tasks skip Phase 0 (Intent Gate) and Phase 1 (Codebase
//! Assessment) because the parent Task has already done these. Sub-tasks
//! start at Phase 2 (Execution) and still enforce Phase 3 (Evidence
//! Gate) before transitioning to `Delivered`.

use crate::config::SubAgentConfig;
use crate::task::run::Phase;

/// Effective config for sub-task lifecycle behavior.
#[derive(Debug, Clone, Copy)]
pub struct SubTaskConfig {
    pub depth_limit: usize,
    pub parallel_limit: usize,
}

impl SubTaskConfig {
    #[must_use]
    pub const fn from_config(cfg: &SubAgentConfig) -> Self {
        Self {
            depth_limit: cfg.depth_limit,
            parallel_limit: cfg.parallel_limit,
        }
    }

    #[must_use]
    pub const fn defaults() -> Self {
        Self {
            depth_limit: 2,
            parallel_limit: 3,
        }
    }
}

impl Default for SubTaskConfig {
    fn default() -> Self {
        Self::defaults()
    }
}

/// Returns `true` if a Task with the given delegation depth should skip
/// Phase 0 and Phase 1 and start directly at Phase 2.
#[must_use]
pub const fn should_skip_phase_zero_one(delegation_depth: usize) -> bool {
    delegation_depth > 0
}

/// Returns the starting phase for a Task based on its delegation depth.
///
/// Root tasks (depth 0) start at Phase 0. Sub-tasks (depth > 0) start
/// at Phase 2 (Execution).
#[must_use]
pub const fn initial_phase_for_depth(delegation_depth: usize) -> Phase {
    if should_skip_phase_zero_one(delegation_depth) {
        Phase::Two
    } else {
        Phase::Zero
    }
}

/// Checks whether the given depth is within the configured limit.
#[must_use]
pub const fn is_within_depth_limit(depth: usize, limit: usize) -> bool {
    depth < limit
}

/// Checks whether a new sub-task can be spawned given the current
/// active count and the parallel limit.
#[must_use]
pub const fn is_within_parallel_limit(active: usize, limit: usize) -> bool {
    active < limit
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_task_starts_at_phase_zero() {
        assert_eq!(initial_phase_for_depth(0), Phase::Zero);
    }

    #[test]
    fn subtask_starts_at_phase_two() {
        assert_eq!(initial_phase_for_depth(1), Phase::Two);
    }

    #[test]
    fn deep_subtask_also_starts_at_phase_two() {
        assert_eq!(initial_phase_for_depth(2), Phase::Two);
    }

    #[test]
    fn root_does_not_skip_phases() {
        assert!(!should_skip_phase_zero_one(0));
    }

    #[test]
    fn subtask_skips_phases() {
        assert!(should_skip_phase_zero_one(1));
        assert!(should_skip_phase_zero_one(2));
    }

    #[test]
    fn depth_within_limit() {
        assert!(is_within_depth_limit(0, 2));
        assert!(is_within_depth_limit(1, 2));
    }

    #[test]
    fn depth_at_limit_rejected() {
        assert!(!is_within_depth_limit(2, 2));
        assert!(!is_within_depth_limit(3, 2));
    }

    #[test]
    fn parallel_within_limit() {
        assert!(is_within_parallel_limit(0, 3));
        assert!(is_within_parallel_limit(2, 3));
    }

    #[test]
    fn parallel_at_limit_rejected() {
        assert!(!is_within_parallel_limit(3, 3));
        assert!(!is_within_parallel_limit(4, 3));
    }

    #[test]
    fn defaults_match_spec() {
        let cfg = SubTaskConfig::defaults();
        assert_eq!(cfg.depth_limit, 2);
        assert_eq!(cfg.parallel_limit, 3);
    }
}
