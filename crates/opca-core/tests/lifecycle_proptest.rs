use opca_core::di::Clock;
use opca_core::lifecycle::{
    ALL_STATUSES, LifecycleTracker, TaskStatus, TransitionError, is_valid_transition, transition,
};
use opca_test_utils::FakeClock;
use proptest::prelude::*;
use std::sync::Arc;
use std::time::UNIX_EPOCH;

fn task_status_strategy() -> impl Strategy<Value = TaskStatus> {
    prop_oneof![
        Just(TaskStatus::Sleeping),
        Just(TaskStatus::Waking),
        Just(TaskStatus::Pondering),
        Just(TaskStatus::OnIt),
        Just(TaskStatus::Waiting),
        Just(TaskStatus::Reviewing),
        Just(TaskStatus::Delivered),
        Just(TaskStatus::Stuck),
        Just(TaskStatus::Axed),
        Just(TaskStatus::Archived),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        ..ProptestConfig::default()
    })]

    #[test]
    fn fn_transition_matches_validator(
        from in task_status_strategy(),
        to in task_status_strategy(),
    ) {
        let result = transition(from, to);
        prop_assert_eq!(result.is_ok(), is_valid_transition(from, to));
        if let Ok(new) = result {
            prop_assert_eq!(new, to);
        }
    }

    #[test]
    fn invalid_transition_returns_correct_error(
        from in task_status_strategy(),
        to in task_status_strategy(),
    ) {
        if !is_valid_transition(from, to) {
            let err = transition(from, to).unwrap_err();
            prop_assert_eq!(err, TransitionError::InvalidTransition { from, to });
        }
    }

    #[test]
    fn arbitrary_sequence_never_panics(
        seq in proptest::collection::vec(
            (task_status_strategy(), 0.0f64..1.0f64, "[a-z]{0,20}"),
            0..100,
        ),
    ) {
        let clock = Arc::new(FakeClock::new(UNIX_EPOCH)) as Arc<dyn Clock>;
        let mut tracker = LifecycleTracker::new("prop-task", clock);
        for (target, progress, summary) in seq {
            let result = tracker.transition(target, progress, &summary);
            if result.is_ok() {
                let hb = result.unwrap();
                prop_assert!(hb.progress >= 0.0 && hb.progress <= 1.0);
                prop_assert_eq!(hb.status, tracker.current());
                prop_assert_eq!(hb.task_id, "prop-task");
            }
            let current = tracker.current();
            prop_assert!(ALL_STATUSES.contains(&current));
        }
    }

    #[test]
    fn arbitrary_sequence_only_accepts_valid(
        from in task_status_strategy(),
        targets in proptest::collection::vec(task_status_strategy(), 1..50),
    ) {
        let clock = Arc::new(FakeClock::new(UNIX_EPOCH)) as Arc<dyn Clock>;
        let mut tracker = LifecycleTracker::new("seq-task", clock);

        if transition(TaskStatus::Sleeping, from).is_err() {
            return Ok(()); // skip if we can't reach the starting state
        }
        let _ = tracker.transition(from, 0.0, "init");

        for target in targets {
            let valid = is_valid_transition(tracker.current(), target);
            let result = tracker.transition(target, 0.5, "step");
            prop_assert_eq!(result.is_ok(), valid);
        }
    }

    #[test]
    fn cancel_always_reachable_except_archived(
        from in task_status_strategy(),
    ) {
        if from != TaskStatus::Archived && from != TaskStatus::Axed {
            prop_assert!(is_valid_transition(from, TaskStatus::Axed));
        }
    }
}
