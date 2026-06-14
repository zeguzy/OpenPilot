//! Integration tests for the Hook extension point (Tasks 13.4–13.9).
//!
//! Covers the 5 handler types, the 4-level event dispatch, and the blocking
//! semantics of `PreToolUse` / `MergePre` denies.

use opca_core::extensions::{
    HookConfig, HookDispatcher, HookEvent, HookHandler, HookPayload, HookResult,
};
use serde_json::json;

// ---------------------------------------------------------------------------
// Task 13.7 — PreToolUse deny blocks the tool call
// ---------------------------------------------------------------------------

#[tokio::test]
async fn pre_tool_use_deny_blocks_tool_call() {
    // `printf` emits a deny response → the dispatcher must surface it.
    let dispatcher = HookDispatcher::new(vec![HookConfig {
        event: HookEvent::PreToolUse,
        matcher: Some("rm -rf".to_string()),
        handler: HookHandler::Command {
            command: "printf".to_string(),
            args: vec![r#"{"result":"deny","reason":"destructive command"}"#.to_string()],
        },
        timeout_ms: 5_000,
        can_block: true,
    }]);

    let payload = HookPayload::new(HookEvent::PreToolUse, json!({"command": "rm -rf /"}));

    let results = dispatcher.dispatch(HookEvent::PreToolUse, &payload).await;

    assert_eq!(results.len(), 1);
    assert!(HookDispatcher::any_deny(&results), "deny must block");
    match &results[0] {
        HookResult::Deny(reason) => assert!(reason.contains("destructive command")),
        other => panic!("expected Deny, got {other:?}"),
    }
}

#[tokio::test]
async fn pre_tool_use_allow_passes_through() {
    let dispatcher = HookDispatcher::new(vec![HookConfig {
        event: HookEvent::PreToolUse,
        matcher: None,
        handler: HookHandler::Command {
            command: "true".to_string(),
            args: vec![],
        },
        timeout_ms: 5_000,
        can_block: true,
    }]);

    let payload = HookPayload::new(HookEvent::PreToolUse, json!({"command": "ls -la"}));
    let results = dispatcher.dispatch(HookEvent::PreToolUse, &payload).await;

    assert_eq!(results.len(), 1);
    assert!(!HookDispatcher::any_deny(&results));
}

#[tokio::test]
async fn pre_tool_use_matcher_skips_safe_commands() {
    let dispatcher = HookDispatcher::new(vec![HookConfig {
        event: HookEvent::PreToolUse,
        matcher: Some("rm -rf".to_string()),
        handler: HookHandler::Command {
            command: "printf".to_string(),
            args: vec![r#"{"result":"deny"}"#.to_string()],
        },
        timeout_ms: 5_000,
        can_block: true,
    }]);

    // Safe command does not match the rm -rf matcher → hook does not fire.
    let payload = HookPayload::new(HookEvent::PreToolUse, json!({"command": "cargo build"}));
    let results = dispatcher.dispatch(HookEvent::PreToolUse, &payload).await;
    assert!(results.is_empty());
}

#[tokio::test]
async fn pre_tool_use_nonzero_exit_denies() {
    let dispatcher = HookDispatcher::new(vec![HookConfig {
        event: HookEvent::PreToolUse,
        matcher: None,
        handler: HookHandler::Command {
            command: "false".to_string(),
            args: vec![],
        },
        timeout_ms: 5_000,
        can_block: true,
    }]);

    let payload = HookPayload::new(HookEvent::PreToolUse, json!({}));
    let results = dispatcher.dispatch(HookEvent::PreToolUse, &payload).await;
    assert!(HookDispatcher::any_deny(&results));
}

// ---------------------------------------------------------------------------
// Task 13.8 — MergePre blocks on test failure
// ---------------------------------------------------------------------------

#[tokio::test]
async fn merge_pre_blocks_when_test_hook_fails() {
    // `false` stands in for a failing `cargo test` — nonzero exit means deny.
    let dispatcher = HookDispatcher::new(vec![HookConfig {
        event: HookEvent::MergePre,
        matcher: None,
        handler: HookHandler::Command {
            command: "false".to_string(),
            args: vec![],
        },
        timeout_ms: 5_000,
        can_block: true,
    }]);

    let payload = HookPayload::new(
        HookEvent::MergePre,
        json!({"task_id": "T-1", "diff_stats": {"added": 10, "removed": 2}}),
    );
    let results = dispatcher.dispatch(HookEvent::MergePre, &payload).await;
    assert_eq!(results.len(), 1);
    assert!(
        HookDispatcher::any_deny(&results),
        "MergePre must block when test hook fails"
    );
}

#[tokio::test]
async fn merge_pre_allows_when_test_hook_passes() {
    let dispatcher = HookDispatcher::new(vec![HookConfig {
        event: HookEvent::MergePre,
        matcher: None,
        handler: HookHandler::Command {
            command: "true".to_string(),
            args: vec![],
        },
        timeout_ms: 5_000,
        can_block: true,
    }]);

    let payload = HookPayload::new(HookEvent::MergePre, json!({}));
    let results = dispatcher.dispatch(HookEvent::MergePre, &payload).await;
    assert!(!HookDispatcher::any_deny(&results));
}

// ---------------------------------------------------------------------------
// Task 13.9 — Prompt-type hook (LLM single-turn judgment)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn prompt_type_hook_returns_continue_as_placeholder() {
    // Until a Provider is wired in, prompt hooks return Continue so the
    // dispatcher pipeline runs end-to-end. This test pins that contract.
    let dispatcher = HookDispatcher::new(vec![HookConfig {
        event: HookEvent::PreToolUse,
        matcher: None,
        handler: HookHandler::Prompt {
            template: "Is this command safe? {{command}}".to_string(),
        },
        timeout_ms: 5_000,
        can_block: false,
    }]);

    let payload = HookPayload::new(HookEvent::PreToolUse, json!({"command": "rm /tmp/x"}));
    let results = dispatcher.dispatch(HookEvent::PreToolUse, &payload).await;

    assert_eq!(results.len(), 1);
    assert_eq!(results[0], HookResult::Continue);
    // Continue is not a deny, so the surrounding operation proceeds.
    assert!(!HookDispatcher::any_deny(&results));
}

#[tokio::test]
async fn prompt_handler_executes_against_payload() {
    let handler = HookHandler::Prompt {
        template: "safe?".to_string(),
    };
    let payload = HookPayload::new(HookEvent::PostToolUse, json!({}));
    let result = handler.execute(&payload).await.unwrap();
    assert_eq!(result, HookResult::Continue);
}

// ---------------------------------------------------------------------------
// Handler variety — make sure all 5 variants at least execute.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn mcp_tool_handler_placeholder_returns_continue() {
    let handler = HookHandler::McpTool {
        server: "github".into(),
        tool: "check_safety".into(),
    };
    let payload = HookPayload::new(HookEvent::PreToolUse, json!({}));
    let result = handler.execute(&payload).await.unwrap();
    assert_eq!(result, HookResult::Continue);
}

