//! Heartbeat aggregation — folds sub-task Layer 1 status into the
//! parent Task's heartbeat, and decides which sub-task Layer 2
//! highlights escalate to the parent's stream.

use crate::focus::Severity;
use crate::lifecycle::{Heartbeat, SubTaskHeartbeat, TaskStatus};

/// Folds a list of sub-task heartbeats into a `Vec<SubTaskHeartbeat>`
/// suitable for inclusion in the parent's Layer 1 `subtasks` field.
#[must_use]
pub fn aggregate_subtask_heartbeats(
    subtask_heartbeats: &[(&str, &Heartbeat)],
) -> Vec<SubTaskHeartbeat> {
    subtask_heartbeats
        .iter()
        .map(|(id, hb)| SubTaskHeartbeat {
            id: (*id).to_string(),
            status: hb.status,
            progress: hb.progress,
            in_progress_todo: hb.todo.as_ref().and_then(|t| t.in_progress.clone()),
        })
        .collect()
}

/// Returns `true` if a sub-task highlight with `severity` should be
/// escalated to the parent Task's Layer 2 stream.
///
/// Per spec: severity `Blocking` → forwarded to parent; `Info`/`Warning`
/// stay local to the sub-task's internal log.
#[must_use]
pub const fn should_escalate_highlight(severity: Severity) -> bool {
    matches!(severity, Severity::Blocking)
}

/// Prefixes a sub-task highlight summary with `[subtask <id>]` for
/// escalation to the parent's Layer 2 stream.
#[must_use]
pub fn escalate_summary(subtask_id: &str, original: &str) -> String {
    format!("[subtask {subtask_id}] {original}")
}

/// Extracts the status of a completed sub-task for parent notification.
#[must_use]
pub const fn subtask_completion_status(hb: &Heartbeat) -> TaskStatus {
    hb.status
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lifecycle::TodoSummary;

    fn make_heartbeat(
        task_id: &str,
        status: TaskStatus,
        progress: f64,
        todo: Option<TodoSummary>,
    ) -> Heartbeat {
        Heartbeat {
            task_id: task_id.to_string(),
            status,
            progress,
            summary: "test".to_string(),
            timestamp: 0,
            todo,
            subtasks: Vec::new(),
        }
    }

    #[test]
    fn aggregate_empty_returns_empty() {
        let result = aggregate_subtask_heartbeats(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn aggregate_single_subtask() {
        let hb = make_heartbeat("sub-1", TaskStatus::OnIt, 0.5, None);
        let result = aggregate_subtask_heartbeats(&[("sub-1", &hb)]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "sub-1");
        assert_eq!(result[0].status, TaskStatus::OnIt);
        assert!((result[0].progress - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn aggregate_multiple_subtasks() {
        let hb1 = make_heartbeat("sub-1", TaskStatus::OnIt, 0.5, None);
        let hb2 = make_heartbeat("sub-2", TaskStatus::Pondering, 0.2, None);
        let result = aggregate_subtask_heartbeats(&[("sub-1", &hb1), ("sub-2", &hb2)]);
        assert_eq!(result.len(), 2);
        assert!((result[0].progress - 0.5).abs() < f64::EPSILON);
        assert!((result[1].progress - 0.2).abs() < f64::EPSILON);
    }

    #[test]
    fn aggregate_includes_in_progress_todo() {
        let todo = TodoSummary {
            total: 3,
            completed: 1,
            in_progress: Some("writing tests".to_string()),
        };
        let hb = make_heartbeat("sub-1", TaskStatus::OnIt, 0.3, Some(todo));
        let result = aggregate_subtask_heartbeats(&[("sub-1", &hb)]);
        assert_eq!(result[0].in_progress_todo.as_deref(), Some("writing tests"));
    }

    #[test]
    fn aggregate_none_todo_when_not_set() {
        let hb = make_heartbeat("sub-1", TaskStatus::OnIt, 0.5, None);
        let result = aggregate_subtask_heartbeats(&[("sub-1", &hb)]);
        assert!(result[0].in_progress_todo.is_none());
    }

    #[test]
    fn aggregate_none_in_progress_when_completed() {
        let todo = TodoSummary {
            total: 3,
            completed: 3,
            in_progress: None,
        };
        let hb = make_heartbeat("sub-1", TaskStatus::Delivered, 1.0, Some(todo));
        let result = aggregate_subtask_heartbeats(&[("sub-1", &hb)]);
        assert!(result[0].in_progress_todo.is_none());
    }

    #[test]
    fn escalate_blocking() {
        assert!(should_escalate_highlight(Severity::Blocking));
    }

    #[test]
    fn do_not_escalate_info_and_warning() {
        assert!(!should_escalate_highlight(Severity::Info));
        assert!(!should_escalate_highlight(Severity::Warning));
    }

    #[test]
    fn escalate_summary_prefixes_with_subtask_id() {
        let result = escalate_summary("sub-3", "found security issue");
        assert_eq!(result, "[subtask sub-3] found security issue");
    }

    #[test]
    fn completion_status_extracts_from_heartbeat() {
        let hb = make_heartbeat("sub-1", TaskStatus::Delivered, 1.0, None);
        assert_eq!(subtask_completion_status(&hb), TaskStatus::Delivered);
    }
}
