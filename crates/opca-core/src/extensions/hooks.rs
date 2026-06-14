//! Hook system — lifecycle interception across four levels.
//!
//! See `design.md` §D10 for the "Hook (生命周期拦截)" rationale and
//! `specs/extension-system/spec.md` for the requirement contracts.
//!
//! # Four event levels
//!
//! The hook system covers the four lifecycle levels of the agent stack:
//! - **Session** — [`HookEvent::SessionStart`], [`HookEvent::SessionEnd`]
//! - **Orchestrator** — [`HookEvent::UserMessage`], [`HookEvent::PreDispatch`],
//!   [`HookEvent::PostDispatch`], [`HookEvent::TaskHighlight`],
//!   [`HookEvent::Recall`], [`HookEvent::MergePre`], [`HookEvent::MergePost`]
//! - **Task** — [`HookEvent::TaskCreate`], [`HookEvent::TaskFreeze`],
//!   [`HookEvent::TaskReject`], [`HookEvent::TaskArchive`],
//!   [`HookEvent::PreToolUse`], [`HookEvent::PostToolUse`]
//! - **Audit** — [`HookEvent::AuditStart`], [`HookEvent::AuditReport`],
//!   [`HookEvent::AuditOverride`]
//!
//! # Five handler types
//!
//! Each hook config picks one of five handler types: `command`, `http`,
//! `mcp_tool`, `prompt`, `agent`. The `command` and `http` handlers are fully
//! implemented for MVP; `mcp_tool`, `prompt`, and `agent` are placeholders
//! that log and return [`HookResult::Continue`] until their dependencies
//! (MCP client / Provider / subagent spawner) are wired in.
//!
//! # Blocking
//!
//! [`HookConfig::can_block`] determines whether a hook's [`HookResult::Deny`]
//! aborts the surrounding operation. Only `PreToolUse` and `MergePre` events
//! honor deny results today.

use std::time::Duration;

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::AsyncWriteExt;
use tokio::time::timeout;

/// All lifecycle events a hook can subscribe to.
///
/// Variants are grouped into the four levels documented at the module top.
/// The string form (used in `hooks.toml`) is the kebab-case event name
/// (`on_pre_tool_use`, `on_merge_pre`, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEvent {
    SessionStart,
    SessionEnd,
    UserMessage,
    PreDispatch,
    PostDispatch,
    TaskHighlight,
    Recall,
    MergePre,
    MergePost,
    TaskCreate,
    TaskFreeze,
    TaskReject,
    TaskArchive,
    PreToolUse,
    PostToolUse,
    AuditStart,
    AuditReport,
    AuditOverride,
}

impl HookEvent {
    /// Whether a [`HookResult::Deny`] from a hook on this event should
    /// actually block the surrounding operation.
    ///
    /// Only `PreToolUse` and `MergePre` honor deny results today — every
    /// other level treats deny as a logged warning.
    #[must_use]
    pub const fn honors_deny(self) -> bool {
        matches!(self, Self::PreToolUse | Self::MergePre)
    }

    /// Convert to the kebab-case `on_*` event name used in `hooks.toml`.
    #[must_use]
    pub const fn as_config_key(self) -> &'static str {
        match self {
            Self::SessionStart => "on_session_start",
            Self::SessionEnd => "on_session_end",
            Self::UserMessage => "on_user_message",
            Self::PreDispatch => "on_pre_dispatch",
            Self::PostDispatch => "on_post_dispatch",
            Self::TaskHighlight => "on_task_highlight",
            Self::Recall => "on_recall",
            Self::MergePre => "on_merge_pre",
            Self::MergePost => "on_merge_post",
            Self::TaskCreate => "on_task_create",
            Self::TaskFreeze => "on_task_freeze",
            Self::TaskReject => "on_task_reject",
            Self::TaskArchive => "on_task_archive",
            Self::PreToolUse => "on_pre_tool_use",
            Self::PostToolUse => "on_post_tool_use",
            Self::AuditStart => "on_audit_start",
            Self::AuditReport => "on_audit_report",
            Self::AuditOverride => "on_audit_override",
        }
    }
}

