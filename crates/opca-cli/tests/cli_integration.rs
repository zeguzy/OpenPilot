use std::sync::Arc;

use opca_cli::commands::{ContinueAction, SlashCommand, SlashError, StopTarget};
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
        other @ (Notification::StatusChanged { .. } | Notification::Clarification { .. }) => {
            panic!("expected Completed, got {other:?}")
        }
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

#[test]
fn slash_continue_start_parses_prompt() {
    let cmd = SlashCommand::parse("/continue fix all the bugs")
        .unwrap()
        .unwrap();
    assert_eq!(
        cmd,
        SlashCommand::Continue {
            action: ContinueAction::Start {
                prompt: "fix all the bugs".to_string(),
                max_iterations: None,
                budget: None,
            }
        }
    );
}

#[test]
fn slash_continue_start_parses_budget_overrides() {
    let cmd = SlashCommand::parse("/continue --max-iterations 4 --budget 1.5 refactor X")
        .unwrap()
        .unwrap();
    assert_eq!(
        cmd,
        SlashCommand::Continue {
            action: ContinueAction::Start {
                prompt: "refactor X".to_string(),
                max_iterations: Some(4),
                budget: Some(1.5),
            }
        }
    );
}

#[test]
fn slash_continue_start_supports_short_flags_in_any_order() {
    let cmd = SlashCommand::parse("/continue refactor Y -i 3 -b 2.0 extra")
        .unwrap()
        .unwrap();
    assert_eq!(
        cmd,
        SlashCommand::Continue {
            action: ContinueAction::Start {
                prompt: "refactor Y extra".to_string(),
                max_iterations: Some(3),
                budget: Some(2.0),
            }
        }
    );
}

#[test]
fn slash_continue_status_no_id() {
    let cmd = SlashCommand::parse("/continue status").unwrap().unwrap();
    assert_eq!(
        cmd,
        SlashCommand::Continue {
            action: ContinueAction::Status { chain_id: None }
        }
    );
}

#[test]
fn slash_continue_status_with_id() {
    let cmd = SlashCommand::parse("/continue status chain-7")
        .unwrap()
        .unwrap();
    assert_eq!(
        cmd,
        SlashCommand::Continue {
            action: ContinueAction::Status {
                chain_id: Some("chain-7".to_string()),
            }
        }
    );
}

#[test]
fn slash_continue_alias_continuation() {
    let cmd = SlashCommand::parse("/continuation do work")
        .unwrap()
        .unwrap();
    assert!(matches!(
        cmd,
        SlashCommand::Continue {
            action: ContinueAction::Start { .. }
        }
    ));
}

#[test]
fn slash_continue_missing_prompt_errors() {
    let result = SlashCommand::parse("/continue");
    assert!(matches!(result, Err(SlashError::MissingPrompt(_))));
}

#[test]
fn slash_continue_only_flags_errors() {
    let result = SlashCommand::parse("/continue --max-iterations 3");
    assert!(matches!(result, Err(SlashError::MissingPrompt(_))));
}

#[test]
fn slash_continue_bad_int_errors() {
    let result = SlashCommand::parse("/continue --max-iterations abc do work");
    assert!(matches!(result, Err(SlashError::Malformed(_))));
}

#[test]
fn slash_continue_missing_flag_value_errors() {
    let result = SlashCommand::parse("/continue --budget do work");
    assert!(matches!(result, Err(SlashError::Malformed(_))));
}

#[test]
fn slash_stop_continuation_one_id() {
    let cmd = SlashCommand::parse("/stop-continuation chain-1")
        .unwrap()
        .unwrap();
    assert_eq!(
        cmd,
        SlashCommand::StopContinuation {
            target: StopTarget::One("chain-1".to_string()),
        }
    );
}

#[test]
fn slash_stop_continuation_all_case_insensitive() {
    for alias in ["/stop-continuation all", "/stop-continuation ALL"] {
        let cmd = SlashCommand::parse(alias).unwrap().unwrap();
        assert_eq!(
            cmd,
            SlashCommand::StopContinuation {
                target: StopTarget::All,
            }
        );
    }
}

#[test]
fn slash_stop_continuation_alias() {
    let cmd = SlashCommand::parse("/stop-continue chain-9")
        .unwrap()
        .unwrap();
    assert_eq!(
        cmd,
        SlashCommand::StopContinuation {
            target: StopTarget::One("chain-9".to_string()),
        }
    );
}

