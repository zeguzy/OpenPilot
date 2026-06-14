use std::sync::Arc;

use opca_cli::commands::{SlashCommand, SlashError};
use opca_cli::mock::{format_task_line, format_task_list, format_task_status};
use opca_cli::repl::{BufferOutput, HandleOutcome, Output, Repl};
use opca_cli::{MockOrchestrator, Notification, OrchestratorApi, Reply, TaskInfo};
use opca_core::lifecycle::TaskStatus;

fn task_info(id: &str, status: TaskStatus, progress: f64, summary: &str) -> TaskInfo {
    TaskInfo {
        id: id.to_string(),
        description: format!("desc-{id}"),
        status,
        progress,
        summary: summary.to_string(),
        files_modified: 0,
    }
}

fn make_repl(mock: Arc<MockOrchestrator>) -> (Arc<Repl>, Arc<BufferOutput>) {
    let buffer = Arc::new(BufferOutput::new());
    let output: Arc<dyn Output> = buffer.clone();
    let repl = Arc::new(Repl::new(mock, output));
    (repl, buffer)
}

#[test]
fn slash_accept_parses_task_id() {
    let cmd = SlashCommand::parse("/accept task-3").unwrap().unwrap();
    assert_eq!(
        cmd,
        SlashCommand::Accept {
            task_id: "task-3".to_string(),
        }
    );
}

#[test]
fn slash_reject_parses_feedback_quoted() {
    let cmd = SlashCommand::parse("/reject task-1 \"fix the bug\"")
        .unwrap()
        .unwrap();
    assert_eq!(
        cmd,
        SlashCommand::Reject {
            task_id: "task-1".to_string(),
            feedback: Some("fix the bug".to_string()),
        }
    );
}

#[test]
fn slash_reject_without_feedback() {
    let cmd = SlashCommand::parse("/reject task-9").unwrap().unwrap();
    assert_eq!(
        cmd,
        SlashCommand::Reject {
            task_id: "task-9".to_string(),
            feedback: None,
        }
    );
}

#[test]
fn slash_tasks_recognises_aliases() {
    assert_eq!(
        SlashCommand::parse("/tasks").unwrap().unwrap(),
        SlashCommand::Tasks
    );
    assert_eq!(
        SlashCommand::parse("/running").unwrap().unwrap(),
        SlashCommand::Tasks
    );
    assert_eq!(
        SlashCommand::parse("/jobs").unwrap().unwrap(),
        SlashCommand::Tasks
    );
}

#[test]
fn slash_status_optional_id() {
    assert_eq!(
        SlashCommand::parse("/status").unwrap().unwrap(),
        SlashCommand::Status { task_id: None }
    );
    assert_eq!(
        SlashCommand::parse("/status task-2").unwrap().unwrap(),
        SlashCommand::Status {
            task_id: Some("task-2".to_string()),
        }
    );
}

#[test]
fn slash_help_aliases() {
    assert_eq!(
        SlashCommand::parse("/help").unwrap().unwrap(),
        SlashCommand::Help
    );
    assert_eq!(
        SlashCommand::parse("/?").unwrap().unwrap(),
        SlashCommand::Help
    );
}

#[test]
fn slash_quit_aliases() {
    for alias in ["/quit", "/exit", "/q"] {
        assert_eq!(
            SlashCommand::parse(alias).unwrap().unwrap(),
            SlashCommand::Quit
        );
    }
}

#[test]
fn slash_unknown_returns_error() {
    let result = SlashCommand::parse("/bogus");
    assert!(matches!(result, Err(SlashError::Unknown(_))));
}

#[test]
fn slash_missing_task_id_errors() {
    let result = SlashCommand::parse("/accept");
    assert!(matches!(result, Err(SlashError::MissingTaskId(_))));
}

#[test]
fn non_slash_input_returns_none() {
    assert!(SlashCommand::parse("refactor auth").unwrap().is_none());
    assert!(
        SlashCommand::parse("how is task-0 going?")
            .unwrap()
            .is_none()
    );
}

