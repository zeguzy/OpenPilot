//! Cleanup stage (Task 12.9).
//!
//! After Memorialize, the workspace is scheduled for delayed removal
//! (configurable, default 3 days). Cold Store data SHALL NOT be affected.
//!
//! See `design.md` §D9 (⑤ Cleanup) and `specs/completion-pipeline/spec.md`.

use std::path::Path;
use std::time::Duration;

use crate::workspace::{CleanupSchedule, DEFAULT_CLEANUP_DELAY};

pub fn schedule_cleanup(
    schedule: &mut CleanupSchedule,
    workspace_path: &Path,
    delay: Duration,
) -> Duration {
    schedule.schedule(workspace_path, delay);
    delay
}

pub fn schedule_cleanup_default(schedule: &mut CleanupSchedule, workspace_path: &Path) -> Duration {
    schedule.schedule_default(workspace_path);
    DEFAULT_CLEANUP_DELAY
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_default_marks_pending() {
        let mut s = CleanupSchedule::new();
        let delay = schedule_cleanup_default(&mut s, Path::new("/tmp/ws-1"));
        assert_eq!(delay, DEFAULT_CLEANUP_DELAY);
        assert!(s.is_scheduled(Path::new("/tmp/ws-1")));
    }

    #[test]
    fn schedule_custom_delay_marks_pending() {
        let mut s = CleanupSchedule::new();
        let delay = Duration::from_secs(60);
        let returned = schedule_cleanup(&mut s, Path::new("/tmp/ws-2"), delay);
        assert_eq!(returned, delay);
        assert!(s.is_scheduled(Path::new("/tmp/ws-2")));
    }

    #[test]
    fn cleanup_now_preserves_other_workspaces() {
        let mut s = CleanupSchedule::new();
        s.schedule_default(Path::new("/tmp/keep"));
        s.schedule_default(Path::new("/tmp/drop"));
        s.cleanup_now(Path::new("/does/not/exist")).unwrap();
        assert!(s.is_scheduled(Path::new("/tmp/keep")));
        assert!(s.is_scheduled(Path::new("/tmp/drop")));
    }
}
