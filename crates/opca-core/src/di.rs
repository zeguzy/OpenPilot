//! Dependency injection traits for testable core logic.
//!
//! All external dependencies (filesystem, subprocess, clock, random)
//! are abstracted behind traits so tests can inject deterministic mocks.

mod std_impls;

pub use std_impls::{StdClock, StdFileSystem, StdProcess, StdRandom};

use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[allow(clippy::module_name_repetitions)]
pub trait FileSystem: Send + Sync {
    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>>;
    fn write(&self, path: &Path, data: &[u8]) -> std::io::Result<()>;
    fn append(&self, path: &Path, data: &[u8]) -> std::io::Result<()>;
    fn exists(&self, path: &Path) -> bool;
    fn remove(&self, path: &Path) -> std::io::Result<()>;
    fn create_dir_all(&self, path: &Path) -> std::io::Result<()>;
    fn list_dir(&self, path: &Path) -> std::io::Result<Vec<PathBuf>>;
    fn is_dir(&self, path: &Path) -> bool;
}

pub trait Process: Send + Sync {
    fn execute(&self, command: &str, args: &[&str], cwd: &Path) -> std::io::Result<ProcessOutput>;
}

#[derive(Debug, Clone)]
#[allow(clippy::module_name_repetitions)]
pub struct ProcessOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

impl ProcessOutput {
    #[must_use]
    pub const fn success(&self) -> bool {
        self.exit_code == 0
    }
}

pub trait Clock: Send + Sync {
    fn now(&self) -> SystemTime;
}

pub trait Random: Send + Sync {
    fn uuid(&self) -> uuid::Uuid;
    fn random_bytes(&self, len: usize) -> Vec<u8>;
}