#[test]
fn format_task_line_uses_emoji_and_status() {
    let info = task_info("task-A", TaskStatus::OnIt, 0.6, "rewriting validator");
    let line = format_task_line(&info);
    assert!(line.contains("task-A"));
    assert!(line.contains("on-it"));
    assert!(line.contains("60%"));
    assert!(line.contains("rewriting validator"));
}

#[test]
fn format_task_line_pending_review_hint() {
    let info = task_info("task-C", TaskStatus::Delivered, 1.0, "done");
    let line = format_task_line(&info);
    assert!(line.contains("pending review"));
    assert!(line.contains("/accept") || line.contains("/reject"));
}

#[test]
fn format_task_list_handles_empty() {
    assert_eq!(format_task_list(&[]), "No tasks.");
}

#[test]
fn format_task_status_shows_progress_and_summary() {
    let info = task_info("task-X", TaskStatus::Pondering, 0.0, "analyzing");
    let s = format_task_status(&info);
    assert!(s.contains("task-X"));
    assert!(s.contains("pondering"));
    assert!(s.contains("0%"));
}

#[test]
fn dispatch_returns_unique_ids() {
    let mock = MockOrchestrator::new();
    let id1 = mock.dispatch("refactor auth");
    let id2 = mock.dispatch("fix bug");
    assert_ne!(id1, id2);
    assert!(id1.starts_with("task-"));
    assert!(id2.starts_with("task-"));
}

#[test]
fn dispatched_task_appears_in_list() {
    let mock = MockOrchestrator::new();
    let id = mock.dispatch("refactor auth module");
    let tasks = mock.list_tasks();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, id);
    assert_eq!(tasks[0].description, "refactor auth module");
    assert_eq!(tasks[0].status, TaskStatus::Pondering);
}

#[test]
fn push_heartbeat_updates_status_and_summary() {
    let mock = MockOrchestrator::new();
    let id = mock.dispatch("fix bug");
    mock.push_heartbeat(&id, TaskStatus::OnIt, 0.5, "editing auth.rs");
    let info = mock.task_status(&id).unwrap();
    assert_eq!(info.status, TaskStatus::OnIt);
    assert!((info.progress - 0.5).abs() < 1e-9);
    assert_eq!(info.summary, "editing auth.rs");
}

#[test]
fn complete_task_emits_notification() {
    let mock = MockOrchestrator::new();
    let mut rx = mock.subscribe();
    let id = mock.dispatch("refactor X");
    mock.complete_task(&id, 5);
    let notif = rx.try_recv().expect("notification should arrive");
    match notif {
        Notification::Completed {
            task_id,
            files_modified,
            ..
        } => {
            assert_eq!(task_id, id);
            assert_eq!(files_modified, 5);
        }
        other @ Notification::StatusChanged { .. } => panic!("expected Completed, got {other:?}"),
    }
}

#[test]
fn pending_review_count_reflects_delivered_tasks() {
    let mock = MockOrchestrator::new();
    let id1 = mock.dispatch("a");
    let id2 = mock.dispatch("b");
    assert_eq!(mock.pending_review_count(), 0);
    mock.complete_task(&id1, 1);
    mock.complete_task(&id2, 1);
    assert_eq!(mock.pending_review_count(), 2);
}

#[test]
fn accept_requires_delivered_status() {
    let mock = MockOrchestrator::new();
    let id = mock.dispatch("a");
    let err = mock.accept(&id).unwrap_err();
    assert!(err.contains("delivered"));
    mock.complete_task(&id, 0);
    mock.accept(&id).expect("accept after delivered");
    assert!(mock.was_accepted(&id));
}

#[test]
fn reject_without_feedback_discards() {
    let mock = MockOrchestrator::new();
    let id = mock.dispatch("a");
    mock.complete_task(&id, 0);
    mock.reject(&id, None).expect("reject");
    assert!(mock.was_rejected(&id));
}

#[test]
fn reject_with_feedback_returns_to_onit() {
    let mock = MockOrchestrator::new();
    let id = mock.dispatch("a");
    mock.complete_task(&id, 0);
    mock.reject(&id, Some("fix the edge case")).expect("reject");
    assert!(mock.was_rejected(&id));
    let info = mock.task_status(&id).unwrap();
    assert_eq!(info.status, TaskStatus::OnIt);
    assert!(info.summary.contains("fix the edge case"));
}