/// One of the five handler types a hook can dispatch to.
///
/// `Command` and `Http` are fully implemented; the other three are
/// placeholders pending their downstream dependencies.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HookHandler {
    /// Spawn a subprocess; write payload JSON to stdin, parse stdout JSON.
    Command {
        command: String,
        #[serde(default)]
        args: Vec<String>,
    },
    /// POST the payload as JSON to a URL, parse the JSON response.
    Http { url: String },
    /// Call an MCP tool. Placeholder — logs and returns [`HookResult::Continue`].
    McpTool { server: String, tool: String },
    /// Single-turn LLM call. Placeholder — needs a Provider reference to run.
    Prompt { template: String },
    /// Spawn a subagent to verify. Placeholder.
    Agent { instruction: String },
}

impl HookHandler {
    /// Execute this handler against `payload`, returning a [`HookResult`].
    ///
    /// `Command` and `Http` perform real I/O. The placeholder handlers log
    /// via `tracing::debug!` and return [`HookResult::Continue`] so the
    /// dispatcher can run end-to-end before their dependencies are wired in.
    ///
    /// Errors from the underlying transport are converted into
    /// [`HookResult::Deny`] with a diagnostic message rather than propagated,
    /// so one flaky hook does not crash a dispatch fan-out.
    pub async fn execute(&self, payload: &HookPayload) -> Result<HookResult> {
        match self {
            Self::Command { command, args } => Self::execute_command(command, args, payload).await,
            Self::Http { url } => Self::execute_http(url, payload).await,
            Self::McpTool { server, tool } => {
                tracing::debug!(
                    server = %server,
                    tool = %tool,
                    "mcp_tool hook handler is a placeholder; returning Continue"
                );
                Ok(HookResult::Continue)
            }
            Self::Prompt { template } => {
                tracing::debug!(
                    template = %template,
                    "prompt hook handler is a placeholder; returning Continue"
                );
                Ok(HookResult::Continue)
            }
            Self::Agent { instruction } => {
                tracing::debug!(
                    instruction = %instruction,
                    "agent hook handler is a placeholder; returning Continue"
                );
                Ok(HookResult::Continue)
            }
        }
    }

