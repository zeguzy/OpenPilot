use opca_core::lifecycle::{
    ALL_STATUSES, TaskStatus, TransitionError, is_valid_transition, transition,
};
use rstest::rstest;

const VALID: &[(TaskStatus, TaskStatus)] = &[
    (TaskStatus::Sleeping, TaskStatus::Waking),
    (TaskStatus::Sleeping, TaskStatus::Axed),
    (TaskStatus::Waking, TaskStatus::Pondering),
    (TaskStatus::Waking, TaskStatus::Axed),
    (TaskStatus::Pondering, TaskStatus::OnIt),
    (TaskStatus::Pondering, TaskStatus::Axed),
    (TaskStatus::OnIt, TaskStatus::Pondering),
    (TaskStatus::OnIt, TaskStatus::Waiting),
    (TaskStatus::OnIt, TaskStatus::Delivered),
    (TaskStatus::OnIt, TaskStatus::Stuck),
    (TaskStatus::OnIt, TaskStatus::Axed),
    (TaskStatus::Waiting, TaskStatus::OnIt),
    (TaskStatus::Waiting, TaskStatus::Axed),
    (TaskStatus::Delivered, TaskStatus::Reviewing),
    (TaskStatus::Delivered, TaskStatus::Archived),
    (TaskStatus::Delivered, TaskStatus::Axed),
    (TaskStatus::Reviewing, TaskStatus::Archived),
    (TaskStatus::Reviewing, TaskStatus::OnIt),
    (TaskStatus::Reviewing, TaskStatus::Axed),
    (TaskStatus::Stuck, TaskStatus::OnIt),
    (TaskStatus::Stuck, TaskStatus::Axed),
    (TaskStatus::Axed, TaskStatus::Archived),
];

fn invalid_pairs() -> Vec<(TaskStatus, TaskStatus)> {
    let valid_set: std::collections::HashSet<(TaskStatus, TaskStatus)> =
        VALID.iter().copied().collect();
    let mut invalid: Vec<_> = ALL_STATUSES
        .iter()
        .flat_map(|&from| ALL_STATUSES.iter().map(move |&to| (from, to)))
        .filter(|pair| !valid_set.contains(pair))
        .collect();
    invalid.sort_by_key(|(f, t)| (*f as u8, *t as u8));
    invalid
}

#[rstest]
fn each_valid_transition_succeeds(
    #[values(
        (TaskStatus::Sleeping, TaskStatus::Waking),
        (TaskStatus::Sleeping, TaskStatus::Axed),
        (TaskStatus::Waking, TaskStatus::Pondering),
        (TaskStatus::Waking, TaskStatus::Axed),
        (TaskStatus::Pondering, TaskStatus::OnIt),
        (TaskStatus::Pondering, TaskStatus::Axed),
        (TaskStatus::OnIt, TaskStatus::Pondering),
        (TaskStatus::OnIt, TaskStatus::Waiting),
        (TaskStatus::OnIt, TaskStatus::Delivered),
        (TaskStatus::OnIt, TaskStatus::Stuck),
        (TaskStatus::OnIt, TaskStatus::Axed),
        (TaskStatus::Waiting, TaskStatus::OnIt),
        (TaskStatus::Waiting, TaskStatus::Axed),
        (TaskStatus::Delivered, TaskStatus::Reviewing),
        (TaskStatus::Delivered, TaskStatus::Archived),
        (TaskStatus::Delivered, TaskStatus::Axed),
        (TaskStatus::Reviewing, TaskStatus::Archived),
        (TaskStatus::Reviewing, TaskStatus::OnIt),
        (TaskStatus::Reviewing, TaskStatus::Axed),
        (TaskStatus::Stuck, TaskStatus::OnIt),
        (TaskStatus::Stuck, TaskStatus::Axed),
        (TaskStatus::Axed, TaskStatus::Archived),
    )]
    pair: (TaskStatus, TaskStatus),
) {
    let (from, to) = pair;
    let result = transition(from, to);
    assert!(result.is_ok(), "{from:?} -> {to:?} should be valid");
    assert_eq!(result.unwrap(), to);
    assert!(is_valid_transition(from, to));
}

#[test]
fn all_valid_transitions_succeed_exhaustive() {
    assert_eq!(VALID.len(), 22, "expected exactly 22 valid transitions");
    for &(from, to) in VALID {
        assert!(is_valid_transition(from, to), "{from:?} -> {to:?}");
        let result = transition(from, to).expect("should succeed");
        assert_eq!(result, to);
    }
}

