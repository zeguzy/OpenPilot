//! Workspace cleanup scheduling with configurable delay (Task 5.12).
//!
//! After a Task is Archived, its workspace is scheduled for cleanup after a
//! configurable delay (default 3 days). [`CleanupSchedule::cleanup_now`] forces
//! immediate removal for tests and explicit reclamation.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::r#trait::WorkspaceError;

/// Default cleanup delay (3 days).
pub const DEFAULT_CLEANUP_DELAY: Duration = Duration::from_secs(3 * 24 * 60 * 60);

#[derive(Debug, Clone)]
struct PendingCleanup {
    path: PathBuf,
    due_at: SystemTime,
}

/// Records pending workspace cleanups and processes them once their delay
/// expires. Test friendly via [`CleanupSchedule::cleanup_now`].
#[derive(Debug, Default)]
pub struct CleanupSchedule {
    pending: Vec<PendingCleanup>,
}

impl CleanupSchedule {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Schedule `path` for cleanup after `delay`.
    pub fn schedule(&mut self, path: impl Into<PathBuf>, delay: Duration) {
        self.schedule_at(SystemTime::now(), path, delay);
    }

    /// Schedule `path` for cleanup at `now + delay`. The explicit `now` makes
    /// tests deterministic across the schedule/due boundary.
    pub fn schedule_at(&mut self, now: SystemTime, path: impl Into<PathBuf>, delay: Duration) {
        self.pending.push(PendingCleanup {
            path: path.into(),
            due_at: now + delay,
        });
    }

    /// Schedule with the default 3-day delay.
    pub fn schedule_default(&mut self, path: impl Into<PathBuf>) {
        self.schedule(path, DEFAULT_CLEANUP_DELAY);
    }

    /// Returns `true` if `path` is currently scheduled.
    #[must_use]
    pub fn is_scheduled(&self, path: &Path) -> bool {
        self.pending.iter().any(|p| p.path == path)
    }

    /// Returns the paths whose delay has expired and removes them from
    /// schedule. Does **not** touch the filesystem; callers decide what to do.
    pub fn due(&mut self) -> Vec<PathBuf> {
        let now = SystemTime::now();
        let mut due = Vec::new();
        self.pending.retain(|p| {
            if p.due_at <= now {
                due.push(p.path.clone());
                false
            } else {
                true
            }
        });
        due
    }

    /// Forcibly remove `path` from the schedule **and** the filesystem
    /// immediately, regardless of remaining delay.
    pub fn cleanup_now(&mut self, path: &Path) -> Result<(), WorkspaceError> {
        self.pending.retain(|p| p.path != path);
        if path.exists() {
            if path.is_dir() {
                std::fs::remove_dir_all(path)?;
            } else {
                std::fs::remove_file(path)?;
            }
        }
        Ok(())
    }

    /// Number of pending cleanups.
    #[must_use]
    pub fn len(&self) -> usize {
        self.pending.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// Process all due cleanups immediately, removing each from disk.
    /// Returns the number of workspaces successfully removed.
    pub fn run_due(&mut self) -> usize {
        let due = self.due();
        let mut removed = 0;
        for path in due {
            if path.exists() {
                let res = if path.is_dir() {
                    std::fs::remove_dir_all(&path)
                } else {
                    std::fs::remove_file(&path)
                };
                if res.is_ok() {
                    removed += 1;
                }
            } else {
                removed += 1;
            }
        }
        removed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_default_marks_pending() {
        let mut s = CleanupSchedule::new();
        s.schedule_default(Path::new("/tmp/foo"));
        assert!(s.is_scheduled(Path::new("/tmp/foo")));
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn due_returns_only_expired() {
        let mut s = CleanupSchedule::new();
        s.schedule(Path::new("/tmp/a"), Duration::from_secs(10));
        s.schedule(Path::new("/tmp/b"), Duration::from_secs(0));

        let due = s.due();
        assert_eq!(due, vec![PathBuf::from("/tmp/b")]);
        assert_eq!(s.len(), 1);
        assert!(!s.is_scheduled(Path::new("/tmp/b")));
    }

    #[test]
    fn cleanup_now_removes_disk_dir() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path().join("ws");
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(dir.join("f.txt"), b"x").expect("write");

        let mut s = CleanupSchedule::new();
        s.schedule_default(&dir);
        assert!(s.is_scheduled(&dir));

        s.cleanup_now(&dir).expect("cleanup");
        assert!(!s.is_scheduled(&dir));
        assert!(!dir.exists());
    }

    #[test]
    fn run_due_removes_expired_from_disk() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let a = tmp.path().join("a");
        let b = tmp.path().join("b");
        std::fs::create_dir_all(&a).expect("mkdir");
        std::fs::create_dir_all(&b).expect("mkdir");

        let mut s = CleanupSchedule::new();
        s.schedule(&a, Duration::from_secs(0));
        s.schedule(&b, Duration::from_secs(60));

        let removed = s.run_due();
        assert_eq!(removed, 1);
        assert!(!a.exists());
        assert!(b.exists());
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn cleanup_now_on_missing_path_is_ok() {
        let mut s = CleanupSchedule::new();
        s.schedule_default(Path::new("/does/not/exist"));
        s.cleanup_now(Path::new("/does/not/exist")).expect("ok");
    }
}
