//! Notification level — respects user activity (Task 12.11).
//!
//! When a Task completes while the user is occupied:
//! - Low-risk completions SHALL be processed silently (auto-merge, result
//!   enters memory).
//! - High-risk completions SHALL be queued as "pending review" without
//!   interrupting the user.
//!
//! See `design.md` §D9 (完成通知边界情况) and
//! `specs/completion-pipeline/spec.md`.

use super::review::RiskLevel;
use crate::audit::AuditVerdict;

/// How loudly a completion should surface to the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationLevel {
    /// Low-risk auto-merged task — just a log entry.
    Silent,
    /// High-risk task queued for explicit user review.
    PendingReview,
}

/// Decide the notification level from the assessed risk and (optional)
/// Audit verdict.
///
/// Rules (mirrors `design.md` §D9):
/// - Low risk, no audit or audit confirmed → `Silent` (auto-merged).
/// - High risk regardless of audit → `PendingReview`.
/// - Medium risk with any non-confirmed verdict → `PendingReview`.
/// - Medium risk with confirmed audit (or no audit) → `Silent`.
#[must_use]
pub const fn notification_level(
    risk: RiskLevel,
    verdict: Option<AuditVerdict>,
) -> NotificationLevel {
    match (risk, verdict) {
        (RiskLevel::High, _) => NotificationLevel::PendingReview,
        (
            RiskLevel::Medium,
            Some(
                AuditVerdict::FalsePositive
                | AuditVerdict::NeedsFix
                | AuditVerdict::NeedsHumanReview,
            ),
        ) => NotificationLevel::PendingReview,
        (RiskLevel::Medium | RiskLevel::Low, _) => NotificationLevel::Silent,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn low_risk_always_silent() {
        assert_eq!(
            notification_level(RiskLevel::Low, None),
            NotificationLevel::Silent
        );
        assert_eq!(
            notification_level(RiskLevel::Low, Some(AuditVerdict::NeedsFix)),
            NotificationLevel::Silent
        );
    }

    #[test]
    fn high_risk_always_pending_review() {
        assert_eq!(
            notification_level(RiskLevel::High, None),
            NotificationLevel::PendingReview
        );
        assert_eq!(
            notification_level(RiskLevel::High, Some(AuditVerdict::Confirmed)),
            NotificationLevel::PendingReview
        );
    }

    #[test]
    fn medium_confirmed_silent() {
        assert_eq!(
            notification_level(RiskLevel::Medium, Some(AuditVerdict::Confirmed)),
            NotificationLevel::Silent
        );
        assert_eq!(
            notification_level(RiskLevel::Medium, None),
            NotificationLevel::Silent
        );
    }

    #[test]
    fn medium_non_confirmed_pending_review() {
        assert_eq!(
            notification_level(RiskLevel::Medium, Some(AuditVerdict::FalsePositive)),
            NotificationLevel::PendingReview
        );
        assert_eq!(
            notification_level(RiskLevel::Medium, Some(AuditVerdict::NeedsFix)),
            NotificationLevel::PendingReview
        );
        assert_eq!(
            notification_level(RiskLevel::Medium, Some(AuditVerdict::NeedsHumanReview)),
            NotificationLevel::PendingReview
        );
    }
}
