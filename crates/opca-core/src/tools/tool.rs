use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::di::{FileSystem, Process};
use crate::provider::{ToolEffects, ToolResult};
use serde_json::Value;

#[derive(Clone)]
pub struct ToolContext {
    pub workspace_path: PathBuf,
    pub fs: Arc<dyn FileSystem>,
    pub proc: Arc<dyn Process>,
}

#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> Value;
    fn effects(&self) -> ToolEffects;
    async fn execute(&self, args: &Value, ctx: &ToolContext) -> anyhow::Result<ToolResult>;
}

pub(crate) fn resolve_path(ctx: &ToolContext, path: &str) -> PathBuf {
    let p = Path::new(path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        ctx.workspace_path.join(p)
    }
}

pub(crate) fn matches_pattern(pattern: &str, name: &str) -> bool {
    if let Some(suffix) = pattern.strip_prefix("*.") {
        return name.ends_with(&format!(".{suffix}"));
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return name.starts_with(prefix);
    }
    name == pattern
}

pub(crate) fn walk(fs: &dyn FileSystem, path: &Path, acc: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs.list_dir(path) {
        for entry in entries {
            if fs.is_dir(&entry) {
                walk(fs, &entry, acc);
            } else {
                acc.push(entry);
            }
        }
    }
}
