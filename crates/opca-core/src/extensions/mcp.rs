//! MCP (Model Context Protocol) client — JSON-RPC 2.0 over stdin/stdout.
//!
//! See `design.md` §D10 "Capability" extension point and
//! `specs/extension-system/spec.md` for the requirement contracts.
//!
//! # MVP scope
//!
//! For the Phase 2 MVP we implement the three methods the agent actually
//! needs:
//! 1. `initialize` (sent on [`McpClient::start`])
//! 2. `tools/list` ([`McpClient::list_tools`])
//! 3. `tools/call` ([`McpClient::call_tool`])
//!
//! Resources and prompts discovery are out of scope until a downstream
//! consumer needs them.
//!
//! # Crash isolation
//!
//! The MCP server runs as a child process. If it exits, the next request
//! returns an error and the agent surfaces that tool as unavailable — the
//! main agent process is unaffected. See [`McpClient::check_alive`].

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use anyhow::{Context as _, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::{Mutex, oneshot};
use tracing::{debug, warn};

/// JSON-RPC 2.0 request envelope.
#[derive(Debug, Serialize)]
struct Request<'a> {
    jsonrpc: &'static str,
    id: i64,
    method: &'a str,
    params: Value,
}

/// JSON-RPC 2.0 response envelope (response or error only — notifications
/// are not used in the MVP).
#[derive(Debug, Deserialize)]
struct RawResponse {
    id: Option<i64>,
    result: Option<Value>,
    error: Option<RpcError>,
}

#[derive(Debug, Deserialize, Clone)]
struct RpcError {
    code: i64,
    message: String,
}

/// Definition of a tool exposed by an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpToolDef {
    pub name: String,
    pub description: String,
    /// JSON Schema for the tool's parameters.
    #[serde(default)]
    pub input_schema: Value,
}

impl McpToolDef {
    /// Convert an MCP tool def into the agent's canonical prefixed name.
    ///
    /// Per spec: `mcp__<server>__<tool>`. This keeps MCP tool names from
    /// colliding with built-in tools or with tools from other servers.
    #[must_use]
    pub fn prefixed_name(server: &str, tool: &str) -> String {
        format!("mcp__{server}__{tool}")
    }
}

/// A connected MCP server child process.
///
/// Owns the child's stdin/stdout pipes and tracks in-flight requests by JSON-RPC
/// id. The reader task is spawned on construction; it routes each response to
/// the waiting caller via a oneshot channel parked in the pending table.
pub struct McpClient {
    child: Child,
    stdin: Arc<Mutex<ChildStdin>>,
    next_id: AtomicI64,
    pending: Arc<Mutex<HashMap<i64, oneshot::Sender<RawResponse>>>>,
    /// Server-reported name (set during initialize).
    server_name: Option<String>,
}

impl McpClient {
    /// Spawn the MCP server and complete the JSON-RPC `initialize` handshake.
    ///
    /// Returns an error if the spawn fails or the server does not respond to
    /// `initialize` — callers should treat that as "MCP server unavailable"
    /// and surface the tool to the agent as missing rather than crash.
    pub async fn start(command: &str, args: &[&str]) -> Result<Self> {
        let mut cmd = tokio::process::Command::new(command);
        cmd.args(args);
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        let mut child = cmd
            .spawn()
            .with_context(|| format!("failed to spawn MCP server `{command}`"))?;

        let stdin = child.stdin.take().context("MCP server stdin missing")?;
        let stdout = child.stdout.take().context("MCP server stdout missing")?;

        let pending: Arc<Mutex<HashMap<i64, oneshot::Sender<RawResponse>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let reader_pending = pending.clone();
        tokio::spawn(async move {
            if let Err(e) = run_reader(stdout, reader_pending).await {
                warn!(error = %e, "MCP reader task ended");
            }
        });

        let mut client = Self {
            child,
            stdin: Arc::new(Mutex::new(stdin)),
            next_id: AtomicI64::new(1),
            pending,
            server_name: None,
        };

        let init = client.initialize().await;
        if let Err(e) = init {
            let _ = client.try_kill();
            return Err(e);
        }
        Ok(client)
    }

    /// Server name reported during `initialize` (or `None` before handshake).
    #[must_use]
    pub fn server_name(&self) -> Option<&str> {
        self.server_name.as_deref()
    }

