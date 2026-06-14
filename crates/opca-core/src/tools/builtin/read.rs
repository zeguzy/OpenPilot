use async_trait::async_trait;
use serde_json::{Value, json};

use crate::provider::{ToolEffects, ToolResult};
use crate::tools::tool::{Tool, ToolContext, resolve_path};

pub struct ReadTool;

#[async_trait]
impl Tool for ReadTool {
    fn name(&self) -> &'static str {
        "read"
    }

    fn description(&self) -> &'static str {
        "Read the contents of a file from the workspace."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Relative (to workspace) or absolute file path."
                }
            },
            "required": ["path"]
        })
    }

    fn effects(&self) -> ToolEffects {
        ToolEffects::Read
    }

    async fn execute(&self, args: &Value, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let path = args
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("missing 'path' argument"))?;
        let resolved = resolve_path(ctx, path);
        let bytes = ctx
            .fs
            .read(&resolved)
            .map_err(|e| anyhow::anyhow!("read failed: {e}"))?;
        let content = String::from_utf8_lossy(&bytes).into_owned();
        Ok(ToolResult {
            content,
            is_error: false,
        })
    }
}
