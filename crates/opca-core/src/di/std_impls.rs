use super::{Clock, FileSystem, Process, ProcessOutput, Random};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

#[derive(Default, Clone)]
pub struct StdFileSystem;

impl FileSystem for StdFileSystem {
    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        fs::read(path)
    }

    fn write(&self, path: &Path, data: &[u8]) -> std::io::Result<()> {
        fs::write(path, data)
    }

    fn append(&self, path: &Path, data: &[u8]) -> std::io::Result<()> {
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)?;
        file.write_all(data)
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn remove(&self, path: &Path) -> std::io::Result<()> {
        if path.is_dir() {
            fs::remove_dir_all(path)
        } else {
            fs::remove_file(path)
        }
    }

    fn create_dir_all(&self, path: &Path) -> std::io::Result<()> {
        fs::create_dir_all(path)
    }

    fn list_dir(&self, path: &Path) -> std::io::Result<Vec<PathBuf>> {
        let mut entries: Vec<PathBuf> = fs::read_dir(path)?
            .filter_map(std::result::Result::ok)
            .map(|e| e.path())
            .collect();
        entries.sort();
        Ok(entries)
    }

    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }
}

#[derive(Default, Clone)]
pub struct StdProcess;

impl Process for StdProcess {
    fn execute(&self, command: &str, args: &[&str], cwd: &Path) -> std::io::Result<ProcessOutput> {
        let output = Command::new(command).args(args).current_dir(cwd).output()?;
        Ok(ProcessOutput {
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            exit_code: output.status.code().unwrap_or(-1),
        })
    }
}

#[derive(Default, Clone)]
pub struct StdClock;

impl Clock for StdClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

#[derive(Default)]
pub struct StdRandom;

impl Random for StdRandom {
    fn uuid(&self) -> uuid::Uuid {
        uuid::Uuid::new_v4()
    }

    fn random_bytes(&self, len: usize) -> Vec<u8> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut buf = Vec::with_capacity(len);
        let mut counter = 0u64;
        while buf.len() < len {
            let mut hasher = DefaultHasher::new();
            SystemTime::now().hash(&mut hasher);
            counter.hash(&mut hasher);
            counter += 1;
            let hash = hasher.finish();
            buf.extend_from_slice(&hash.to_le_bytes());
        }
        buf.truncate(len);
        buf
    }
}