#[test]
fn slash_stop_continuation_missing_target_errors() {
    let result = SlashCommand::parse("/stop-continuation");
    assert!(matches!(result, Err(SlashError::MissingChainId(_))));
}

#[test]
fn mock_start_continuation_returns_unique_chain_ids() {
    let mock = MockOrchestrator::new();
    let id1 = mock.start_continuation("do X", None, None);
    let id2 = mock.start_continuation("do Y", None, None);
    assert_ne!(id1, id2);
    assert!(id1.starts_with("chain-"));
    assert!(id2.starts_with("chain-"));
}

#[test]
fn mock_start_continuation_dispatches_root_task() {
    let mock = MockOrchestrator::new();
    let chain_id = mock.start_continuation("refactor auth", None, None);
    assert!(mock.is_chain_active(&chain_id));
    assert_eq!(mock.list_tasks().len(), 1);
    assert!(mock.task_ids()[0].starts_with("task-"));
}

#[test]
fn mock_stop_continuation_one_terminates_chain() {
    let mock = MockOrchestrator::new();
    let id = mock.start_continuation("work", None, None);
    assert!(mock.is_chain_active(&id));
    let stopped = mock.stop_continuation(&id).unwrap();
    assert_eq!(stopped, 1);
    assert!(!mock.is_chain_active(&id));
}

#[test]
fn mock_stop_continuation_unknown_errors() {
    let mock = MockOrchestrator::new();
    let err = mock.stop_continuation("chain-999").unwrap_err();
    assert!(err.contains("not found"));
}

#[test]
fn mock_stop_continuation_already_stopped_returns_zero() {
    let mock = MockOrchestrator::new();
    let id = mock.start_continuation("work", None, None);
    mock.stop_continuation(&id).unwrap();
    let second = mock.stop_continuation(&id).unwrap();
    assert_eq!(second, 0);
}

#[test]
fn mock_stop_continuation_all_terminates_every_active_chain() {
    let mock = MockOrchestrator::new();
    let id1 = mock.start_continuation("a", None, None);
    let id2 = mock.start_continuation("b", None, None);
    let id3 = mock.start_continuation("c", None, None);
    let stopped = mock.stop_continuation("all").unwrap();
    assert_eq!(stopped, 3);
    assert!(!mock.is_chain_active(&id1));
    assert!(!mock.is_chain_active(&id2));
    assert!(!mock.is_chain_active(&id3));
}

#[test]
fn mock_stop_continuation_all_with_no_chains_returns_zero() {
    let mock = MockOrchestrator::new();
    let stopped = mock.stop_continuation("all").unwrap();
    assert_eq!(stopped, 0);
}

#[test]
fn mock_continuation_status_empty_reports_no_chains() {
    let mock = MockOrchestrator::new();
    let report = mock.continuation_status(None);
    assert!(report.contains("No continuation chains"));
}

#[test]
fn mock_continuation_status_lists_all_chains() {
    let mock = MockOrchestrator::new();
    let id1 = mock.start_continuation("first task", None, None);
    let id2 = mock.start_continuation("second task", Some(5), Some(1.0));
    let report = mock.continuation_status(None);
    assert!(report.contains("2 active"));
    assert!(report.contains(&id1));
    assert!(report.contains(&id2));
    assert!(report.contains("first task"));
    assert!(report.contains("second task"));
}

#[test]
fn mock_continuation_status_single_chain() {
    let mock = MockOrchestrator::new();
    let id = mock.start_continuation("specific task", Some(7), Some(2.5));
    let report = mock.continuation_status(Some(&id));
    assert!(report.contains(&id));
    assert!(report.contains("specific task"));
    assert!(report.contains('7'));
    assert!(report.contains("2.50"));
}

#[test]
fn mock_continuation_status_unknown_chain_reports_missing() {
    let mock = MockOrchestrator::new();
    let report = mock.continuation_status(Some("chain-ghost"));
    assert!(report.contains("No chain named 'chain-ghost'"));
}