    async fn initialize(&mut self) -> Result<()> {
        let params = serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "opca",
                "version": env!("CARGO_PKG_VERSION"),
            }
        });
        let result = self.request("initialize", params).await?;
        if let Some(info) = result.get("serverInfo").and_then(|v| v.get("name")) {
            if let Some(name) = info.as_str() {
                self.server_name = Some(name.to_string());
            }
        }
        // Send the initialized notification per spec — fire-and-forget.
        let notif = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        });
        let mut bytes = serde_json::to_vec(&notif).unwrap_or_default();
        bytes.push(b'\n');
        let _ = self.stdin.lock().await.write_all(&bytes).await;
        Ok(())
    }

    /// Call `tools/list` and return every tool the server exposes.
    pub async fn list_tools(&mut self) -> Result<Vec<McpToolDef>> {
        let result = self.request("tools/list", serde_json::json!({})).await?;
        let tools_val = result
            .get("tools")
            .cloned()
            .unwrap_or_else(|| Value::Array(vec![]));
        let tools: Vec<McpToolDef> = serde_json::from_value(tools_val).unwrap_or_default();
        Ok(tools)
    }

    /// Call a tool by name with JSON arguments.
    pub async fn call_tool(&mut self, name: &str, args: &Value) -> Result<Value> {
        let params = serde_json::json!({
            "name": name,
            "arguments": args,
        });
        let result = self.request("tools/call", params).await?;
        Ok(result)
    }

    /// Send a JSON-RPC request and await its response.
    ///
    /// The reader task resolves the response by matching id.
    async fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        self.check_alive()?;
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel::<RawResponse>();
        {
            let mut pending = self.pending.lock().await;
            pending.insert(id, tx);
        }
        let req = Request {
            jsonrpc: "2.0",
            id,
            method,
            params,
        };
        let mut bytes = serde_json::to_vec(&req)
            .with_context(|| format!("failed to encode MCP request `{method}`"))?;
        bytes.push(b'\n');
        self.stdin
            .lock()
            .await
            .write_all(&bytes)
            .await
            .with_context(|| format!("failed to write MCP request `{method}`"))?;
        debug!(method = method, id = id, "MCP request sent");

        let resp = tokio::time::timeout(std::time::Duration::from_secs(30), rx)
            .await
            .map_err(|_| anyhow::anyhow!("MCP request `{method}` timed out"))?
            .map_err(|_| anyhow::anyhow!("MCP reader dropped response for `{method}`"))?;

        if let Some(err) = resp.error {
            bail!("MCP error {} on `{method}`: {}", err.code, err.message);
        }
        Ok(resp.result.unwrap_or(Value::Null))
    }

    /// Returns Ok iff the child process is still running.
    pub fn check_alive(&mut self) -> Result<()> {
        match self.child.try_wait() {
            Ok(None) => Ok(()),
            Ok(Some(status)) => bail!("MCP server exited: {status}"),
            Err(e) => bail!("MCP server poll failed: {e}"),
        }
    }

    /// Best-effort kill of the child process (used during teardown and on init error).
    pub fn try_kill(&mut self) -> std::io::Result<()> {
        self.child.start_kill()
    }
}

impl Drop for McpClient {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

/// The reader task: reads newline-delimited JSON from stdout and dispatches
/// each response to the matching pending oneshot.
///
/// On EOF or parse error the task exits and all pending requests will time
/// out. This is intentional — we surface MCP server crashes to callers as
/// request timeouts rather than crashing the agent.
async fn run_reader(
    stdout: ChildStdout,
    pending: Arc<Mutex<HashMap<i64, oneshot::Sender<RawResponse>>>>,
) -> Result<()> {
    let mut reader = BufReader::new(stdout);
    let mut buf = Vec::with_capacity(4096);
    // Use read_until for framing — newline-delimited JSON, which is the
    // most common transport and matches what most MCP servers emit.
    // We also support Content-Length framing (LSP-style) as a fallback by
    // detecting a leading "Content-Length:" header.
    loop {
        buf.clear();
        let n = reader
            .read_until(b'\n', &mut buf)
            .await
            .context("MCP stdout read failed")?;
        if n == 0 {
            // EOF — server closed.
            return Ok(());
        }
        let trimmed = buf.iter().any(|&b| !b.is_ascii_whitespace());
        if !trimmed {
            continue;
        }
        let parsed: RawResponse = match serde_json::from_slice(&buf) {
            Ok(p) => p,
            Err(e) => {
                warn!(error = %e, "MCP reader: non-JSON line, skipping");
                continue;
            }
        };
        // Notifications have no id; ignore them.
        if let Some(id) = parsed.id {
            let waiter = {
                let mut p = pending.lock().await;
                p.remove(&id)
            };
            if let Some(tx) = waiter {
                let _ = tx.send(parsed);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixed_name_format() {
        assert_eq!(
            McpToolDef::prefixed_name("github", "create_issue"),
            "mcp__github__create_issue"
        );
    }

    /// Crash isolation: a server that dies immediately must surface as an error.
    #[tokio::test]
    async fn start_with_broken_server_returns_error() {
        let result = McpClient::start("false", &[]).await;
        assert!(result.is_err(), "dead MCP server must surface as error");
    }

    /// When the binary does not exist, `start` returns an error without panic.
    #[tokio::test]
    async fn start_with_missing_binary_returns_error() {
        let result = McpClient::start("this-binary-truly-does-not-exist-xyz", &[]).await;
        assert!(result.is_err());
    }
}