    async fn execute_command(
        command: &str,
        hook_args: &[String],
        payload: &HookPayload,
    ) -> Result<HookResult> {
        let cmd_args: Vec<&str> = hook_args.iter().map(String::as_str).collect();
        let mut cmd = tokio::process::Command::new(command);
        if !cmd_args.is_empty() {
            cmd.args(&cmd_args);
        }
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let mut child = cmd
            .spawn()
            .with_context(|| format!("failed to spawn hook command `{command}`"))?;

        if let Some(mut stdin) = child.stdin.take() {
            let payload_bytes = serde_json::to_vec(&payload.data).unwrap_or_default();
            let _ = stdin.write_all(&payload_bytes).await;
        }

        let output = child
            .wait_with_output()
            .await
            .context("hook command wait failed")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Ok(HookResult::Deny(format!(
                "hook command `{command}` exited {} stderr: {stderr}",
                output.status
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stdout_trimmed = stdout.trim();
        if stdout_trimmed.is_empty() {
            return Ok(HookResult::Allow);
        }
        let parsed: Value = serde_json::from_str(stdout_trimmed).with_context(|| {
            format!("hook command `{command}` returned non-JSON stdout: {stdout_trimmed}")
        })?;
        Ok(parse_hook_value(&parsed))
    }

    async fn execute_http(url: &str, payload: &HookPayload) -> Result<HookResult> {
        let client = reqwest::Client::new();
        let resp = client
            .post(url)
            .json(&payload.data)
            .send()
            .await
            .with_context(|| format!("hook http POST to `{url}` failed"))?;
        let status = resp.status();
        let body: Value = resp
            .json()
            .await
            .with_context(|| format!("hook http response from `{url}` is not JSON"))?;
        if !status.is_success() {
            return Ok(HookResult::Deny(format!(
                "hook http {url} returned status {status}: {body}"
            )));
        }
        Ok(parse_hook_value(&body))
    }
}

/// Convert a parsed JSON response into a [`HookResult`].
///
/// Recognized shapes (case-insensitive on string values):
/// - `{"result": "allow"}` → [`HookResult::Allow`]
/// - `{"result": "deny", "reason": "..."}` → [`HookResult::Deny`]
/// - `{"result": "modify", "data": {...}}` → [`HookResult::Modify`]
/// - any other shape → [`HookResult::Allow`] (default-safe)
fn parse_hook_value(value: &Value) -> HookResult {
    let Some(obj) = value.as_object() else {
        return HookResult::Allow;
    };
    let kind = obj
        .get("result")
        .and_then(|v| v.as_str())
        .unwrap_or("allow")
        .to_ascii_lowercase();
    match kind.as_str() {
        "deny" => {
            let reason = obj
                .get("reason")
                .and_then(|v| v.as_str())
                .unwrap_or("hook denied")
                .to_string();
            HookResult::Deny(reason)
        }
        "modify" => HookResult::Modify(obj.get("data").cloned().unwrap_or(Value::Null)),
        "continue" => HookResult::Continue,
        _ => HookResult::Allow,
    }
}

/// The outcome of running a single hook.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookResult {
    /// Operation may proceed.
    Allow,
    /// Operation must be blocked. The carried reason is surfaced to the agent.
    Deny(String),
    /// Replace part of the in-flight payload with `data`.
    Modify(Value),
    /// Neither allow nor deny — the hook abstained.
    Continue,
}

impl HookResult {
    /// `true` if this result is [`HookResult::Deny`].
    #[must_use]
    pub const fn is_deny(&self) -> bool {
        matches!(self, Self::Deny(..))
    }
}

/// A single hook configuration entry (one row in `hooks.toml`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookConfig {
    pub event: HookEvent,
    /// Optional matcher on payload content. If present, the hook only fires
    /// when `matcher` appears as a substring of the JSON-encoded payload.
    /// This is the cheap pre-filter that keeps hot paths from spawning a
    /// subprocess on every event.
    #[serde(default)]
    pub matcher: Option<String>,
    pub handler: HookHandler,
    /// Per-hook timeout in milliseconds. Defaults to 10s per `design.md`.
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
    /// Whether a `Deny` from this hook should actually block the surrounding
    /// operation. Ignored for events whose [`HookEvent::honors_deny`] is false.
    #[serde(default = "default_can_block")]
    pub can_block: bool,
}

const fn default_timeout_ms() -> u64 {
    10_000
}

const fn default_can_block() -> bool {
    true
}

impl HookConfig {
    /// Construct a hook with sane defaults (10s timeout, blocking).
    #[must_use]
    pub const fn new(event: HookEvent, handler: HookHandler) -> Self {
        Self {
            event,
            matcher: None,
            handler,
            timeout_ms: default_timeout_ms(),
            can_block: default_can_block(),
        }
    }

    /// `true` if `payload` matches this hook's matcher (or if no matcher).
    fn matches(&self, payload: &HookPayload) -> bool {
        let Some(needle) = &self.matcher else {
            return true;
        };
        let hay = payload.data.to_string();
        hay.contains(needle.as_str())
    }
}

/// The payload delivered to every handler invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookPayload {
    pub event: HookEvent,
    pub data: Value,
}

impl HookPayload {
    #[must_use]
    pub const fn new(event: HookEvent, data: Value) -> Self {
        Self { event, data }
    }
}

/// Dispatches hooks across all configured events.
///
/// Hooks are stored in registration order. [`HookDispatcher::dispatch`]
/// returns results in the same order, so callers can attribute a deny back
/// to a specific hook by index.
pub struct HookDispatcher {
    hooks: Vec<HookConfig>,
}

