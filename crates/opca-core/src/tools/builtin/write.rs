use async_trait::async_trait;
use serde_json::{Value, json};

use crate::provider::{ToolEffects, ToolResult};
use crate::tools::tool::{Tool, ToolContext, resolve_path};

pub struct WriteTool;

#[async_trait]
impl Tool for WriteTool {
    fn name(&self) -> &'static str {
        "write"
    }

    fn description(&self) -> &'static str {
        "Write content to a file in the workspace, overwriting any existing content."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Relative (to workspace) or absolute file path."
                },
                "content": {
                    "type": "string",
                    "description": "Full content to write."
                }
            },
            "required": ["path", "content"]
        })
    }

    fn effects(&self) -> ToolEffects {
        ToolEffects::Write
    }

    async fn execute(&self, args: &Value, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let path = args
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("missing 'path' argument"))?;
        let content = args
            .get("content")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("missing 'content' argument"))?;

        let resolved = resolve_path(ctx, path);
        ctx.fs
            .write(&resolved, content.as_bytes())
            .map_err(|e| anyhow::anyhow!("write failed: {e}"))?;
        Ok(ToolResult {
            content: format!("wrote {} bytes to {path}", content.len()),
            is_error: false,
        })
    }
}
