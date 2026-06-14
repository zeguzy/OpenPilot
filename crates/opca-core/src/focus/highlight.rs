use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{FocusContract, FocusError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warning,
    Blocking,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Highlight {
    pub tag: String,
    pub severity: Severity,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl Highlight {
    #[must_use]
    pub fn new(tag: &str, severity: Severity, summary: &str) -> Self {
        Self {
            tag: tag.to_string(),
            severity,
            summary: summary.to_string(),
            detail: None,
        }
    }

    #[must_use]
    pub fn with_detail(mut self, detail: &str) -> Self {
        self.detail = Some(detail.to_string());
        self
    }

    pub fn validate(&self, contract: &FocusContract) -> Result<(), FocusError> {
        if contract.contains(&self.tag) {
            Ok(())
        } else {
            Err(FocusError::NotInFocus(self.tag.clone()))
        }
    }
}

pub struct ReportHighlightTool;

impl ReportHighlightTool {
    #[must_use]
    pub const fn name() -> &'static str {
        "report_highlight"
    }

    #[must_use]
    pub const fn description() -> &'static str {
        "Report an important finding to the orchestrator. The tag must match a dimension in your focus contract."
    }

    #[must_use]
    pub fn parameters_schema() -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "tag": { "type": "string", "description": "Must match a focus dimension" },
                "severity": { "type": "string", "enum": ["info", "warning", "blocking"] },
                "summary": { "type": "string" },
                "detail": { "type": "string" }
            },
            "required": ["tag", "severity", "summary"]
        })
    }
}
