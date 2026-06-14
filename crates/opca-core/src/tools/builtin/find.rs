use async_trait::async_trait;
use serde_json::{Value, json};

use crate::provider::{ToolEffects, ToolResult};
use crate::tools::tool::{Tool, ToolContext, matches_pattern, resolve_path, walk};

pub struct FindTool;

#[async_trait]
impl Tool for FindTool {
    fn name(&self) -> &'static str {
        "find"
    }

    fn description(&self) -> &'static str {
        "Find files under a path whose name matches a glob pattern."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Filename glob, e.g. \"*.rs\"." },
                "path": { "type": "string", "description": "Search root (default workspace)." }
            },
            "required": ["pattern"]
        })
    }

    fn effects(&self) -> ToolEffects {
        ToolEffects::Read
    }

    async fn execute(&self, args: &Value, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let pattern = args
            .get("pattern")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("missing 'pattern' argument"))?;
        let path = args.get("path").and_then(Value::as_str).unwrap_or(".");
        let resolved = resolve_path(ctx, path);

        let mut all = Vec::new();
        if ctx.fs.is_dir(&resolved) {
            walk(ctx.fs.as_ref(), &resolved, &mut all);
        } else if ctx.fs.exists(&resolved) {
            all.push(resolved.clone());
        }

        let mut found: Vec<String> = Vec::new();
        for file in all {
            if let Some(name) = file.file_name().and_then(|n| n.to_str()) {
                if matches_pattern(pattern, name) {
                    let display = file
                        .strip_prefix(&ctx.workspace_path)
                        .unwrap_or(&file)
                        .display()
                        .to_string();
                    found.push(display);
                }
            }
        }

        let content = if found.is_empty() {
            "no files matched".to_string()
        } else {
            found.join("\n")
        };
        Ok(ToolResult {
            content,
            is_error: false,
        })
    }
}
