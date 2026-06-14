use async_trait::async_trait;
use serde_json::{Value, json};

use crate::provider::{ToolEffects, ToolResult};
use crate::tools::tool::{Tool, ToolContext, resolve_path};

pub struct EditTool;

#[async_trait]
impl Tool for EditTool {
    fn name(&self) -> &'static str {
        "edit"
    }

    fn description(&self) -> &'static str {
        "Replace the first occurrence of `old_text` with `new_text` in a file."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Relative (to workspace) or absolute file path."
                },
                "old_text": {
                    "type": "string",
                    "description": "Exact text to find."
                },
                "new_text": {
                    "type": "string",
                    "description": "Replacement text."
                }
            },
            "required": ["path", "old_text", "new_text"]
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
        let old_text = args
            .get("old_text")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("missing 'old_text' argument"))?;
        let new_text = args
            .get("new_text")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("missing 'new_text' argument"))?;

        let resolved = resolve_path(ctx, path);
        let bytes = ctx
            .fs
            .read(&resolved)
            .map_err(|e| anyhow::anyhow!("read failed: {e}"))?;
        let content = String::from_utf8_lossy(&bytes).into_owned();

        if !content.contains(old_text) {
            return Ok(ToolResult {
                content: format!("old_text not found in {path}"),
                is_error: true,
            });
        }

        let new_content = content.replacen(old_text, new_text, 1);
        ctx.fs
            .write(&resolved, new_content.as_bytes())
            .map_err(|e| anyhow::anyhow!("write failed: {e}"))?;
        Ok(ToolResult {
            content: format!("edited {path}"),
            is_error: false,
        })
    }
}