#[tokio::test]
async fn agent_handler_placeholder_returns_continue() {
    let handler = HookHandler::Agent {
        instruction: "verify this change".to_string(),
    };
    let payload = HookPayload::new(HookEvent::AuditReport, json!({}));
    let result = handler.execute(&payload).await.unwrap();
    assert_eq!(result, HookResult::Continue);
}

// ---------------------------------------------------------------------------
// Four-level dispatch — sanity-check that events from each level route.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn session_level_event_dispatches() {
    let dispatcher = HookDispatcher::new(vec![HookConfig {
        event: HookEvent::SessionStart,
        matcher: None,
        handler: HookHandler::Prompt {
            template: "session init".to_string(),
        },
        timeout_ms: 1_000,
        can_block: false,
    }]);
    let payload = HookPayload::new(HookEvent::SessionStart, json!({}));
    let results = dispatcher.dispatch(HookEvent::SessionStart, &payload).await;
    assert_eq!(results.len(), 1);
}

#[tokio::test]
async fn orchestrator_level_event_dispatches() {
    let dispatcher = HookDispatcher::new(vec![HookConfig {
        event: HookEvent::UserMessage,
        matcher: None,
        handler: HookHandler::Prompt {
            template: "user message".to_string(),
        },
        timeout_ms: 1_000,
        can_block: false,
    }]);
    let payload = HookPayload::new(HookEvent::UserMessage, json!({"text": "hi"}));
    let results = dispatcher.dispatch(HookEvent::UserMessage, &payload).await;
    assert_eq!(results.len(), 1);
}

