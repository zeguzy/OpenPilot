use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::mpsc::UnboundedSender;

use crate::focus::{FocusContract, Highlight, ReportHighlightTool as ReportHighlightDef, Severity};
use crate::provider::{ToolEffects, ToolResult};
use crate::tools::tool::{Tool, ToolContext};

pub struct ReportHighlightTool {
    focus: Arc<FocusContract>,
    highlight_tx: UnboundedSender<Highlight>,
}

impl ReportHighlightTool {
    #[must_use]
    pub const fn new(focus: Arc<FocusContract>, highlight_tx: UnboundedSender<Highlight>) -> Self {
        Self {
            focus,
            highlight_tx,
        }
    }

    fn parse_severity(s: &str) -> anyhow::Result<Severity> {
        match s {
            "info" => Ok(Severity::Info),
            "warning" => Ok(Severity::Warning),
            "blocking" => Ok(Severity::Blocking),
            other => anyhow::bail!("invalid severity: {other}"),
        }
    }
}

#[async_trait]
impl Tool for ReportHighlightTool {
    fn name(&self) -> &str {
        ReportHighlightDef::name()
    }

    fn description(&self) -> &str {
        ReportHighlightDef::description()
    }

    fn parameters_schema(&self) -> Value {
        ReportHighlightDef::parameters_schema()
    }

    fn effects(&self) -> ToolEffects {
        ToolEffects::Read
    }

    async fn execute(&self, args: &Value, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let tag = args
            .get("tag")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("missing 'tag' argument"))?;
        let severity_str = args
            .get("severity")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("missing 'severity' argument"))?;
        let summary = args
            .get("summary")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("missing 'summary' argument"))?;
        let detail = args.get("detail").and_then(Value::as_str);

        let severity = Self::parse_severity(severity_str)?;
        let mut hl = Highlight::new(tag, severity, summary);
        if let Some(d) = detail {
            hl = hl.with_detail(d);
        }

        hl.validate(&self.focus)?;

        self.highlight_tx
            .send(hl)
            .map_err(|_| anyhow::anyhow!("highlight channel closed"))?;

        Ok(ToolResult {
            content: "highlight reported".to_string(),
            is_error: false,
        })
    }
}