#[test]
fn repl_continue_start_prints_chain_id() {
    let mock = Arc::new(MockOrchestrator::new());
    let (repl, buffer) = make_repl(mock);
    repl.handle_line("/continue fix the bug");
    let text = buffer.joined();
    assert!(text.contains("continuation chain"));
    assert!(text.contains("started"));
    assert!(text.contains("fix the bug"));
}

#[test]
fn repl_continue_status_prints_report() {
    let mock = Arc::new(MockOrchestrator::new());
    let chain_id = mock.start_continuation("seed task", None, None);
    let (repl, buffer) = make_repl(mock);
    repl.handle_line("/continue status");
    let text = buffer.joined();
    assert!(text.contains(&chain_id));
}

#[test]
fn repl_stop_continuation_all_with_no_chains_reports_none() {
    let mock = Arc::new(MockOrchestrator::new());
    let (repl, buffer) = make_repl(mock);
    repl.handle_line("/stop-continuation all");
    let text = buffer.joined();
    assert!(text.contains("no active"));
}

#[test]
fn repl_stop_continuation_one_reports_stopped() {
    let mock = Arc::new(MockOrchestrator::new());
    let chain_id = mock.start_continuation("work", None, None);
    let (repl, buffer) = make_repl(mock);
    repl.handle_line(&format!("/stop-continuation {chain_id}"));
    let text = buffer.joined();
    assert!(text.contains("stopped"));
    assert!(text.contains(&chain_id));
}

#[test]
fn repl_help_includes_continue_and_stop_commands() {
    let mock = Arc::new(MockOrchestrator::new());
    let (repl, buffer) = make_repl(mock);
    repl.handle_line("/help");
    let text = buffer.joined();
    assert!(text.contains("/continue"));
    assert!(text.contains("/stop-continuation"));
}

// ---------------------------------------------------------------------------
// stream_foreground streaming regression tests
//
// These guard against the bug where a normal (non-dispatch) reply was buffered
// until the first newline and only flushed at Done — making short answers look
// non-streaming. We drive RealOrchestrator with a ScriptedProvider and assert
// on the StreamEvent sequence.
// ---------------------------------------------------------------------------

use std::time::Duration;

use opca_cli::RealOrchestrator;
use opca_cli::tui::app::StreamEvent;
use opca_core::di::{StdClock, StdFileSystem, StdProcess};
use opca_test_utils::ScriptedProvider;

fn real_orch_with_provider(provider: ScriptedProvider) -> Arc<RealOrchestrator> {
    let dir = tempfile::tempdir().expect("tempdir");
    Arc::new(RealOrchestrator::new(
        Arc::new(provider),
        dir.path().to_path_buf(),
        Arc::new(StdClock),
        Arc::new(StdFileSystem),
        Arc::new(StdProcess),
    ))
}

/// A normal multi-token reply must arrive as multiple Deltas (streaming), not
/// one blob at Done. This is the direct regression test for the buffering bug.
#[tokio::test]
async fn normal_reply_streams_token_by_token() {
    let provider = ScriptedProvider::new()
        .then_text("Hello")
        .then_text(", ")
        .then_text("world!")
        .then_done();
    let orch = real_orch_with_provider(provider);

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    orch.stream_foreground("hi", tx);

    let events = collect_stream_events(&mut rx).await;

    let deltas: Vec<String> = events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::Delta(s) => Some(s.clone()),
            _ => None,
        })
        .collect();

    // Key assertion: more than one Delta means it streamed, didn't blob.
    assert!(
        deltas.len() >= 2,
        "expected streaming (>=2 deltas), got {deltas:?}"
    );
    assert_eq!(deltas.concat(), "Hello, world!");
    assert!(matches!(events.last(), Some(StreamEvent::Done)));
}

/// A short single-token reply with no newline must still flush (it used to be
/// swallowed until Done because there was never a `\n` to trigger the check).
#[tokio::test]
async fn short_reply_without_newline_still_streams() {
    let provider = ScriptedProvider::new().then_text("ok").then_done();
    let orch = real_orch_with_provider(provider);

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    orch.stream_foreground("ping", tx);

    let events = collect_stream_events(&mut rx).await;

    // The "ok" must reach the user as a Delta (not be swallowed).
    let has_ok_delta = events
        .iter()
        .any(|e| matches!(e, StreamEvent::Delta(s) if s == "ok"));
    assert!(has_ok_delta, "short reply was not streamed: {events:?}");
}

