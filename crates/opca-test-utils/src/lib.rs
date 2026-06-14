#![forbid(unsafe_code)]

pub mod scripted_provider;

pub use scripted_provider::ScriptedProvider;

use opca_core::di::{Clock, FileSystem, Process, ProcessOutput, Random};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Default, Clone)]
pub struct MockFileSystem {
    files: Arc<Mutex<HashMap<PathBuf, Vec<u8>>>>,
    dirs: Arc<Mutex<Vec<PathBuf>>>,
}

impl MockFileSystem {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_file(&self, path: impl Into<PathBuf>, content: impl Into<Vec<u8>>) {
        self.files
            .lock()
            .unwrap()
            .insert(path.into(), content.into());
    }
}

impl FileSystem for MockFileSystem {
    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        self.files
            .lock()
            .unwrap()
            .get(path)
            .cloned()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "file not found"))
    }

    fn write(&self, path: &Path, data: &[u8]) -> std::io::Result<()> {
        self.files
            .lock()
            .unwrap()
            .insert(path.to_path_buf(), data.to_vec());
        Ok(())
    }

    fn append(&self, path: &Path, data: &[u8]) -> std::io::Result<()> {
        let mut files = self.files.lock().unwrap();
        files
            .entry(path.to_path_buf())
            .or_default()
            .extend_from_slice(data);
        drop(files);
        Ok(())
    }

    fn exists(&self, path: &Path) -> bool {
        if self.files.lock().unwrap().contains_key(path) {
            return true;
        }
        self.dirs.lock().unwrap().iter().any(|d| d == path)
    }

    fn remove(&self, path: &Path) -> std::io::Result<()> {
        self.files.lock().unwrap().remove(path);
        self.dirs.lock().unwrap().retain(|d| d != path);
        Ok(())
    }

    fn create_dir_all(&self, path: &Path) -> std::io::Result<()> {
        self.dirs.lock().unwrap().push(path.to_path_buf());
        Ok(())
    }

    fn list_dir(&self, path: &Path) -> std::io::Result<Vec<PathBuf>> {
        let children: Vec<PathBuf> = {
            let files = self.files.lock().unwrap();
            files
                .keys()
                .filter(|p| p.parent() == Some(path))
                .cloned()
                .collect()
        };
        let mut sorted = children;
        sorted.sort();
        Ok(sorted)
    }

    fn is_dir(&self, path: &Path) -> bool {
        self.dirs.lock().unwrap().iter().any(|d| d == path)
    }
}

#[derive(Clone)]
pub struct FakeClock {
    current: Arc<Mutex<SystemTime>>,
}

impl FakeClock {
    #[must_use]
    pub fn new(epoch: SystemTime) -> Self {
        Self {
            current: Arc::new(Mutex::new(epoch)),
        }
    }

    pub fn advance(&self, duration: std::time::Duration) {
        let mut current = self.current.lock().unwrap();
        *current += duration;
    }

    pub fn set(&self, time: SystemTime) {
        *self.current.lock().unwrap() = time;
    }
}

impl Default for FakeClock {
    fn default() -> Self {
        Self::new(UNIX_EPOCH)
    }
}

impl Clock for FakeClock {
    fn now(&self) -> SystemTime {
        *self.current.lock().unwrap()
    }
}

#[derive(Default, Clone)]
pub struct FakeRandom {
    counter: Arc<Mutex<u64>>,
}

impl FakeRandom {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl Random for FakeRandom {
    fn uuid(&self) -> uuid::Uuid {
        let mut counter = self.counter.lock().unwrap();
        *counter += 1;
        let val = *counter;
        drop(counter);
        uuid::Uuid::from_u64_pair(val, 0)
    }

    fn random_bytes(&self, len: usize) -> Vec<u8> {
        let mut counter = self.counter.lock().unwrap();
        *counter += 1;
        let val = *counter;
        drop(counter);
        val.to_le_bytes()
            .repeat(len.div_ceil(8))
            .into_iter()
            .take(len)
            .collect()
    }
}

#[derive(Clone)]
pub struct MockProcess {
    responses: Arc<Mutex<HashMap<String, ProcessOutput>>>,
}

impl MockProcess {
    #[must_use]
    pub fn new() -> Self {
        Self {
            responses: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn set_response(&self, key: impl Into<String>, output: ProcessOutput) {
        self.responses.lock().unwrap().insert(key.into(), output);
    }
}

impl Default for MockProcess {
    fn default() -> Self {
        Self::new()
    }
}

impl Process for MockProcess {
    fn execute(
        &self,
        command: &str,
        _args: &[&str],
        _cwd: &Path,
    ) -> std::io::Result<ProcessOutput> {
        self.responses
            .lock()
            .unwrap()
            .get(command)
            .cloned()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no mock response"))
    }
}
