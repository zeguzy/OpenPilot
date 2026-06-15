use serde::{Deserialize, Serialize};

use crate::focus::Severity;

use super::agent::ModelTier;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuditVerdict {
    /// All checks passed; the Task's work is correct and complete.
    /// Only this verdict terminates a continuation chain.
    Confirmed,
    /// The Task claimed success but the work is actually incomplete or
    /// incorrect (e.g. a core function is still unimplemented).
    /// Triggers continuation with feedback.
    FalsePositive,
    /// A concrete, fixable problem was found (e.g. a failing test).
    /// Triggers continuation with feedback pointing at the findings.
    NeedsFix,
    /// The Audit Agent cannot decide automatically (low confidence,
    /// ambiguous diff). Halts the chain and escalates to the user.
    NeedsHumanReview,
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
    fn verdict_serde_snake_case() {
        assert_eq!(
            serde_json::to_string(&AuditVerdict::Confirmed).unwrap(),
            "\"confirmed\""
        );
        assert_eq!(
            serde_json::to_string(&AuditVerdict::FalsePositive).unwrap(),
            "\"false_positive\""
        );
        assert_eq!(
            serde_json::to_string(&AuditVerdict::NeedsFix).unwrap(),
            "\"needs_fix\""
        );
        assert_eq!(
            serde_json::to_string(&AuditVerdict::NeedsHumanReview).unwrap(),
            "\"needs_human_review\""
        );
    }

    #[test]
    fn verdict_deserialize_snake_case() {
        let v: AuditVerdict = serde_json::from_str("\"confirmed\"").unwrap();
        assert_eq!(v, AuditVerdict::Confirmed);
        let v: AuditVerdict = serde_json::from_str("\"false_positive\"").unwrap();
        assert_eq!(v, AuditVerdict::FalsePositive);
        let v: AuditVerdict = serde_json::from_str("\"needs_fix\"").unwrap();
        assert_eq!(v, AuditVerdict::NeedsFix);
        let v: AuditVerdict = serde_json::from_str("\"needs_human_review\"").unwrap();
        assert_eq!(v, AuditVerdict::NeedsHumanReview);
    }

    #[test]
    fn verdict_false_positive_roundtrip() {
        let json = serde_json::to_string(&AuditVerdict::FalsePositive).unwrap();
        let back: AuditVerdict = serde_json::from_str(&json).unwrap();
        assert_eq!(back, AuditVerdict::FalsePositive);
    }

    #[test]
    fn verdict_needs_human_review_roundtrip() {
        let json = serde_json::to_string(&AuditVerdict::NeedsHumanReview).unwrap();
        let back: AuditVerdict = serde_json::from_str(&json).unwrap();
        assert_eq!(back, AuditVerdict::NeedsHumanReview);
    }

    #[test]
    fn report_serde_roundtrip() {
        let report = AuditReport {
            task_id: "task-1".to_string(),
            verdict: AuditVerdict::FalsePositive,
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
        let json = r#"{"verdict":"confirmed","confidence":0.9,"findings":[],"summary":"ok"}"#;
        let report: AuditReport = serde_json::from_str(json).unwrap();
        assert_eq!(report.task_id, "");
        assert_eq!(report.verdict, AuditVerdict::Confirmed);
    }

    #[test]
    fn decision_accept_has_no_override() {
        let report = AuditReport {
            task_id: "t".to_string(),
            verdict: AuditVerdict::Confirmed,
            confidence: 1.0,
            findings: vec![],
            summary: "ok".to_string(),
        };
        let decision = AuditDecision::accept(report.clone());
        assert!(!decision.was_overridden());
        assert_eq!(decision.effective_verdict(), AuditVerdict::Confirmed);
        assert_eq!(decision.report, report);
    }

    #[test]
    fn decision_override_changes_effective_verdict() {
        let report = AuditReport {
            task_id: "t".to_string(),
            verdict: AuditVerdict::NeedsFix,
            confidence: 0.8,
            findings: vec![],
            summary: "tests failed".to_string(),
        };
        let decision = AuditDecision::override_to(
            report,
            AuditVerdict::Confirmed,
            "tests were pre-existing failures",
        );
        assert!(decision.was_overridden());
        assert_eq!(decision.effective_verdict(), AuditVerdict::Confirmed);
        assert_eq!(decision.report.verdict, AuditVerdict::NeedsFix);
        assert_eq!(
            decision.override_reason.as_deref(),
            Some("tests were pre-existing failures")
        );
    }
}