/// A `dispatch_task` tool call must produce a Dispatch event with the prompt.
#[tokio::test]
async fn dispatch_task_tool_call_emits_dispatch_event() {
    let provider = ScriptedProvider::new()
        .then_tool_call(
            "dispatch_task",
            serde_json::json!({"prompt": "refactor the auth module"}),
        )
        .then_done();
    let orch = real_orch_with_provider(provider);

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    orch.stream_foreground("refactor auth", tx);

    let events = collect_stream_events(&mut rx).await;

    let dispatches: Vec<String> = events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::Dispatch(d) => Some(d.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        dispatches,
        vec!["refactor the auth module".to_string()],
        "expected exactly one Dispatch with the prompt from the tool call"
    );
}

/// A text-only reply (no tool calls) must NOT produce a Dispatch event.
#[tokio::test]
async fn text_only_reply_no_dispatch() {
    let provider = ScriptedProvider::new()
        .then_text("This is a direct answer.")
        .then_done();
    let orch = real_orch_with_provider(provider);

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    orch.stream_foreground("what does this do?", tx);

    let events = collect_stream_events(&mut rx).await;

    let has_dispatch = events.iter().any(|e| matches!(e, StreamEvent::Dispatch(_)));
    assert!(!has_dispatch, "text-only reply must not trigger Dispatch");
    assert!(matches!(events.last(), Some(StreamEvent::Done)));
}

/// Legacy `OPCA_DISPATCH:` text in model output must be treated as ordinary
/// text — NOT trigger a Dispatch. This proves the prefix-matching code path
/// is gone.
#[tokio::test]
async fn legacy_prefix_in_text_does_not_dispatch() {
    let provider = ScriptedProvider::new()
        .then_text("OPCA_DISPATCH: refactor the auth module\n")
        .then_done();
    let orch = real_orch_with_provider(provider);

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    orch.stream_foreground("refactor auth", tx);

    let events = collect_stream_events(&mut rx).await;

    let has_dispatch = events.iter().any(|e| matches!(e, StreamEvent::Dispatch(_)));
    assert!(
        !has_dispatch,
        "legacy OPCA_DISPATCH text must NOT trigger Dispatch: {events:?}"
    );

    // The text should be forwarded as regular Delta content.
    let all_text: String = events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::Delta(s) => Some(s.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        all_text.contains("OPCA_DISPATCH"),
        "legacy prefix text should be shown to user as ordinary text"
    );
    assert!(matches!(events.last(), Some(StreamEvent::Done)));
}

/// Drain `StreamEvent`s until a terminal event (Done/Dispatch/Error) arrives,
/// with a 2s timeout so a regression that never flushes fails fast.
async fn collect_stream_events(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<StreamEvent>,
) -> Vec<StreamEvent> {
    let mut out = Vec::new();
    while let Ok(Some(ev)) = tokio::time::timeout(Duration::from_secs(2), rx.recv()).await {
        let terminal = matches!(
            ev,
            StreamEvent::Done | StreamEvent::Dispatch(_) | StreamEvent::Error(_)
        );
        out.push(ev);
        if terminal {
            break;
        }
    }
    out
}

// ── G9: Clarification Protocol tests ──────────────────────────────────

#[test]
fn slash_answer_parses_task_id_and_choice() {
    let cmd = SlashCommand::parse("/answer task-0 use JWT")
        .unwrap()
        .unwrap();
    assert_eq!(
        cmd,
        SlashCommand::Answer {
            task_id: "task-0".to_string(),
            choice: "use JWT".to_string(),
        }
    );
}

#[test]
fn slash_answer_requires_task_id() {
    let result = SlashCommand::parse("/answer");
    assert!(matches!(result, Err(SlashError::MissingTaskId(_))));
}

#[test]
fn slash_answer_requires_choice() {
    let result = SlashCommand::parse("/answer task-0");
    assert!(matches!(result, Err(SlashError::MissingPrompt(_))));
}

#[test]
fn slash_answer_strips_quotes_around_choice() {
    let cmd = SlashCommand::parse("/answer task-1 \"option A\"")
        .unwrap()
        .unwrap();
    assert_eq!(
        cmd,
        SlashCommand::Answer {
            task_id: "task-1".to_string(),
            choice: "option A".to_string(),
        }
    );
}

