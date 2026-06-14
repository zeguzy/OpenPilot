//! Integration tests for the MCP client (Tasks 13.10–13.11).
//!
//! Spawns a real subprocess speaking a minimal subset of MCP JSON-RPC over
//! stdin/stdout. We use Python because it is universally available on the
//! dev/CI host and lets us express the protocol in ~30 lines without a
//! separate fixture binary.

use std::time::Duration;

use opca_core::extensions::{McpClient, McpToolDef};
use serde_json::json;

/// Inline Python script that implements a tiny MCP server: it speaks
/// newline-delimited JSON-RPC 2.0 and answers `initialize`, `tools/list`,
/// and `tools/call`.
const FAKE_MCP_SERVER_PY: &str = r#"
import sys, json

def emit(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()

# Read newline-delimited requests forever.
for line in iter(sys.stdin.readline, ""):
    line = line.strip()
    if not line:
        continue
    try:
        req = json.loads(line)
    except Exception:
        continue
    method = req.get("method")
    rid = req.get("id")
    if method == "initialize":
        emit({
            "jsonrpc": "2.0",
            "id": rid,
            "result": {
                "protocolVersion": "2024-11-05",
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "fake-mcp", "version": "0.0.1"},
            },
        })
    elif method == "notifications/initialized":
        # No response needed for notifications.
        pass
    elif method == "tools/list":
        emit({
            "jsonrpc": "2.0",
            "id": rid,
            "result": {
                "tools": [
                    {
                        "name": "create_issue",
                        "description": "Create an issue",
                        "inputSchema": {
                            "type": "object",
                            "properties": {"title": {"type": "string"}},
                            "required": ["title"],
                        },
                    },
                    {
                        "name": "list_repos",
                        "description": "List repositories",
                        "inputSchema": {"type": "object"},
                    },
                ]
            },
        })
    elif method == "tools/call":
        params = req.get("params", {})
        name = params.get("name")
        args = params.get("arguments", {})
        if name == "create_issue":
            title = args.get("title", "untitled")
            emit({
                "jsonrpc": "2.0",
                "id": rid,
                "result": {
                    "content": [{"type": "text", "text": f"created issue: {title}"}],
                    "isError": False,
                },
            })
        elif name == "crash":
            # Simulate a tool that crashes the server.
            sys.exit(137)
        else:
            emit({
                "jsonrpc": "2.0",
                "id": rid,
                "error": {"code": -32601, "message": f"unknown tool {name}"},
            })
    else:
        emit({
            "jsonrpc": "2.0",
            "id": rid,
            "error": {"code": -32601, "message": f"unknown method {method}"},
        })
"#;

fn python3() -> String {
    if std::process::Command::new("python3")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        "python3".to_string()
    } else {
        "python".to_string()
    }
}

/// Skip the test if Python is not available on the host.
fn require_python() -> Option<String> {
    let cmd = python3();
    if std::process::Command::new(&cmd)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        Some(cmd)
    } else {
        None
    }
}

fn spawn_fake_server() -> Option<(String, Vec<String>)> {
    let py = require_python()?;
    Some((py, vec!["-c".to_string(), FAKE_MCP_SERVER_PY.to_string()]))
}

#[tokio::test]
async fn mcp_client_initialize_and_list_tools() {
    let Some((cmd, server_args)) = spawn_fake_server() else {
        eprintln!("[skip] python not available on host");
        return;
    };
    let argv: Vec<&str> = server_args.iter().map(String::as_str).collect();
    let mut client = McpClient::start(&cmd, &argv)
        .await
        .expect("fake MCP server should start");

    assert_eq!(client.server_name(), Some("fake-mcp"));

    let tools = client
        .list_tools()
        .await
        .expect("tools/list should succeed");
    assert_eq!(tools.len(), 2);
    assert!(tools.iter().any(|t| t.name == "create_issue"));
    assert!(tools.iter().any(|t| t.name == "list_repos"));

    let create_issue = tools
        .iter()
        .find(|t| t.name == "create_issue")
        .expect("create_issue tool present");
    assert!(create_issue.description.contains("Create"));
}

#[tokio::test]
async fn mcp_client_call_tool_returns_result() {
    let Some((cmd, server_args)) = spawn_fake_server() else {
        eprintln!("[skip] python not available on host");
        return;
    };
    let argv: Vec<&str> = server_args.iter().map(String::as_str).collect();
    let mut client = McpClient::start(&cmd, &argv)
        .await
        .expect("fake MCP server should start");

    let result = client
        .call_tool("create_issue", &json!({"title": "hello"}))
        .await
        .expect("tools/call should succeed");

    let content = result
        .get("content")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|item| item.get("text"))
        .and_then(|t| t.as_str())
        .expect("content[0].text present");
    assert!(content.contains("created issue: hello"));
}

#[tokio::test]
async fn mcp_client_unknown_tool_returns_error() {
    let Some((cmd, server_args)) = spawn_fake_server() else {
        eprintln!("[skip] python not available on host");
        return;
    };
    let argv: Vec<&str> = server_args.iter().map(String::as_str).collect();
    let mut client = McpClient::start(&cmd, &argv)
        .await
        .expect("fake MCP server should start");

    let result = client.call_tool("nonexistent", &json!({})).await;
    assert!(result.is_err(), "unknown tool must surface as error");
}

#[tokio::test]
async fn mcp_tool_def_prefix_is_stable() {
    assert_eq!(
        McpToolDef::prefixed_name("github", "create_issue"),
        "mcp__github__create_issue"
    );
    // Verify the format round-trips into something an Agent can recognize.
    let name = McpToolDef::prefixed_name("github", "create_issue");
    assert!(name.starts_with("mcp__"));
    assert!(name.contains("github"));
    assert!(name.ends_with("create_issue"));
}

// ---------------------------------------------------------------------------
// Crash isolation (spec scenario)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn mcp_client_crash_does_not_panic_caller() {
    let Some((cmd, server_args)) = spawn_fake_server() else {
        eprintln!("[skip] python not available on host");
        return;
    };
    let argv: Vec<&str> = server_args.iter().map(String::as_str).collect();
    let mut client = McpClient::start(&cmd, &argv)
        .await
        .expect("fake MCP server should start");

    // Call the `crash` tool — the fake server exits with code 137.
    let _ = client.call_tool("crash", &json!({})).await;

    // Wait briefly for the child to actually die.
    tokio::time::sleep(Duration::from_millis(100)).await;

    // The caller can observe the crash via check_alive and continue running.
    let alive = client.check_alive();
    assert!(alive.is_err(), "server should be reported dead after crash");

    // Subsequent requests surface as errors rather than panicking.
    let next = client
        .call_tool("create_issue", &json!({"title": "x"}))
        .await;
    assert!(next.is_err(), "requests after crash must error");
}

#[tokio::test]
async fn mcp_client_start_with_missing_binary_errors() {
    let result = McpClient::start("definitely-not-a-real-binary-xyz", &[]).await;
    assert!(result.is_err());
}