impl HookDispatcher {
    #[must_use]
    pub const fn new(hooks: Vec<HookConfig>) -> Self {
        Self { hooks }
    }

    /// Build an empty dispatcher.
    #[must_use]
    pub const fn empty() -> Self {
        Self { hooks: Vec::new() }
    }

    /// Register an additional hook at runtime.
    pub fn register(&mut self, hook: HookConfig) {
        self.hooks.push(hook);
    }

    /// The configured hook count.
    #[must_use]
    pub fn len(&self) -> usize {
        self.hooks.len()
    }

    /// Whether the dispatcher has zero hooks.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.hooks.is_empty()
    }

    /// Dispatch an event to every matching hook, returning one result per
    /// firing hook (in registration order).
    ///
    /// Hooks whose [`HookConfig::matcher`] does not match the payload are
    /// silently skipped — they contribute no entry to the returned slice.
    /// Hooks that time out are converted to `Deny("hook timeout")` only if
    /// `can_block` is true; otherwise they become `Continue`.
    pub async fn dispatch(&self, event: HookEvent, payload: &HookPayload) -> Vec<HookResult> {
        let mut out = Vec::new();
        for hook in &self.hooks {
            if hook.event != event || !hook.matches(payload) {
                continue;
            }
            let result = match timeout(
                Duration::from_millis(hook.timeout_ms),
                hook.handler.execute(payload),
            )
            .await
            {
                Ok(Ok(r)) => r,
                Ok(Err(e)) => {
                    tracing::warn!(error = %e, event = ?hook.event, "hook handler errored");
                    if hook.can_block {
                        HookResult::Deny(format!("hook error: {e}"))
                    } else {
                        HookResult::Continue
                    }
                }
                Err(_) => {
                    tracing::warn!(
                        event = ?hook.event,
                        timeout_ms = hook.timeout_ms,
                        "hook handler timed out"
                    );
                    if hook.can_block {
                        HookResult::Deny("hook timeout".into())
                    } else {
                        HookResult::Continue
                    }
                }
            };
            out.push(result);
        }
        out
    }

    /// `true` if any of `results` is a [`HookResult::Deny`].
    ///
    /// Convenience for callers deciding whether to abort an operation.
    #[must_use]
    pub fn any_deny(results: &[HookResult]) -> bool {
        results.iter().any(HookResult::is_deny)
    }
}