#[test]
fn help_text_documents_answer_command() {
    assert!(opca_cli::commands::HELP_TEXT.contains("/answer"));
}

#[test]
fn mock_answer_task_requires_waiting_status() {
    let mock = MockOrchestrator::new();
    let id = mock.dispatch("work");
    let err = mock.answer_task(&id, "yes").unwrap_err();
    assert!(err.contains("Waiting"));
}

#[test]
fn mock_answer_task_succeeds_for_waiting_task() {
    let mock = MockOrchestrator::new();
    let id = mock.dispatch("work");
    mock.push_heartbeat(&id, TaskStatus::Waiting, 0.0, "need clarification");
    mock.answer_task(&id, "go with JWT")
        .expect("answer should succeed");
    let info = mock.task_status(&id).unwrap();
    assert_eq!(info.status, TaskStatus::OnIt);
    assert!(info.summary.contains("go with JWT"));
}

#[test]
fn repl_answer_command_prints_confirmation() {
    let mock = Arc::new(MockOrchestrator::new());
    let id = mock.dispatch("work");
    mock.push_heartbeat(&id, TaskStatus::Waiting, 0.0, "need info");
    let (repl, buffer) = make_repl(mock);
    repl.handle_line(&format!("/answer {id} use JWT"));
    let text = buffer.joined();
    assert!(text.contains("answered"));
    assert!(text.contains("use JWT"));
}

#[test]
fn repl_answer_on_non_waiting_errors() {
    let mock = Arc::new(MockOrchestrator::new());
    let id = mock.dispatch("work");
    let (repl, buffer) = make_repl(mock);
    repl.handle_line(&format!("/answer {id} yes"));
    let text = buffer.joined();
    assert!(text.contains("cannot answer"));
}

// ── G9: Context-Completion Gate tests ─────────────────────────────────

#[test]
fn dispatch_gate_allows_sufficient_request() {
    let result = opca_core::orchestrator::can_dispatch("implement JWT auth for the API", 0);
    assert!(result.is_ok());
}

#[test]
fn dispatch_gate_rejects_missing_verb() {
    let result = opca_core::orchestrator::can_dispatch("hello there, how are you?", 0);
    assert_eq!(
        result,
        Err(opca_core::orchestrator::DispatchRejection::NoImplementationVerb)
    );
}

#[test]
fn dispatch_gate_rejects_vague_scope() {
    let result = opca_core::orchestrator::can_dispatch("fix it", 0);
    assert_eq!(
        result,
        Err(opca_core::orchestrator::DispatchRejection::ScopeTooVague)
    );
}

#[test]
fn dispatch_gate_rejects_pending_specialist() {
    let result = opca_core::orchestrator::can_dispatch("implement the new feature properly", 1);
    assert_eq!(
        result,
        Err(opca_core::orchestrator::DispatchRejection::SpecialistPending)
    );
}

#[test]
fn dispatch_gate_rejects_ambiguous_add_tests() {
    let result = opca_core::orchestrator::can_dispatch("add tests", 0);
    assert!(result.is_err());
}

// ── G9: Clarification notification rendering ──────────────────────────