#[tokio::test]
async fn audit_level_event_dispatches() {
    let dispatcher = HookDispatcher::new(vec![HookConfig {
        event: HookEvent::AuditStart,
        matcher: None,
        handler: HookHandler::Prompt {
            template: "audit".to_string(),
        },
        timeout_ms: 1_000,
        can_block: false,
    }]);
    let payload = HookPayload::new(HookEvent::AuditStart, json!({}));
    let results = dispatcher.dispatch(HookEvent::AuditStart, &payload).await;
    assert_eq!(results.len(), 1);
}

#[tokio::test]
async fn post_tool_use_deny_does_not_block_by_contract() {
    // PostToolUse does not honor deny — the operation already happened.
    // The dispatcher still records the result; callers must check honors_deny.
    let dispatcher = HookDispatcher::new(vec![HookConfig {
        event: HookEvent::PostToolUse,
        matcher: None,
        handler: HookHandler::Command {
            command: "printf".to_string(),
            args: vec![r#"{"result":"deny","reason":"too late"}"#.to_string()],
        },
        timeout_ms: 5_000,
        can_block: true,
    }]);
    let payload = HookPayload::new(HookEvent::PostToolUse, json!({}));
    let results = dispatcher.dispatch(HookEvent::PostToolUse, &payload).await;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], HookResult::Deny("too late".into()));
    // Contract: PostToolUse.honors_deny() is false even though result is deny.
    assert!(!HookEvent::PostToolUse.honors_deny());
}

#[tokio::test]
async fn hook_timeout_converts_to_deny_when_can_block() {
    // `sleep 5` exceeds the 100ms timeout; can_block=true → Deny.
    let dispatcher = HookDispatcher::new(vec![HookConfig {
        event: HookEvent::PreToolUse,
        matcher: None,
        handler: HookHandler::Command {
            command: "sleep".to_string(),
            args: vec!["5".to_string()],
        },
        timeout_ms: 100,
        can_block: true,
    }]);
    let payload = HookPayload::new(HookEvent::PreToolUse, json!({}));
    let results = dispatcher.dispatch(HookEvent::PreToolUse, &payload).await;
    assert_eq!(results.len(), 1);
    match &results[0] {
        HookResult::Deny(reason) => assert!(reason.contains("timeout")),
        other => panic!("expected Deny on timeout, got {other:?}"),
    }
}

#[tokio::test]
async fn hook_timeout_returns_continue_when_cannot_block() {
    let dispatcher = HookDispatcher::new(vec![HookConfig {
        event: HookEvent::PostToolUse,
        matcher: None,
        handler: HookHandler::Command {
            command: "sleep".to_string(),
            args: vec!["5".to_string()],
        },
        timeout_ms: 100,
        can_block: false,
    }]);
    let payload = HookPayload::new(HookEvent::PostToolUse, json!({}));
    let results = dispatcher.dispatch(HookEvent::PostToolUse, &payload).await;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], HookResult::Continue);
}

#[tokio::test]
async fn dispatcher_register_adds_hook_at_runtime() {
    let mut dispatcher = HookDispatcher::empty();
    dispatcher.register(HookConfig::new(
        HookEvent::TaskCreate,
        HookHandler::Prompt {
            template: "x".to_string(),
        },
    ));
    assert_eq!(dispatcher.len(), 1);

    let payload = HookPayload::new(HookEvent::TaskCreate, json!({}));
    let results = dispatcher.dispatch(HookEvent::TaskCreate, &payload).await;
    assert_eq!(results.len(), 1);
}
