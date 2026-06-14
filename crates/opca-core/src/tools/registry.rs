use std::collections::HashMap;

use super::tool::{Tool, ToolContext};
use crate::provider::{ToolDef, ToolResult};
use serde_json::Value;

pub struct ToolRegistry {
    tools: Vec<Box<dyn Tool>>,
    name_index: HashMap<String, usize>,
}

impl ToolRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            tools: Vec::new(),
            name_index: HashMap::new(),
        }
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) {
        let name = tool.name().to_string();
        self.name_index.insert(name, self.tools.len());
        self.tools.push(tool);
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.name_index.get(name).map(|&i| self.tools[i].as_ref())
    }

    #[must_use]
    pub fn definitions(&self) -> Vec<ToolDef> {
        self.tools
            .iter()
            .map(|t| ToolDef {
                name: t.name().to_string(),
                description: t.description().to_string(),
                parameters: t.parameters_schema(),
                effects: t.effects(),
            })
            .collect()
    }

    pub async fn execute(
        &self,
        name: &str,
        args: &Value,
        ctx: &ToolContext,
    ) -> anyhow::Result<ToolResult> {
        match self.get(name) {
            Some(t) => t.execute(args, ctx).await,
            None => anyhow::bail!("unknown tool: {name}"),
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
