use insta::assert_json_snapshot;
use opca_core::di::Clock;
use opca_core::lifecycle::{Heartbeat, LifecycleTracker, TaskStatus};
use opca_test_utils::FakeClock;
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};
use tokio::sync::mpsc;

fn tracker(task_id: &str, epoch: std::time::SystemTime) -> LifecycleTracker {
    LifecycleTracker::new(task_id, Arc::new(FakeClock::new(epoch)) as Arc<dyn Clock>)
}

#[test]
fn tracker_starts_in_sleeping() {
    let t = tracker("t1", UNIX_EPOCH);
    assert_eq!(t.current(), TaskStatus::Sleeping);
}

#[test]
fn transition_returns_heartbeat_with_new_status() {
    let mut t = tracker("t1", UNIX_EPOCH);
    let hb = t
        .transition(TaskStatus::Waking, 0.0, "initializing")
        .unwrap();
    assert_eq!(hb.task_id, "t1");
    assert_eq!(hb.status, TaskStatus::Waking);
    assert!((hb.progress - 0.0).abs() < f64::EPSILON);
    assert_eq!(hb.summary, "initializing");
    assert_eq!(hb.timestamp, 0);
}

#[test]
fn invalid_transition_returns_error_no_state_change() {
    let mut t = tracker("t1", UNIX_EPOCH);
    let err = t
        .transition(TaskStatus::Delivered, 1.0, "skip")
        .unwrap_err();
    assert_eq!(
        err,
        opca_core::lifecycle::TransitionError::InvalidTransition {
            from: TaskStatus::Sleeping,
            to: TaskStatus::Delivered,
        }
    );
    assert_eq!(t.current(), TaskStatus::Sleeping);
}

#[test]
fn heartbeat_pushed_to_channel_on_valid_transition() {
    let (tx, mut rx) = mpsc::unbounded_channel::<Heartbeat>();
    let mut t = tracker("t1", UNIX_EPOCH).with_heartbeat_channel(tx);

    t.transition(TaskStatus::Waking, 0.1, "loading config")
        .unwrap();
    t.transition(TaskStatus::Pondering, 0.2, "thinking")
        .unwrap();
    t.transition(TaskStatus::OnIt, 0.3, "executing").unwrap();

    let hb1 = rx.try_recv().unwrap();
    assert_eq!(hb1.status, TaskStatus::Waking);
    let hb2 = rx.try_recv().unwrap();
    assert_eq!(hb2.status, TaskStatus::Pondering);
    let hb3 = rx.try_recv().unwrap();
    assert_eq!(hb3.status, TaskStatus::OnIt);
    assert!(rx.try_recv().is_err());
}

#[test]
fn no_heartbeat_pushed_on_invalid_transition() {
    let (tx, mut rx) = mpsc::unbounded_channel::<Heartbeat>();
    let mut t = tracker("t1", UNIX_EPOCH).with_heartbeat_channel(tx);

    let _ = t.transition(TaskStatus::Delivered, 1.0, "skip");
    assert!(rx.try_recv().is_err());
}

#[test]
fn timestamp_uses_clock() {
    let epoch = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    let mut t = tracker("t1", epoch);
    let hb = t.transition(TaskStatus::Waking, 0.0, "wake").unwrap();
    assert_eq!(hb.timestamp, 1_700_000_000);
}

#[test]
fn progress_clamped_to_range() {
    let mut t = tracker("t1", UNIX_EPOCH);
    let hb = t.transition(TaskStatus::Waking, 5.0, "over").unwrap();
    assert!((hb.progress - 1.0).abs() < f64::EPSILON);

    let hb = t.transition(TaskStatus::Pondering, -3.0, "under").unwrap();
    assert!((hb.progress - 0.0).abs() < f64::EPSILON);
}

#[test]
fn heartbeat_json_snapshot_on_it() {
    let hb = Heartbeat {
        task_id: "task-alpha".to_string(),
        status: TaskStatus::OnIt,
        progress: 0.0,
        summary: "starting execution".to_string(),
        timestamp: 0,
        todo: None,
        subtasks: Vec::new(),
    };
    assert_json_snapshot!(hb);
}