#[tokio::test]
async fn clarification_notification_renders_in_repl_loop() {
    let mock = Arc::new(MockOrchestrator::new());
    let id = mock.dispatch("work");
    let buffer = Arc::new(BufferOutput::new());
    let output: Arc<dyn Output> = buffer.clone();
    let repl = Arc::new(Repl::new(mock.clone(), output));

    let (input_tx, input_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let notif_rx = mock.subscribe();

    mock.push_heartbeat(
        &id,
        TaskStatus::Waiting,
        0.0,
        "Waiting for clarification: JWT or sessions?",
    );

    let repl_clone = repl.clone();
    let handle = tokio::spawn(async move {
        opca_cli::repl::run_main_loop_for_test(repl_clone, input_rx, notif_rx).await;
    });

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let text = buffer.joined();
    assert!(
        text.contains("waiting") || text.contains("Waiting"),
        "clarification notification should be rendered: got {text}"
    );
    assert!(
        text.contains("JWT or sessions?"),
        "question text should appear in output: got {text}"
    );
    assert!(
        text.contains(&id),
        "task id should appear in notification: got {text}"
    );

    input_tx.send(String::new()).ok();
    handle.abort();
}

// ── Sub-Agent feature tests (behind `sub-agents` feature flag) ──────────

#[cfg(feature = "sub-agents")]
mod sub_agents {
    use super::*;
    use opca_cli::SubTaskInfo;

    #[test]
    fn slash_subtasks_parses_no_arg() {
        let cmd = SlashCommand::parse("/subtasks").unwrap().unwrap();
        assert_eq!(
            cmd,
            SlashCommand::Subtasks {
                parent_task_id: None
            }
        );
    }

    #[test]
    fn slash_subtasks_parses_with_parent() {
        let cmd = SlashCommand::parse("/subtasks task-0").unwrap().unwrap();
        assert_eq!(
            cmd,
            SlashCommand::Subtasks {
                parent_task_id: Some("task-0".to_string()),
            }
        );
    }

    #[test]
    fn slash_subtasks_renders_empty() {
        let mock = Arc::new(MockOrchestrator::new());
        let (repl, buffer) = make_repl(mock);
        repl.handle_line("/subtasks");
        assert!(buffer.joined().contains("No sub-tasks."));
    }

    #[test]
    fn slash_subtasks_renders_seeded_subtasks() {
        let mock = Arc::new(MockOrchestrator::new());
        mock.seed_subtask("task-0", "sub-1", "find deprecated", TaskStatus::OnIt, 0.5);
        mock.seed_subtask("task-0", "sub-2", "write tests", TaskStatus::Pondering, 0.2);
        let (repl, buffer) = make_repl(mock);
        repl.handle_line("/subtasks task-0");
        let out = buffer.joined();
        assert!(out.contains("sub-1"), "should list sub-1: {out}");
        assert!(out.contains("sub-2"), "should list sub-2: {out}");
        assert!(
            out.contains("find deprecated"),
            "should show description: {out}"
        );
    }

    #[test]
    fn slash_subtasks_filters_by_parent() {
        let mock = Arc::new(MockOrchestrator::new());
        mock.seed_subtask("task-0", "sub-1", "work A", TaskStatus::OnIt, 0.1);
        mock.seed_subtask("task-1", "sub-2", "work B", TaskStatus::OnIt, 0.9);
        let (repl, buffer) = make_repl(mock);
        repl.handle_line("/subtasks task-0");
        let out = buffer.joined();
        assert!(
            out.contains("sub-1"),
            "should include task-0's subtask: {out}"
        );
        assert!(
            !out.contains("sub-2"),
            "should NOT include task-1's subtask: {out}"
        );
    }

    #[test]
    fn slash_subtasks_all_when_no_parent() {
        let mock = Arc::new(MockOrchestrator::new());
        mock.seed_subtask("task-0", "sub-1", "work A", TaskStatus::OnIt, 0.1);
        mock.seed_subtask("task-1", "sub-2", "work B", TaskStatus::OnIt, 0.9);
        let (repl, buffer) = make_repl(mock);
        repl.handle_line("/subtasks");
        let out = buffer.joined();
        assert!(out.contains("sub-1"), "should include all: {out}");
        assert!(out.contains("sub-2"), "should include all: {out}");
    }

    #[test]
    fn mock_list_subtasks_returns_subtask_info() {
        let mock = MockOrchestrator::new();
        mock.seed_subtask("task-0", "sub-1", "test work", TaskStatus::Delivered, 1.0);
        let subs: Vec<SubTaskInfo> = mock.list_subtasks(Some("task-0"));
        assert_eq!(subs.len(), 1);
        assert_eq!(subs[0].id, "sub-1");
        assert_eq!(subs[0].status, TaskStatus::Delivered);
        assert!((subs[0].progress - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn help_text_includes_subtasks() {
        use opca_cli::commands::HELP_TEXT;
        assert!(
            HELP_TEXT.contains("/subtasks"),
            "HELP_TEXT should list /subtasks"
        );
    }
}

#[cfg(not(feature = "sub-agents"))]
mod no_sub_agents {
    use super::*;

    #[test]
    fn slash_subtasks_not_recognized_without_feature() {
        let result = SlashCommand::parse("/subtasks");
        assert!(
            result.is_err(),
            "/subtasks should not be recognized without the feature"
        );
    }
}
