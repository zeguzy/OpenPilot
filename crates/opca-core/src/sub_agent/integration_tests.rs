//! Integration tests for the sub-agent dispatch + aggregation pipeline.
//!
//! These tests exercise the interaction between the dispatch tool, limit
//! enforcement, lifecycle configuration, and heartbeat aggregation —
//! verifying end-to-end behavior across multiple components.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde_json::json;

use crate::focus::Severity;
use crate::lifecycle::{Heartbeat, TaskStatus, TodoSummary};
use crate::sub_agent::aggregation::{
    aggregate_subtask_heartbeats, escalate_summary, should_escalate_highlight,
};
use crate::sub_agent::dispatch::{
    DispatchLimits, DispatchSubtaskTool, DispatchSubtaskToolDef, SubTaskResult, SubTaskScope,
};
use crate::sub_agent::lifecycle::{
    SubTaskConfig, initial_phase_for_depth, is_within_depth_limit, is_within_parallel_limit,
    should_skip_phase_zero_one,
};
use crate::tools::tool::{Tool, ToolContext};

fn dummy_ctx() -> ToolContext {
    ToolContext {
        workspace_path: PathBuf::from("/workspace"),
        fs: Arc::new(crate::di::StdFileSystem),
        proc: Arc::new(crate::di::StdProcess),
        task_id: None,
    }
}

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

// ── 12.8: Dispatch + aggregation integration ─────────────────────────────

#[tokio::test]
async fn dispatch_and_aggregate_full_flow() {
    let queue = Arc::new(Mutex::new(Vec::new()));
    let limits = Arc::new(Mutex::new(DispatchLimits::default()));
    let tool = DispatchSubtaskTool::new(queue.clone(), limits);

    let args = json!({
        "description": "Find all uses of deprecated_api()",
        "focus": ["diff-sanity"],
        "workspace_mode": "inherited"
    });

    let result = tool.execute(&args, &dummy_ctx()).await.unwrap();
    assert!(!result.is_error);
    assert!(result.content.contains("Sub-task dispatched"));

    let guard = queue.lock().unwrap();
    assert_eq!(guard.len(), 1);
    let request = &guard[0];
    assert_eq!(request.description, "Find all uses of deprecated_api()");
    assert_eq!(request.workspace_mode, SubTaskScope::Inherited);
    assert_eq!(request.focus, vec!["diff-sanity".to_string()]);
    assert_eq!(request.parent_workspace_path, PathBuf::from("/workspace"));
    drop(guard);

    let hb = make_heartbeat("sub-1", TaskStatus::OnIt, 0.5, None);
    let aggregated = aggregate_subtask_heartbeats(&[("sub-1", &hb)]);
    assert_eq!(aggregated.len(), 1);
    assert_eq!(aggregated[0].id, "sub-1");
    assert_eq!(aggregated[0].status, TaskStatus::OnIt);
    assert!((aggregated[0].progress - 0.5).abs() < f64::EPSILON);
}

#[tokio::test]
async fn dispatch_multiple_subtasks_then_aggregate() {
    let queue = Arc::new(Mutex::new(Vec::new()));
    let limits = Arc::new(Mutex::new(DispatchLimits::default()));
    let tool = DispatchSubtaskTool::new(queue.clone(), limits);

    for i in 0..3 {
        let args = json!({"description": format!("subtask {i}")});
        let result = tool.execute(&args, &dummy_ctx()).await.unwrap();
        assert!(!result.is_error, "dispatch {i} should succeed");
    }

    let guard = queue.lock().unwrap();
    assert_eq!(guard.len(), 3, "three requests should be enqueued");
    drop(guard);

    let hb1 = make_heartbeat("sub-0", TaskStatus::OnIt, 0.6, None);
    let hb2 = make_heartbeat("sub-1", TaskStatus::Pondering, 0.1, None);
    let hb3 = make_heartbeat("sub-2", TaskStatus::Delivered, 1.0, None);

    let aggregated =
        aggregate_subtask_heartbeats(&[("sub-0", &hb1), ("sub-1", &hb2), ("sub-2", &hb3)]);

    assert_eq!(aggregated.len(), 3);
    assert_eq!(aggregated[0].status, TaskStatus::OnIt);
    assert_eq!(aggregated[1].status, TaskStatus::Pondering);
    assert_eq!(aggregated[2].status, TaskStatus::Delivered);
}