#[rstest]
fn each_invalid_transition_rejected(
    #[values(
        (TaskStatus::Sleeping, TaskStatus::Pondering),
        (TaskStatus::Sleeping, TaskStatus::OnIt),
        (TaskStatus::Sleeping, TaskStatus::Waiting),
        (TaskStatus::Sleeping, TaskStatus::Reviewing),
        (TaskStatus::Sleeping, TaskStatus::Delivered),
        (TaskStatus::Sleeping, TaskStatus::Stuck),
        (TaskStatus::Sleeping, TaskStatus::Archived),
        (TaskStatus::Waking, TaskStatus::OnIt),
        (TaskStatus::Waking, TaskStatus::Delivered),
        (TaskStatus::Pondering, TaskStatus::Delivered),
        (TaskStatus::Pondering, TaskStatus::Waiting),
        (TaskStatus::OnIt, TaskStatus::Archived),
        (TaskStatus::OnIt, TaskStatus::Reviewing),
        (TaskStatus::Waiting, TaskStatus::Delivered),
        (TaskStatus::Waiting, TaskStatus::Pondering),
        (TaskStatus::Delivered, TaskStatus::OnIt),
        (TaskStatus::Delivered, TaskStatus::Stuck),
        (TaskStatus::Reviewing, TaskStatus::Delivered),
        (TaskStatus::Reviewing, TaskStatus::Stuck),
        (TaskStatus::Stuck, TaskStatus::Delivered),
        (TaskStatus::Stuck, TaskStatus::Archived),
        (TaskStatus::Archived, TaskStatus::Sleeping),
        (TaskStatus::Archived, TaskStatus::Axed),
        (TaskStatus::Axed, TaskStatus::OnIt),
        (TaskStatus::Axed, TaskStatus::Sleeping),
    )]
    pair: (TaskStatus, TaskStatus),
) {
    let (from, to) = pair;
    assert!(
        !is_valid_transition(from, to),
        "{from:?} -> {to:?} should be invalid"
    );
    let err = transition(from, to).unwrap_err();
    assert_eq!(
        err,
        TransitionError::InvalidTransition { from, to },
        "error should carry from={from:?} to={to:?}"
    );
}

#[test]
fn all_invalid_transitions_rejected_exhaustive() {
    let invalid = invalid_pairs();
    let total_pairs = ALL_STATUSES.len() * ALL_STATUSES.len();
    let expected_invalid = total_pairs - VALID.len();
    assert_eq!(
        invalid.len(),
        expected_invalid,
        "expected {expected_invalid} invalid pairs out of {total_pairs}"
    );
    for (from, to) in invalid {
        assert!(!is_valid_transition(from, to), "{from:?} -> {to:?}");
        assert!(transition(from, to).is_err(), "{from:?} -> {to:?}");
    }
}

#[test]
fn no_self_transition_is_valid() {
    for &s in &ALL_STATUSES {
        assert!(
            !is_valid_transition(s, s),
            "self-transition {s:?} -> {s:?} must be invalid"
        );
    }
}

#[test]
fn archived_is_terminal_no_outgoing() {
    for &to in &ALL_STATUSES {
        assert!(
            !is_valid_transition(TaskStatus::Archived, to),
            "Archived -> {to:?} must be invalid (terminal state)"
        );
    }
}

#[test]
fn cancel_to_axed_from_all_non_axed_non_archived() {
    for &from in &ALL_STATUSES {
        if from == TaskStatus::Axed || from == TaskStatus::Archived {
            continue;
        }
        assert!(
            is_valid_transition(from, TaskStatus::Axed),
            "{from:?} -> Axed should be valid (cancel from any state)"
        );
    }
}

#[test]
fn normal_lifecycle_progression() {
    let steps = [
        (TaskStatus::Sleeping, TaskStatus::Waking),
        (TaskStatus::Waking, TaskStatus::Pondering),
        (TaskStatus::Pondering, TaskStatus::OnIt),
        (TaskStatus::OnIt, TaskStatus::Delivered),
        (TaskStatus::Delivered, TaskStatus::Reviewing),
        (TaskStatus::Reviewing, TaskStatus::Archived),
    ];
    let mut current = TaskStatus::Sleeping;
    for (from, to) in steps {
        assert_eq!(current, from);
        current = transition(from, to).unwrap();
    }
    assert_eq!(current, TaskStatus::Archived);
    assert!(current.is_terminal());
}

#[test]
fn task_gets_stuck_then_recovers() {
    let mut current = TaskStatus::Sleeping;
    for to in [TaskStatus::Waking, TaskStatus::Pondering, TaskStatus::OnIt] {
        current = transition(current, to).unwrap();
    }
    current = transition(current, TaskStatus::Stuck).unwrap();
    current = transition(current, TaskStatus::OnIt).unwrap();
    assert_eq!(current, TaskStatus::OnIt);
}

#[test]
fn task_cancelled_from_on_it() {
    let mut current = TaskStatus::Sleeping;
    for to in [TaskStatus::Waking, TaskStatus::Pondering, TaskStatus::OnIt] {
        current = transition(current, to).unwrap();
    }
    current = transition(current, TaskStatus::Axed).unwrap();
    current = transition(current, TaskStatus::Archived).unwrap();
    assert_eq!(current, TaskStatus::Archived);
}