impl Default for HookDispatcher {
    fn default() -> Self {
        Self::empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn honors_deny_only_for_pre_tool_use_and_merge_pre() {
        assert!(HookEvent::PreToolUse.honors_deny());
        assert!(HookEvent::MergePre.honors_deny());
        assert!(!HookEvent::SessionStart.honors_deny());
        assert!(!HookEvent::PostToolUse.honors_deny());
    }

    #[test]
    fn parse_hook_value_recognizes_deny() {
        let v = serde_json::json!({"result": "deny", "reason": "unsafe"});
        assert_eq!(parse_hook_value(&v), HookResult::Deny("unsafe".into()));
    }

    #[test]
    fn parse_hook_value_recognizes_allow_by_default() {
        let v = serde_json::json!({"hello": "world"});
        assert_eq!(parse_hook_value(&v), HookResult::Allow);
    }

    #[test]
    fn parse_hook_value_recognizes_modify() {
        let v = serde_json::json!({"result": "modify", "data": {"x": 1}});
        assert_eq!(
            parse_hook_value(&v),
            HookResult::Modify(serde_json::json!({"x": 1}))
        );
    }

    #[test]
    fn matcher_substring_filters_payload() {
        let cfg = HookConfig {
            event: HookEvent::PreToolUse,
            matcher: Some("rm -rf".to_string()),
            handler: HookHandler::Prompt {
                template: "is this safe?".to_string(),
            },
            timeout_ms: 1000,
            can_block: true,
        };
        let dangerous = HookPayload::new(
            HookEvent::PreToolUse,
            serde_json::json!({"command": "rm -rf /"}),
        );
        let safe = HookPayload::new(HookEvent::PreToolUse, serde_json::json!({"command": "ls"}));
        assert!(cfg.matches(&dangerous));
        assert!(!cfg.matches(&safe));
    }

    #[tokio::test]
    async fn placeholder_handlers_return_continue() {
        let payload = HookPayload::new(HookEvent::PreToolUse, Value::Null);
        let prompt = HookHandler::Prompt {
            template: "is this safe?".to_string(),
        };
        assert_eq!(
            prompt.execute(&payload).await.unwrap(),
            HookResult::Continue
        );

        let mcp = HookHandler::McpTool {
            server: "x".into(),
            tool: "y".into(),
        };
        assert_eq!(mcp.execute(&payload).await.unwrap(), HookResult::Continue);
    }

    #[tokio::test]
    async fn command_handler_allow_on_zero_exit_with_empty_stdout() {
        // `true` exits 0 with no stdout → Allow.
        let handler = HookHandler::Command {
            command: "true".to_string(),
            args: vec![],
        };
        let payload = HookPayload::new(HookEvent::PreToolUse, Value::Null);
        let result = handler.execute(&payload).await.unwrap();
        assert_eq!(result, HookResult::Allow);
    }

    #[tokio::test]
    async fn command_handler_deny_on_nonzero_exit() {
        let handler = HookHandler::Command {
            command: "false".to_string(),
            args: vec![],
        };
        let payload = HookPayload::new(HookEvent::PreToolUse, Value::Null);
        let result = handler.execute(&payload).await.unwrap();
        assert!(result.is_deny());
    }

    #[tokio::test]
    async fn command_handler_parses_json_stdout() {
        // `printf` is universally available; print a JSON deny response.
        let handler = HookHandler::Command {
            command: "printf".to_string(),
            args: vec!["{\"result\":\"deny\",\"reason\":\"blocked\"}".to_string()],
        };
        let payload = HookPayload::new(HookEvent::PreToolUse, Value::Null);
        let result = handler.execute(&payload).await.unwrap();
        assert_eq!(result, HookResult::Deny("blocked".into()));
    }

    #[tokio::test]
    async fn dispatcher_no_hooks_returns_empty() {
        let d = HookDispatcher::empty();
        let payload = HookPayload::new(HookEvent::PreToolUse, Value::Null);
        let results = d.dispatch(HookEvent::PreToolUse, &payload).await;
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn dispatcher_collects_results_in_order() {
        let dispatcher = HookDispatcher::new(vec![
            HookConfig::new(
                HookEvent::PreToolUse,
                HookHandler::Command {
                    command: "true".to_string(),
                    args: vec![],
                },
            ),
            HookConfig::new(
                HookEvent::PreToolUse,
                HookHandler::Prompt {
                    template: "x".to_string(),
                },
            ),
        ]);
        let payload = HookPayload::new(HookEvent::PreToolUse, Value::Null);
        let results = dispatcher.dispatch(HookEvent::PreToolUse, &payload).await;
        assert_eq!(results.len(), 2);
        assert_eq!(results[0], HookResult::Allow);
        assert_eq!(results[1], HookResult::Continue);
    }

    #[tokio::test]
    async fn dispatcher_skips_non_matching_event() {
        let dispatcher = HookDispatcher::new(vec![HookConfig::new(
            HookEvent::SessionStart,
            HookHandler::Prompt {
                template: "x".to_string(),
            },
        )]);
        let payload = HookPayload::new(HookEvent::PreToolUse, Value::Null);
        let results = dispatcher.dispatch(HookEvent::PreToolUse, &payload).await;
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn dispatcher_any_deny_helper() {
        let results = vec![HookResult::Allow, HookResult::Deny("x".into())];
        assert!(HookDispatcher::any_deny(&results));
        let results = vec![HookResult::Allow];
        assert!(!HookDispatcher::any_deny(&results));
    }
}
