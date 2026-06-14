use serde::{Deserialize, Serialize};

use crate::focus::Severity;

use super::agent::ModelTier;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AuditVerdict {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Finding {
    pub severity: Severity,
    pub location: String,
    pub issue: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AuditReport {
    #[serde(default)]
    pub task_id: String,
    pub verdict: AuditVerdict,
    pub confidence: f64,
    pub findings: Vec<Finding>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AuditDecision {
    pub report: AuditReport,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub override_verdict: Option<AuditVerdict>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub override_reason: Option<String>,
}

impl AuditDecision {
    #[must_use]
    pub const fn accept(report: AuditReport) -> Self {
        Self {
            report,
            override_verdict: None,
            override_reason: None,
        }
    }

    #[must_use]
    pub fn override_to(report: AuditReport, verdict: AuditVerdict, reason: &str) -> Self {
        Self {
            report,
            override_verdict: Some(verdict),
            override_reason: Some(reason.to_string()),
        }
    }

    #[must_use]
    pub fn effective_verdict(&self) -> AuditVerdict {
        self.override_verdict.unwrap_or(self.report.verdict)
    }

    #[must_use]
    pub const fn was_overridden(&self) -> bool {
        self.override_verdict.is_some()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AuditManifest {
    pub task_id: String,
    pub model_tier: ModelTier,
    pub focus: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdict_serde_lowercase() {
        assert_eq!(
            serde_json::to_string(&AuditVerdict::Pass).unwrap(),
            "\"pass\""
        );
        assert_eq!(
            serde_json::to_string(&AuditVerdict::Warn).unwrap(),
            "\"warn\""
        );
        assert_eq!(
            serde_json::to_string(&AuditVerdict::Fail).unwrap(),
            "\"fail\""
        );
    }

    #[test]
    fn verdict_deserialize_lowercase() {
        let v: AuditVerdict = serde_json::from_str("\"pass\"").unwrap();
        assert_eq!(v, AuditVerdict::Pass);
        let v: AuditVerdict = serde_json::from_str("\"warn\"").unwrap();
        assert_eq!(v, AuditVerdict::Warn);
        let v: AuditVerdict = serde_json::from_str("\"fail\"").unwrap();
        assert_eq!(v, AuditVerdict::Fail);
    }

    #[test]
    fn report_serde_roundtrip() {
        let report = AuditReport {
            task_id: "task-1".to_string(),
            verdict: AuditVerdict::Warn,
            confidence: 0.7,
            findings: vec![Finding {
                severity: Severity::Warning,
                location: "src/auth.rs:42".to_string(),
                issue: "missing null check".to_string(),
            }],
            summary: "minor issue found".to_string(),
        };
        let json = serde_json::to_string(&report).unwrap();
        let back: AuditReport = serde_json::from_str(&json).unwrap();
        assert_eq!(report, back);
    }

    #[test]
    fn report_deserialize_without_task_id() {
        let json = r#"{"verdict":"pass","confidence":0.9,"findings":[],"summary":"ok"}"#;
        let report: AuditReport = serde_json::from_str(json).unwrap();
        assert_eq!(report.task_id, "");
        assert_eq!(report.verdict, AuditVerdict::Pass);
    }

    #[test]
    fn decision_accept_has_no_override() {
        let report = AuditReport {
            task_id: "t".to_string(),
            verdict: AuditVerdict::Pass,
            confidence: 1.0,
            findings: vec![],
            summary: "ok".to_string(),
        };
        let decision = AuditDecision::accept(report.clone());
        assert!(!decision.was_overridden());
        assert_eq!(decision.effective_verdict(), AuditVerdict::Pass);
        assert_eq!(decision.report, report);
    }

    #[test]
    fn decision_override_changes_effective_verdict() {
        let report = AuditReport {
            task_id: "t".to_string(),
            verdict: AuditVerdict::Fail,
            confidence: 0.8,
            findings: vec![],
            summary: "tests failed".to_string(),
        };
        let decision = AuditDecision::override_to(
            report,
            AuditVerdict::Pass,
            "tests were pre-existing failures",
        );
        assert!(decision.was_overridden());
        assert_eq!(decision.effective_verdict(), AuditVerdict::Pass);
        assert_eq!(decision.report.verdict, AuditVerdict::Fail);
        assert_eq!(
            decision.override_reason.as_deref(),
            Some("tests were pre-existing failures")
        );
    }
}
