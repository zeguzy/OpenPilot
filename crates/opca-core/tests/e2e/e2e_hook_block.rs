//! Task 17.9 — E2E: hook blocks dangerous bash command (`on_pre_tool_use` deny).
//!
//! Creates a `HookDispatcher` with a `PreToolUse` hook whose matcher is
//! `rm -rf` and whose handler is a command that emits a JSON deny response.
//! Dispatches the event with a dangerous payload and verifies the Deny result.

use opca_core::extensions::{
    HookConfig, HookDispatcher, HookEvent, HookHandler, HookPayload, HookResult,
};
use serde_json::json;

#[tokio::test]
#[ignore = "E2E: hook blocks dangerous command"]
async fn e2e_hook_blocks_rm_rf() {
    let deny_hook = HookConfig {
        event: HookEvent::PreToolUse,
        matcher: Some("rm -rf".to_string()),
        handler: HookHandler::Command {
            command: "printf".to_string(),
            args: vec![
                r#"{"result":"deny","reason":"rm -rf is blocked by safety hook"}"#.to_string(),
            ],
        },
        timeout_ms: 5000,
        can_block: true,
    };

    let dispatcher = HookDispatcher::new(vec![deny_hook]);

    let dangerous_payload = HookPayload::new(
        HookEvent::PreToolUse,
        json!({
            "tool": "bash",
            "command": "rm -rf /important/dir"
        }),
    );

    let results = dispatcher
        .dispatch(HookEvent::PreToolUse, &dangerous_payload)
        .await;

    assert_eq!(results.len(), 1, "hook should fire for rm -rf payload");

    match &results[0] {
        HookResult::Deny(reason) => {
            assert!(
                reason.contains("blocked"),
                "deny reason should mention blocked, got: {reason}"
            );
        }
        other => panic!("expected Deny, got {other:?}"),
    }

    assert!(
        HookDispatcher::any_deny(&results),
        "any_deny should return true"
    );
}

#[tokio::test]
#[ignore = "E2E: hook allows safe command"]
async fn e2e_hook_allows_safe_command() {
    let deny_hook = HookConfig {
        event: HookEvent::PreToolUse,
        matcher: Some("rm -rf".to_string()),
        handler: HookHandler::Command {
            command: "printf".to_string(),
            args: vec![r#"{"result":"deny","reason":"blocked"}"#.to_string()],
        },
        timeout_ms: 5000,
        can_block: true,
    };

    let dispatcher = HookDispatcher::new(vec![deny_hook]);

    let safe_payload = HookPayload::new(
        HookEvent::PreToolUse,
        json!({
            "tool": "bash",
            "command": "ls -la /tmp"
        }),
    );

    let results = dispatcher
        .dispatch(HookEvent::PreToolUse, &safe_payload)
        .await;

    assert!(
        results.is_empty(),
        "hook should NOT fire for safe command (matcher mismatch)"
    );
    assert!(!HookDispatcher::any_deny(&results));
}
