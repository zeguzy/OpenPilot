//! Task 17.7 — E2E: MCP server provides tool → Task uses it → result integrated.
//!
//! Spawns a minimal echo MCP server (inline Python), connects an `McpClient`,
//! lists the exposed tool, calls it, and verifies the echo result.

use opca_core::extensions::McpClient;
use serde_json::json;

const ECHO_MCP_SERVER_PY: &str = r#"
import sys, json

def emit(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()

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
                "serverInfo": {"name": "echo-mcp", "version": "0.1.0"},
            },
        })
    elif method == "notifications/initialized":
        pass
    elif method == "tools/list":
        emit({
            "jsonrpc": "2.0",
            "id": rid,
            "result": {
                "tools": [
                    {
                        "name": "echo",
                        "description": "Echo back the provided message",
                        "inputSchema": {
                            "type": "object",
                            "properties": {"message": {"type": "string"}},
                            "required": ["message"],
                        },
                    }
                ]
            },
        })
    elif method == "tools/call":
        params = req.get("params", {})
        name = params.get("name")
        args = params.get("arguments", {})
        if name == "echo":
            msg = args.get("message", "")
            emit({
                "jsonrpc": "2.0",
                "id": rid,
                "result": {
                    "content": [{"type": "text", "text": f"echo: {msg}"}],
                    "isError": False,
                },
            })
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

fn python3() -> Option<String> {
    let cmd = if std::process::Command::new("python3")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        "python3"
    } else if std::process::Command::new("python")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        "python"
    } else {
        return None;
    };
    Some(cmd.to_string())
}

#[tokio::test]
#[ignore = "E2E: requires python3 for inline MCP server"]
async fn e2e_mcp_server_provides_echo_tool() {
    let Some(py) = python3() else {
        eprintln!("[skip] python not available on host");
        return;
    };

    let mut client = McpClient::start(&py, &["-c", ECHO_MCP_SERVER_PY])
        .await
        .expect("echo MCP server should start");

    assert_eq!(client.server_name(), Some("echo-mcp"));

    let tools = client.list_tools().await.expect("tools/list");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "echo");
    assert!(!tools[0].description.is_empty());

    let result = client
        .call_tool("echo", &json!({"message": "hello from e2e"}))
        .await
        .expect("tools/call echo");

    let text = result
        .get("content")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|item| item.get("text"))
        .and_then(|t| t.as_str())
        .expect("content[0].text present");

    assert!(
        text.contains("hello from e2e"),
        "echo result should contain the message, got: {text}"
    );
}