#[test]
fn heartbeat_json_snapshot_delivered() {
    let hb = Heartbeat {
        task_id: "task-beta".to_string(),
        status: TaskStatus::Delivered,
        progress: 1.0,
        summary: "refactored auth module".to_string(),
        timestamp: 1_700_000_000,
        todo: None,
        subtasks: Vec::new(),
    };
    assert_json_snapshot!(hb);
}

#[test]
fn heartbeat_json_snapshot_stuck() {
    let hb = Heartbeat {
        task_id: "task-gamma".to_string(),
        status: TaskStatus::Stuck,
        progress: 0.42,
        summary: "blocked on API rate limit".to_string(),
        timestamp: 1_699_999_999,
        todo: None,
        subtasks: Vec::new(),
    };
    assert_json_snapshot!(hb);
}

#[test]
fn heartbeat_json_snapshot_waiting() {
    let hb = Heartbeat {
        task_id: "task-delta".to_string(),
        status: TaskStatus::Waiting,
        progress: 0.5,
        summary: "needs clarification on auth flow".to_string(),
        timestamp: 1_000,
        todo: None,
        subtasks: Vec::new(),
    };
    assert_json_snapshot!(hb);
}

#[test]
fn heartbeat_roundtrip_serialize_deserialize() {
    let hb = Heartbeat {
        task_id: "rt".to_string(),
        status: TaskStatus::Pondering,
        progress: 0.33,
        summary: "thinking".to_string(),
        timestamp: 42,
        todo: None,
        subtasks: Vec::new(),
    };
    let json = serde_json::to_string(&hb).unwrap();
    let back: Heartbeat = serde_json::from_str(&json).unwrap();
    assert_eq!(hb, back);
}

#[test]
fn status_serializes_to_kebab_case() {
    assert_eq!(
        serde_json::to_string(&TaskStatus::OnIt).unwrap(),
        "\"on-it\""
    );
    let hb = Heartbeat {
        task_id: "t".to_string(),
        status: TaskStatus::OnIt,
        progress: 0.0,
        summary: "starting execution".to_string(),
        timestamp: 0,
        todo: None,
        subtasks: Vec::new(),
    };
    let json = serde_json::to_string(&hb).unwrap();
    assert!(json.contains("\"on-it\""));
}

#[test]
fn heartbeat_with_todo_serializes_correctly() {
    let hb = Heartbeat {
        task_id: "task-todo".to_string(),
        status: TaskStatus::OnIt,
        progress: 0.5,
        summary: "working".to_string(),
        timestamp: 100,
        todo: Some(opca_core::lifecycle::TodoSummary {
            total: 5,
            completed: 2,
            in_progress: Some("implementing auth module".to_string()),
        }),
        subtasks: Vec::new(),
    };
    let json = serde_json::to_string(&hb).unwrap();
    assert!(json.contains("\"todo\""));
    assert!(json.contains("\"total\":5"));
    assert!(json.contains("\"completed\":2"));
    assert!(json.contains("implementing auth module"));
    let back: Heartbeat = serde_json::from_str(&json).unwrap();
    assert_eq!(hb, back);
}

#[test]
fn heartbeat_with_none_todo_omits_field() {
    let hb = Heartbeat {
        task_id: "no-todo".to_string(),
        status: TaskStatus::OnIt,
        progress: 0.0,
        summary: "trivial".to_string(),
        timestamp: 0,
        todo: None,
        subtasks: Vec::new(),
    };
    let json = serde_json::to_string(&hb).unwrap();
    assert!(
        !json.contains("\"todo\""),
        "todo field should be omitted when None"
    );
}

#[test]
fn heartbeat_with_todo_no_in_progress() {
    let hb = Heartbeat {
        task_id: "no-active".to_string(),
        status: TaskStatus::Pondering,
        progress: 0.3,
        summary: "thinking".to_string(),
        timestamp: 0,
        todo: Some(opca_core::lifecycle::TodoSummary {
            total: 3,
            completed: 3,
            in_progress: None,
        }),
        subtasks: Vec::new(),
    };
    let json = serde_json::to_string(&hb).unwrap();
    assert!(json.contains("\"total\":3"));
    assert!(json.contains("\"completed\":3"));
    assert!(json.contains("\"in_progress\":null"));
}
