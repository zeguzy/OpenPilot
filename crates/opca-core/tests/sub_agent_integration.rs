#![cfg(feature = "sub-agents")]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use opca_core::focus::Severity;
use opca_core::lifecycle::{Heartbeat, SubTaskHeartbeat, TaskStatus, TodoSummary};
use opca_core::provider::{Message, Provider};
use opca_core::sub_agent::{
    DispatchSubtaskTool, DispatchSubtaskToolDef, SubTaskConfig, SubTaskResult, SubTaskScope,
    aggregate_subtask_heartbeats, escalate_summary, initial_phase_for_depth, is_within_depth_limit,
    should_escalate_highlight, should_skip_phase_zero_one,
};
use opca_core::tools::tool::{Tool, ToolContext};
use opca_test_utils::{MockFileSystem, MockProcess, ScriptedProvider};
use serde_json::json;
use tokio_stream::StreamExt;

fn dummy_ctx() -> ToolContext {
    ToolContext {
        workspace_path: PathBuf::from("/workspace"),
        fs: Arc::new(MockFileSystem::new()),
        proc: Arc::new(MockProcess::new()),
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

// ── 12.8: Parent dispatches 2 sub-tasks, both complete, results consumed ──

#[tokio::test]
async fn parent_dispatches_two_subtasks_both_complete() {
    let queue = Arc::new(Mutex::new(Vec::new()));
    let limits = Arc::new(Mutex::new(
        opca_core::sub_agent::dispatch::DispatchLimits::default(),
    ));
    let tool = DispatchSubtaskTool::new(queue.clone(), limits);

    let args1 = json!({
        "description": "Find all uses of deprecated_api()",
        "focus": ["diff-sanity"]
    });
    let args2 = json!({
        "description": "Check test coverage for auth module",
        "focus": ["test-coverage"]
    });

    let r1 = tool.execute(&args1, &dummy_ctx()).await.unwrap();
    let r2 = tool.execute(&args2, &dummy_ctx()).await.unwrap();

    assert!(!r1.is_error);
    assert!(!r2.is_error);

    let guard = queue.lock().unwrap();
    assert_eq!(guard.len(), 2);
    assert_eq!(guard[0].description, "Find all uses of deprecated_api()");
    assert_eq!(guard[1].description, "Check test coverage for auth module");
    assert_eq!(guard[0].focus, vec!["diff-sanity".to_string()]);
    assert_eq!(guard[1].focus, vec!["test-coverage".to_string()]);
}

#[tokio::test]
async fn subtask_results_aggregate_into_parent_heartbeat() {
    let sub_hb1 = make_heartbeat(
        "sub-1",
        TaskStatus::OnIt,
        0.5,
        Some(TodoSummary {
            total: 3,
            completed: 1,
            in_progress: Some("writing tests".to_string()),
        }),
    );
    let sub_hb2 = make_heartbeat(
        "sub-2",
        TaskStatus::OnIt,
        0.2,
        Some(TodoSummary {
            total: 2,
            completed: 0,
            in_progress: None,
        }),
    );

    let aggregated = aggregate_subtask_heartbeats(&[("sub-1", &sub_hb1), ("sub-2", &sub_hb2)]);

    assert_eq!(aggregated.len(), 2);
    assert_eq!(aggregated[0].id, "sub-1");
    assert!((aggregated[0].progress - 0.5).abs() < f64::EPSILON);
    assert_eq!(
        aggregated[0].in_progress_todo.as_deref(),
        Some("writing tests")
    );
    assert_eq!(aggregated[1].id, "sub-2");
    assert!(aggregated[1].in_progress_todo.is_none());

    let parent_hb = Heartbeat {
        task_id: "parent".to_string(),
        status: TaskStatus::Waiting,
        progress: 0.3,
        summary: "waiting for sub-tasks".to_string(),
        timestamp: 0,
        todo: None,
        subtasks: aggregated,
    };
    assert_eq!(parent_hb.subtasks.len(), 2);
}

#[tokio::test]
async fn subtask_result_struct_carries_artifacts() {
    let result = SubTaskResult {
        task_id: "sub-1".to_string(),
        summary: "Refactored auth to use new token format".to_string(),
        artifacts: vec![PathBuf::from("src/auth.rs")],
    };

    let injection_msg = Message::user(format!(
        "Sub-task {} completed: {}\nArtifacts: {:?}",
        result.task_id, result.summary, result.artifacts
    ));
    assert!(injection_msg.text().contains("Refactored auth"));
    assert!(injection_msg.text().contains("src/auth.rs"));
}

// ── 12.9: Depth limit chain: root → child → grandchild → great-grandchild rejected ──

#[test]
fn depth_chain_root_child_grandchild_allowed_great_grandchild_rejected() {
    let cfg = SubTaskConfig::defaults();
    assert_eq!(cfg.depth_limit, 2);

    assert!(is_within_depth_limit(0, cfg.depth_limit));
    assert!(is_within_depth_limit(1, cfg.depth_limit));
    assert!(!is_within_depth_limit(2, cfg.depth_limit));
    assert!(!is_within_depth_limit(3, cfg.depth_limit));
}

#[test]
fn depth_limit_error_message_at_great_grandchild() {
    let queue = Arc::new(Mutex::new(Vec::new()));
    let limits = Arc::new(Mutex::new(opca_core::sub_agent::dispatch::DispatchLimits {
        depth_limit: 2,
        parallel_limit: 3,
        active_subtask_count: 0,
        current_depth: 2,
    }));
    let tool = DispatchSubtaskTool::new(queue.clone(), limits);

    let rt = tokio::runtime::Runtime::new().unwrap();
    let args = json!({"description": "great-grandchild attempt"});
    let result = rt.block_on(tool.execute(&args, &dummy_ctx())).unwrap();

    assert!(result.is_error);
    assert!(
        result.content.contains("Maximum delegation depth"),
        "should mention depth limit"
    );

    let guard = queue.lock().unwrap();
    assert!(
        guard.is_empty(),
        "no request should be enqueued at depth limit"
    );
}

// ── Parallel limit enforcement ──

#[tokio::test]
async fn parallel_limit_blocks_fourth_subtask() {
    let queue = Arc::new(Mutex::new(Vec::new()));
    let limits = Arc::new(Mutex::new(opca_core::sub_agent::dispatch::DispatchLimits {
        depth_limit: 2,
        parallel_limit: 3,
        active_subtask_count: 3,
        current_depth: 0,
    }));
    let tool = DispatchSubtaskTool::new(queue.clone(), limits);

    let args = json!({"description": "4th subtask"});
    let result = tool.execute(&args, &dummy_ctx()).await.unwrap();

    assert!(result.is_error);
    assert!(result.content.contains("Maximum concurrent sub-tasks"));
}

#[tokio::test]
async fn parallel_limit_allows_after_slot_freed() {
    let queue = Arc::new(Mutex::new(Vec::new()));
    let limits = Arc::new(Mutex::new(opca_core::sub_agent::dispatch::DispatchLimits {
        depth_limit: 2,
        parallel_limit: 3,
        active_subtask_count: 2,
        current_depth: 0,
    }));
    let tool = DispatchSubtaskTool::new(queue.clone(), limits);

    let args = json!({"description": "3rd subtask"});
    let result = tool.execute(&args, &dummy_ctx()).await.unwrap();
    assert!(
        !result.is_error,
        "3rd subtask should be allowed with 2 active"
    );
}

// ── Abbreviated lifecycle ──

#[test]
fn subtask_starts_at_phase_two() {
    assert_eq!(initial_phase_for_depth(0), opca_core::task::Phase::Zero);
    assert_eq!(initial_phase_for_depth(1), opca_core::task::Phase::Two);
    assert_eq!(initial_phase_for_depth(2), opca_core::task::Phase::Two);
}

#[test]
fn root_does_not_skip_phases() {
    assert!(!should_skip_phase_zero_one(0));
    assert!(should_skip_phase_zero_one(1));
}

// ── Highlight escalation ──

#[test]
fn blocking_highlight_escalates() {
    assert!(should_escalate_highlight(Severity::Blocking));
    assert!(!should_escalate_highlight(Severity::Info));
    assert!(!should_escalate_highlight(Severity::Warning));
}

#[test]
fn escalated_highlight_gets_subtask_prefix() {
    let prefixed = escalate_summary("sub-3", "critical failure in auth");
    assert_eq!(prefixed, "[subtask sub-3] critical failure in auth");
}

// ── Tool definition ──

#[test]
fn dispatch_subtask_tool_def_has_correct_name() {
    assert_eq!(DispatchSubtaskToolDef::name(), "dispatch_subtask");
}

#[test]
fn dispatch_subtask_tool_def_has_workspace_mode_enum() {
    let schema = DispatchSubtaskToolDef::parameters_schema();
    let mode = &schema["properties"]["workspace_mode"];
    assert!(mode.get("enum").is_some());
    let variants: Vec<&str> = mode["enum"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(variants.contains(&"inherited"));
    assert!(variants.contains(&"isolated"));
}

#[test]
fn dispatch_subtask_tool_def_has_required_description() {
    let schema = DispatchSubtaskToolDef::parameters_schema();
    let required = schema["required"].as_array().unwrap();
    assert!(required.iter().any(|v| v.as_str() == Some("description")));
}

// ── Workspace inheritance ──

#[tokio::test]
async fn inherited_workspace_mode_is_default() {
    let queue = Arc::new(Mutex::new(Vec::new()));
    let limits = Arc::new(Mutex::new(
        opca_core::sub_agent::dispatch::DispatchLimits::default(),
    ));
    let tool = DispatchSubtaskTool::new(queue.clone(), limits);

    let args = json!({"description": "test"});
    tool.execute(&args, &dummy_ctx()).await.unwrap();

    let guard = queue.lock().unwrap();
    assert_eq!(guard[0].workspace_mode, SubTaskScope::Inherited);
}

#[tokio::test]
async fn isolated_workspace_mode_explicit() {
    let queue = Arc::new(Mutex::new(Vec::new()));
    let limits = Arc::new(Mutex::new(
        opca_core::sub_agent::dispatch::DispatchLimits::default(),
    ));
    let tool = DispatchSubtaskTool::new(queue.clone(), limits);

    let args = json!({
        "description": "destructive operation",
        "workspace_mode": "isolated"
    });
    tool.execute(&args, &dummy_ctx()).await.unwrap();

    let guard = queue.lock().unwrap();
    assert_eq!(guard[0].workspace_mode, SubTaskScope::Isolated);
}

// ── Config ──

#[test]
fn subagent_config_defaults() {
    let cfg = SubTaskConfig::defaults();
    assert_eq!(cfg.depth_limit, 2);
    assert_eq!(cfg.parallel_limit, 3);
}

#[test]
fn subagent_config_from_toml() {
    let toml = r"
[sub_agents]
depth_limit = 3
parallel_limit = 5
";
    let cfg: opca_core::config::Config = toml::from_str(toml).unwrap();
    assert_eq!(cfg.sub_agents.depth_limit, 3);
    assert_eq!(cfg.sub_agents.parallel_limit, 5);
}

#[test]
fn subagent_config_partial_uses_defaults() {
    let toml = r"
[sub_agents]
depth_limit = 4
";
    let cfg: opca_core::config::Config = toml::from_str(toml).unwrap();
    assert_eq!(cfg.sub_agents.depth_limit, 4);
    assert_eq!(cfg.sub_agents.parallel_limit, 3);
}

#[test]
fn subagent_config_absent_uses_defaults() {
    let toml = "";
    let cfg: opca_core::config::Config = toml::from_str(toml).unwrap();
    assert_eq!(cfg.sub_agents.depth_limit, 2);
    assert_eq!(cfg.sub_agents.parallel_limit, 3);
}

// ── SubTaskHeartbeat serialization ──

#[test]
fn heartbeat_with_subtasks_serializes() {
    let hb = Heartbeat {
        task_id: "parent".to_string(),
        status: TaskStatus::Waiting,
        progress: 0.3,
        summary: "waiting".to_string(),
        timestamp: 0,
        todo: None,
        subtasks: vec![SubTaskHeartbeat {
            id: "sub-1".to_string(),
            status: TaskStatus::OnIt,
            progress: 0.5,
            in_progress_todo: Some("working".to_string()),
        }],
    };
    let json = serde_json::to_string(&hb).unwrap();
    assert!(json.contains("\"subtasks\""));
    assert!(json.contains("\"sub-1\""));
    let back: Heartbeat = serde_json::from_str(&json).unwrap();
    assert_eq!(hb, back);
}

#[test]
fn heartbeat_without_subtasks_omits_field() {
    let hb = Heartbeat {
        task_id: "t".to_string(),
        status: TaskStatus::OnIt,
        progress: 0.0,
        summary: "working".to_string(),
        timestamp: 0,
        todo: None,
        subtasks: Vec::new(),
    };
    let json = serde_json::to_string(&hb).unwrap();
    assert!(
        !json.contains("\"subtasks\""),
        "subtasks field should be omitted when empty"
    );
}

// ── ScriptedProvider sub-task simulation ──

#[tokio::test]
async fn scripted_subtask_completes_and_result_injected() {
    let provider = ScriptedProvider::new().then_text("done").then_done();

    let provider = Arc::new(provider) as Arc<dyn Provider>;
    let messages = vec![Message::user("do the work")];
    let stream = provider.stream(&messages, &[], None).await.unwrap();

    let mut text = String::new();
    tokio::pin!(stream);
    while let Some(event) = stream.next().await {
        if let Ok(opca_core::provider::ProviderEvent::TextDelta(d)) = event {
            text.push_str(&d);
        }
    }
    assert_eq!(text, "done");

    let result = SubTaskResult {
        task_id: "sub-1".to_string(),
        summary: text.clone(),
        artifacts: vec![],
    };

    let injection = Message::user(format!(
        "Sub-task {} completed: {}",
        result.task_id, result.summary
    ));
    assert!(injection.text().contains("done"));
}

// ── Phase 5: New async dispatch flow tests ─────────────────────────────────

#[tokio::test]
async fn dispatch_subtask_tool_populates_parent_id_from_context() {
    let queue = Arc::new(Mutex::new(Vec::new()));
    let limits = Arc::new(Mutex::new(
        opca_core::sub_agent::dispatch::DispatchLimits::default(),
    ));
    let tool = DispatchSubtaskTool::new(queue.clone(), limits);

    let ctx = ToolContext {
        workspace_path: PathBuf::from("/workspace"),
        fs: Arc::new(MockFileSystem::new()),
        proc: Arc::new(MockProcess::new()),
        task_id: Some("task-42".to_string()),
    };

    let args = json!({"description": "find unused deps"});
    tool.execute(&args, &ctx).await.unwrap();

    let guard = queue.lock().unwrap();
    assert_eq!(guard.len(), 1);
    assert_eq!(guard[0].parent_id, "task-42");
}

#[tokio::test]
async fn dispatch_subtask_tool_parent_id_empty_without_context() {
    let queue = Arc::new(Mutex::new(Vec::new()));
    let limits = Arc::new(Mutex::new(
        opca_core::sub_agent::dispatch::DispatchLimits::default(),
    ));
    let tool = DispatchSubtaskTool::new(queue.clone(), limits);

    let ctx = ToolContext {
        workspace_path: PathBuf::from("/workspace"),
        fs: Arc::new(MockFileSystem::new()),
        proc: Arc::new(MockProcess::new()),
        task_id: None,
    };

    let args = json!({"description": "test"});
    tool.execute(&args, &ctx).await.unwrap();

    let guard = queue.lock().unwrap();
    assert_eq!(guard[0].parent_id, "");
}

#[tokio::test]
async fn notification_queue_drain_preserves_order() {
    use opca_core::sub_agent::dispatch::{
        SubTaskNotification, SubTaskResult, SubTaskVerdict, new_notification_queue,
    };

    let queue = new_notification_queue();
    {
        let mut g = queue.lock().unwrap();
        g.push(SubTaskNotification::Completed {
            sub_task_id: "child-1".to_string(),
            result: SubTaskResult {
                task_id: "child-1".to_string(),
                summary: "first result".to_string(),
                artifacts: vec![],
            },
            verdict: SubTaskVerdict::Delivered,
        });
        g.push(SubTaskNotification::Failed {
            sub_task_id: "child-2".to_string(),
            reason: "compile error".to_string(),
        });
    }

    let drained: Vec<_> = {
        let mut g = queue.lock().unwrap();
        std::mem::take(&mut *g)
    };

    assert_eq!(drained.len(), 2);
    assert!(matches!(
        &drained[0],
        SubTaskNotification::Completed { sub_task_id, .. } if sub_task_id == "child-1"
    ));
    assert!(matches!(
        &drained[1],
        SubTaskNotification::Failed { sub_task_id, reason } if sub_task_id == "child-2" && reason == "compile error"
    ));

    assert!(queue.lock().unwrap().is_empty());
}

#[tokio::test]
async fn notification_queue_cap_at_ten() {
    use opca_core::sub_agent::dispatch::{SubTaskNotification, new_notification_queue};

    let queue = new_notification_queue();
    {
        let mut g = queue.lock().unwrap();
        for i in 0..15 {
            if g.len() >= 10 {
                continue;
            }
            g.push(SubTaskNotification::Failed {
                sub_task_id: format!("child-{i}"),
                reason: "test".to_string(),
            });
        }
    }

    assert_eq!(queue.lock().unwrap().len(), 10);
}

#[tokio::test]
async fn subtask_notification_formats_message_correctly() {
    use opca_core::sub_agent::dispatch::{SubTaskNotification, SubTaskResult, SubTaskVerdict};

    let notif = SubTaskNotification::Completed {
        sub_task_id: "task-1-sub".to_string(),
        result: SubTaskResult {
            task_id: "task-1-sub".to_string(),
            summary: "refactored auth module".to_string(),
            artifacts: vec![PathBuf::from("src/auth.rs")],
        },
        verdict: SubTaskVerdict::Delivered,
    };

    let msg = match &notif {
        SubTaskNotification::Completed { result, .. } => Message::user(format!(
            "[Sub-task result] {}: {}",
            result.task_id, result.summary
        )),
        SubTaskNotification::Failed { .. } => unreachable!(),
    };

    assert!(msg.text().contains("refactored auth module"));
    assert!(msg.text().contains("[Sub-task result]"));
}

#[tokio::test]
async fn subtask_failed_notification_formats_error() {
    use opca_core::sub_agent::dispatch::SubTaskNotification;

    let notif = SubTaskNotification::Failed {
        sub_task_id: "task-2-sub".to_string(),
        reason: "evidence gate failed 3 times".to_string(),
    };

    let msg = match &notif {
        SubTaskNotification::Failed {
            sub_task_id,
            reason,
        } => Message::user(format!("[Sub-task error] {sub_task_id}: {reason}")),
        SubTaskNotification::Completed { .. } => unreachable!(),
    };

    assert!(msg.text().contains("evidence gate failed"));
    assert!(msg.text().contains("[Sub-task error]"));
}