#[test]
fn handle_message_progress_query_returns_status() {
    let mock = MockOrchestrator::new();
    let id = mock.dispatch("refactor auth");
    mock.push_heartbeat(&id, TaskStatus::OnIt, 0.7, "editing files");
    let reply = mock.handle_message(&format!("how is {id} going?"));
    match reply {
        Reply::Foreground(text) => {
            assert!(text.contains(&id));
            assert!(text.contains("on-it"));
        }
        other => panic!("expected Foreground, got {other:?}"),
    }
}

#[test]
fn handle_message_whats_running_returns_list() {
    let mock = MockOrchestrator::new();
    mock.dispatch("task a");
    mock.dispatch("task b");
    let reply = mock.handle_message("what's running?");
    match reply {
        Reply::Foreground(text) => {
            assert!(text.contains("Active Tasks"));
        }
        other => panic!("expected Foreground, got {other:?}"),
    }
}

#[test]
fn repl_tasks_command_lists_active() {
    let mock = Arc::new(MockOrchestrator::new());
    let (repl, buffer) = make_repl(mock);
    repl.handle_line("/tasks");
    assert!(buffer.joined().contains("No tasks."));
}

#[test]
fn repl_accept_command_emits_merge_message() {
    let mock = Arc::new(MockOrchestrator::new());
    let id = mock.dispatch("work");
    mock.complete_task(&id, 3);
    let (repl, buffer) = make_repl(mock.clone());
    repl.handle_line(&format!("/accept {id}"));
    let text = buffer.joined();
    assert!(text.contains("merged"));
    assert!(text.contains(&id));
    assert!(mock.was_accepted(&id));
}

#[test]
fn repl_reject_with_feedback_indicates_return_to_onit() {
    let mock = Arc::new(MockOrchestrator::new());
    let id = mock.dispatch("work");
    mock.complete_task(&id, 0);
    let (repl, buffer) = make_repl(mock);
    repl.handle_line(&format!("/reject {id} \"tighten validation\""));
    let text = buffer.joined();
    assert!(text.contains("OnIt") || text.contains("feedback"));
}

#[test]
fn repl_help_command_prints_help() {
    let mock = Arc::new(MockOrchestrator::new());
    let (repl, buffer) = make_repl(mock);
    repl.handle_line("/help");
    let text = buffer.joined();
    assert!(text.contains("Slash commands"));
    assert!(text.contains("/accept"));
    assert!(text.contains("/tasks"));
}

#[test]
fn repl_quit_returns_quit_outcome() {
    let mock = Arc::new(MockOrchestrator::new());
    let (repl, _buffer) = make_repl(mock);
    let outcome = repl.handle_line("/quit");
    assert_eq!(outcome, HandleOutcome::Quit);
}

#[test]
fn repl_dispatches_background_message() {
    let mock = Arc::new(MockOrchestrator::new());
    let (repl, buffer) = make_repl(mock.clone());
    repl.handle_line("refactor the auth module");
    let text = buffer.joined();
    assert!(text.contains("dispatched"));
    assert_eq!(mock.list_tasks().len(), 1);
}

#[test]
fn repl_renders_completion_notification() {
    let mock = Arc::new(MockOrchestrator::new());
    let id = mock.dispatch("refactor Y");
    let (repl, buffer) = make_repl(mock);
    let notif = Notification::Completed {
        task_id: id.clone(),
        description: "refactor Y".to_string(),
        files_modified: 5,
    };
    repl.render_notification(&notif);
    let text = buffer.joined();
    assert!(text.contains("\u{1F514}"));
    assert!(text.contains(&id));
    assert!(text.contains('5'));
}

#[test]
fn repl_pending_review_indicator_in_prompt() {
    let mock = Arc::new(MockOrchestrator::new());
    let id = mock.dispatch("x");
    mock.complete_task(&id, 0);
    let (repl, _buffer) = make_repl(mock);
    let prompt = repl.prompt_indicator();
    assert!(prompt.contains("pending review"));
    assert!(prompt.contains("/tasks"));
}

