use async_trait::async_trait;
use serde_json::{Value, json};

use crate::provider::{ToolEffects, ToolResult};
use crate::tools::tool::{Tool, ToolContext, resolve_path};

pub struct BashTool;

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &'static str {
        "bash"
    }

    fn description(&self) -> &'static str {
        "Run a shell command in the workspace."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Command line (split on whitespace into program + args)."
                },
                "cwd": {
                    "type": "string",
                    "description": "Working directory (relative to workspace, default \".\")."
                }
            },
            "required": ["command"]
        })
    }

    fn effects(&self) -> ToolEffects {
        ToolEffects::Process
    }

    async fn execute(&self, args: &Value, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let command = args
            .get("command")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("missing 'command' argument"))?;
        let cwd = args.get("cwd").and_then(Value::as_str).unwrap_or(".");

        let cwd_path = resolve_path(ctx, cwd);

        let parts: Vec<&str> = command.split_whitespace().collect();
        if parts.is_empty() {
            anyhow::bail!("empty command");
        }

        let output = ctx
            .proc
            .execute(parts[0], &parts[1..], &cwd_path)
            .map_err(|e| anyhow::anyhow!("process failed: {e}"))?;

        let is_error = !output.success();
        let content = if output.success() {
            if output.stdout.is_empty() {
                "(no output)".to_string()
            } else {
                output.stdout
            }
        } else {
            format!(
                "exit code: {}\n--- stdout ---\n{}\n--- stderr ---\n{}",
                output.exit_code, output.stdout, output.stderr
            )
        };

        Ok(ToolResult { content, is_error })
    }
}