#[test]
fn highlight_escalation_integration() {
    assert!(should_escalate_highlight(Severity::Blocking));
    assert!(!should_escalate_highlight(Severity::Info));
    assert!(!should_escalate_highlight(Severity::Warning));

    let escalated = escalate_summary("sub-1", "critical failure in module X");
    assert!(escalated.starts_with("[subtask sub-1]"));
    assert!(escalated.contains("critical failure"));
}

#[test]
fn subtask_result_struct_construction() {
    let result = SubTaskResult {
        task_id: "sub-1".to_string(),
        summary: "Refactored auth module".to_string(),
        artifacts: vec![
            PathBuf::from("src/auth.rs"),
            PathBuf::from("src/auth_test.rs"),
        ],
    };
    assert_eq!(result.task_id, "sub-1");
    assert_eq!(result.artifacts.len(), 2);
    assert!(result.artifacts.contains(&PathBuf::from("src/auth.rs")));
}

#[test]
fn subtask_config_from_defaults() {
    let cfg = SubTaskConfig::defaults();
    assert_eq!(cfg.depth_limit, 2);
    assert_eq!(cfg.parallel_limit, 3);
}

// ── 12.9: Depth limit enforcement ────────────────────────────────────────

#[tokio::test]
async fn depth_limit_chain_depth_0_to_1_allowed() {
    let queue = Arc::new(Mutex::new(Vec::new()));
    let limits = Arc::new(Mutex::new(DispatchLimits {
        depth_limit: 2,
        parallel_limit: 3,
        active_subtask_count: 0,
        current_depth: 0,
    }));
    let tool = DispatchSubtaskTool::new(queue.clone(), limits);

    let result = tool
        .execute(&json!({"description": "depth 0→1"}), &dummy_ctx())
        .await
        .unwrap();
    assert!(!result.is_error, "depth 0 should be able to dispatch");
    assert_eq!(queue.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn depth_limit_chain_depth_1_to_2_allowed() {
    let queue = Arc::new(Mutex::new(Vec::new()));
    let limits = Arc::new(Mutex::new(DispatchLimits {
        depth_limit: 2,
        parallel_limit: 3,
        active_subtask_count: 0,
        current_depth: 1,
    }));
    let tool = DispatchSubtaskTool::new(queue.clone(), limits);

    let result = tool
        .execute(&json!({"description": "depth 1→2"}), &dummy_ctx())
        .await
        .unwrap();
    assert!(!result.is_error, "depth 1 should be able to dispatch");
    assert_eq!(queue.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn depth_limit_chain_depth_2_to_3_rejected() {
    let queue = Arc::new(Mutex::new(Vec::new()));
    let limits = Arc::new(Mutex::new(DispatchLimits {
        depth_limit: 2,
        parallel_limit: 3,
        active_subtask_count: 0,
        current_depth: 2,
    }));
    let tool = DispatchSubtaskTool::new(queue.clone(), limits);

    let result = tool
        .execute(&json!({"description": "depth 2→3"}), &dummy_ctx())
        .await
        .unwrap();
    assert!(result.is_error, "depth 2 should NOT be able to dispatch");
    assert!(result.content.contains("Maximum delegation depth"));
    assert!(
        queue.lock().unwrap().is_empty(),
        "no request should be enqueued"
    );
}

#[test]
fn depth_limit_helper_boundaries() {
    assert!(is_within_depth_limit(0, 2));
    assert!(is_within_depth_limit(1, 2));
    assert!(!is_within_depth_limit(2, 2));
    assert!(!is_within_depth_limit(3, 2));
}

#[test]
fn depth_limit_configurable() {
    assert!(
        is_within_depth_limit(2, 3),
        "depth 2 should be ok with limit 3"
    );
    assert!(
        !is_within_depth_limit(3, 3),
        "depth 3 should fail with limit 3"
    );
}

// ── 12.9: Parallel limit enforcement ─────────────────────────────────────

#[tokio::test]
async fn parallel_limit_allows_up_to_limit() {
    let queue = Arc::new(Mutex::new(Vec::new()));
    let limits = Arc::new(Mutex::new(DispatchLimits {
        depth_limit: 2,
        parallel_limit: 3,
        active_subtask_count: 2,
        current_depth: 0,
    }));
    let tool = DispatchSubtaskTool::new(queue.clone(), limits);

    let result = tool
        .execute(&json!({"description": "3rd subtask"}), &dummy_ctx())
        .await
        .unwrap();
    assert!(
        !result.is_error,
        "3rd subtask should be allowed (active=2, limit=3)"
    );
    assert_eq!(queue.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn parallel_limit_rejects_at_limit() {
    let queue = Arc::new(Mutex::new(Vec::new()));
    let limits = Arc::new(Mutex::new(DispatchLimits {
        depth_limit: 2,
        parallel_limit: 3,
        active_subtask_count: 3,
        current_depth: 0,
    }));
    let tool = DispatchSubtaskTool::new(queue.clone(), limits);

    let result = tool
        .execute(&json!({"description": "4th subtask"}), &dummy_ctx())
        .await
        .unwrap();
    assert!(
        result.is_error,
        "4th subtask should be rejected (active=3, limit=3)"
    );
    assert!(result.content.contains("Maximum concurrent sub-tasks"));
    assert!(queue.lock().unwrap().is_empty());
}

#[tokio::test]
async fn parallel_limit_slot_freed_allows_new_dispatch() {
    let queue = Arc::new(Mutex::new(Vec::new()));

    let limits = Arc::new(Mutex::new(DispatchLimits {
        depth_limit: 2,
        parallel_limit: 3,
        active_subtask_count: 3,
        current_depth: 0,
    }));
    let tool = DispatchSubtaskTool::new(queue.clone(), limits.clone());

    let blocked = tool
        .execute(&json!({"description": "4th"}), &dummy_ctx())
        .await
        .unwrap();
    assert!(blocked.is_error, "should be blocked at limit");

    {
        let mut guard = limits.lock().unwrap();
        guard.active_subtask_count = 2;
    }

    let allowed = tool
        .execute(&json!({"description": "new dispatch"}), &dummy_ctx())
        .await
        .unwrap();
    assert!(!allowed.is_error, "should succeed after slot freed");
    assert!(allowed.content.contains("Sub-task dispatched"));
    assert_eq!(queue.lock().unwrap().len(), 1);
}

#[test]
fn parallel_limit_helper_boundaries() {
    assert!(is_within_parallel_limit(0, 3));
    assert!(is_within_parallel_limit(2, 3));
    assert!(!is_within_parallel_limit(3, 3));
    assert!(!is_within_parallel_limit(4, 3));
}

// ── Abbreviated lifecycle integration ─────────────────────────────────────

#[test]
fn subtask_lifecycle_skips_phase_zero_and_one() {
    assert!(!should_skip_phase_zero_one(0), "root task should NOT skip");
    assert!(should_skip_phase_zero_one(1), "depth 1 subtask should skip");
    assert!(should_skip_phase_zero_one(2), "depth 2 subtask should skip");

    assert_eq!(initial_phase_for_depth(0), crate::task::run::Phase::Zero);
    assert_eq!(initial_phase_for_depth(1), crate::task::run::Phase::Two);
    assert_eq!(initial_phase_for_depth(2), crate::task::run::Phase::Two);
}

#[test]
fn tool_def_schema_is_valid() {
    let schema = DispatchSubtaskToolDef::parameters_schema();
    assert_eq!(schema["type"], "object");
    assert!(schema["properties"]["description"]["type"].is_string());
    assert!(schema["properties"]["workspace_mode"]["enum"].is_array());
    let required = schema["required"].as_array().unwrap();
    assert!(required.iter().any(|v| v.as_str() == Some("description")));
}
