use async_trait::async_trait;
use serde_json::{Value, json};

use crate::provider::{ToolEffects, ToolResult};
use crate::tools::tool::{Tool, ToolContext, matches_pattern, resolve_path, walk};

pub struct GrepTool;

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &'static str {
        "grep"
    }

    fn description(&self) -> &'static str {
        "Search for a substring in files under a path."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Substring to search for." },
                "path": { "type": "string", "description": "File or directory to search." },
                "include": { "type": "string", "description": "Filename glob filter, e.g. \"*.rs\"." }
            },
            "required": ["pattern", "path"]
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
        let path = args
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("missing 'path' argument"))?;
        let include = args.get("include").and_then(Value::as_str);

        let resolved = resolve_path(ctx, path);

        let files = if ctx.fs.is_dir(&resolved) {
            let mut out = Vec::new();
            walk(ctx.fs.as_ref(), &resolved, &mut out);
            out
        } else if ctx.fs.exists(&resolved) {
            vec![resolved]
        } else {
            Vec::new()
        };

        let mut matches: Vec<String> = Vec::new();
        for file in files {
            if let Some(inc) = include {
                if let Some(name) = file.file_name().and_then(|n| n.to_str()) {
                    if !matches_pattern(inc, name) {
                        continue;
                    }
                }
            }
            let Ok(bytes) = ctx.fs.read(&file) else {
                continue;
            };
            let content = String::from_utf8_lossy(&bytes);
            for (lineno, line) in content.lines().enumerate() {
                if line.contains(pattern) {
                    let display = file
                        .strip_prefix(&ctx.workspace_path)
                        .unwrap_or(&file)
                        .display()
                        .to_string();
                    matches.push(format!("{}:{}: {}", display, lineno + 1, line));
                }
            }
        }

        let content = if matches.is_empty() {
            "no matches".to_string()
        } else {
            matches.join("\n")
        };
        Ok(ToolResult {
            content,
            is_error: false,
        })
    }
}