#[test]
fn repl_empty_input_is_ignored() {
    let mock = Arc::new(MockOrchestrator::new());
    let (repl, buffer) = make_repl(mock);
    let outcome = repl.handle_line("   ");
    assert_eq!(outcome, HandleOutcome::Continue);
    assert!(buffer.lines().is_empty());
}

#[test]
fn repl_status_with_id_shows_detailed_status() {
    let mock = Arc::new(MockOrchestrator::new());
    let id = mock.dispatch("implement X");
    mock.push_heartbeat(&id, TaskStatus::OnIt, 0.42, "writing code");
    let (repl, buffer) = make_repl(mock);
    repl.handle_line(&format!("/status {id}"));
    let text = buffer.joined();
    assert!(text.contains(&id));
    assert!(text.contains("42%"));
    assert!(text.contains("writing code"));
}

#[tokio::test]
async fn non_blocking_input_while_task_running() {
    let mock = Arc::new(MockOrchestrator::new());
    let task_id = mock.dispatch("long task");
    let (repl, buffer) = make_repl(mock);
    repl.handle_line(&format!("how is {task_id} going?"));
    assert!(
        !buffer.lines().is_empty(),
        "foreground query must succeed while task is notionally running"
    );
}

#[tokio::test]
async fn notification_appears_on_completion_in_repl_loop() {
    use tokio::sync::mpsc;

    let mock = Arc::new(MockOrchestrator::new());
    let task_id = mock.dispatch("bg work");
    let buffer = Arc::new(BufferOutput::new());
    let output: Arc<dyn Output> = buffer.clone();
    let repl = Arc::new(Repl::new(mock.clone(), output));

    let (input_tx, input_rx) = mpsc::unbounded_channel::<String>();
    let notif_rx = mock.subscribe();

    let repl_clone = repl.clone();
    let handle = tokio::spawn(async move {
        opca_cli::repl::run_main_loop_for_test(repl_clone, input_rx, notif_rx).await;
    });

    mock.complete_task(&task_id, 7);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let text = buffer.joined();
    assert!(
        text.contains("\u{1F514}"),
        "expected notification bell in output: {text}"
    );
    assert!(text.contains(&task_id));

    input_tx.send(String::new()).ok();
    handle.abort();
}

#[tokio::test]
async fn accept_in_loop_triggers_merge() {
    use tokio::sync::mpsc;

    let mock = Arc::new(MockOrchestrator::new());
    let task_id = mock.dispatch("bg");
    mock.complete_task(&task_id, 2);

    let buffer = Arc::new(BufferOutput::new());
    let output: Arc<dyn Output> = buffer.clone();
    let repl = Arc::new(Repl::new(mock.clone(), output));

    let (input_tx, input_rx) = mpsc::unbounded_channel::<String>();
    let (_notif_tx, notif_rx) = mpsc::unbounded_channel::<Notification>();

    let repl_clone = repl.clone();
    let handle = tokio::spawn(async move {
        opca_cli::repl::run_main_loop_for_test(repl_clone, input_rx, notif_rx).await;
    });

    input_tx.send(format!("/accept {task_id}")).expect("send");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    assert!(mock.was_accepted(&task_id));
    assert!(buffer.joined().contains("merged"));

    input_tx.send(String::new()).ok();
    handle.abort();
}

#[tokio::test]
async fn tasks_command_in_loop_lists_active() {
    use tokio::sync::mpsc;

    let mock = Arc::new(MockOrchestrator::new());
    mock.dispatch("one");
    mock.dispatch("two");

    let buffer = Arc::new(BufferOutput::new());
    let output: Arc<dyn Output> = buffer.clone();
    let repl = Arc::new(Repl::new(mock, output));

    let (input_tx, input_rx) = mpsc::unbounded_channel::<String>();
    let (_notif_tx, notif_rx) = mpsc::unbounded_channel::<Notification>();

    let repl_clone = repl.clone();
    let handle = tokio::spawn(async move {
        opca_cli::repl::run_main_loop_for_test(repl_clone, input_rx, notif_rx).await;
    });

    input_tx.send("/tasks".to_string()).expect("send");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let text = buffer.joined();
    assert!(text.contains("Active Tasks"));
    assert!(text.contains("task-0"));
    assert!(text.contains("task-1"));

    input_tx.send(String::new()).ok();
    handle.abort();
}
