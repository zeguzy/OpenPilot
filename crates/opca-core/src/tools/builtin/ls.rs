use async_trait::async_trait;
use serde_json::{Value, json};

use crate::provider::{ToolEffects, ToolResult};
use crate::tools::tool::{Tool, ToolContext, resolve_path};

pub struct LsTool;

#[async_trait]
impl Tool for LsTool {
    fn name(&self) -> &'static str {
        "ls"
    }

    fn description(&self) -> &'static str {
        "List the contents of a directory."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Directory path (default workspace)." }
            }
        })
    }

    fn effects(&self) -> ToolEffects {
        ToolEffects::Read
    }

    async fn execute(&self, args: &Value, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let path = args.get("path").and_then(Value::as_str).unwrap_or(".");
        let resolved = resolve_path(ctx, path);

        let entries = ctx
            .fs
            .list_dir(&resolved)
            .map_err(|e| anyhow::anyhow!("list_dir failed: {e}"))?;

        let mut lines: Vec<String> = Vec::new();
        for entry in entries {
            let display = entry
                .strip_prefix(&ctx.workspace_path)
                .unwrap_or(&entry)
                .display()
                .to_string();
            let suffix = if ctx.fs.is_dir(&entry) { "/" } else { "" };
            lines.push(format!("{display}{suffix}"));
        }

        let content = if lines.is_empty() {
            "(empty)".to_string()
        } else {
            lines.join("\n")
        };
        Ok(ToolResult {
            content,
            is_error: false,
        })
    }
}
