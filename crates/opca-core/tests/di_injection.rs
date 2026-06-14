use opca_core::di::{Clock, FileSystem, Process, ProcessOutput, Random};
use opca_test_utils::{FakeClock, FakeRandom, MockFileSystem, MockProcess};
use std::io::ErrorKind;
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};
use tempfile::tempdir;

fn use_filesystem<F: FileSystem>(fs: &F, path: &Path) -> Vec<u8> {
    fs.read(path).unwrap_or_default()
}

fn use_clock<C: Clock>(clock: &C) -> std::time::SystemTime {
    clock.now()
}

fn use_random<R: Random>(random: &R) -> uuid::Uuid {
    random.uuid()
}

fn use_process<P: Process>(proc_: &P, cmd: &str, cwd: &Path) -> std::io::Result<ProcessOutput> {
    proc_.execute(cmd, &[], cwd)
}

#[test]
fn std_filesystem_writes_and_reads() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("test.txt");
    let fs = opca_core::di::StdFileSystem;

    fs.write(&file_path, b"hello world").unwrap();
    let content = use_filesystem(&fs, &file_path);
    assert_eq!(content, b"hello world");
}

#[test]
fn mock_filesystem_is_injected_and_deterministic() {
    let mock_fs = MockFileSystem::new();
    let path = Path::new("/fake/file.txt");

    mock_fs.insert_file(path, b"deterministic content");

    let result1 = use_filesystem(&mock_fs, path);
    let result2 = use_filesystem(&mock_fs, path);
    assert_eq!(result1, b"deterministic content");
    assert_eq!(result1, result2);
}

#[test]
fn mock_filesystem_returns_empty_for_missing_file() {
    let mock_fs = MockFileSystem::new();
    let result = use_filesystem(&mock_fs, Path::new("/nonexistent"));
    assert!(result.is_empty());
}

#[test]
fn fake_clock_is_deterministic_and_advancable() {
    let clock = FakeClock::new(UNIX_EPOCH);

    let t0 = use_clock(&clock);
    assert_eq!(t0, UNIX_EPOCH);

    clock.advance(Duration::from_secs(10));
    let t1 = use_clock(&clock);
    assert_eq!(t1, UNIX_EPOCH + Duration::from_secs(10));

    clock.advance(Duration::from_secs(5));
    let t2 = use_clock(&clock);
    assert_eq!(t2, UNIX_EPOCH + Duration::from_secs(15));
}

#[test]
fn fake_clock_set_jumps_to_exact_time() {
    let clock = FakeClock::new(UNIX_EPOCH);
    let target = UNIX_EPOCH + Duration::from_secs(1_000_000);
    clock.set(target);
    assert_eq!(use_clock(&clock), target);
}

#[test]
fn fake_random_produces_deterministic_sequence() {
    let random = FakeRandom::new();

    let u1 = use_random(&random);
    let u2 = use_random(&random);
    let u3 = use_random(&random);

    assert_ne!(u1, u2);
    assert_ne!(u2, u3);
    assert_eq!(u1.as_u64_pair().0, 1);
    assert_eq!(u2.as_u64_pair().0, 2);
    assert_eq!(u3.as_u64_pair().0, 3);
}

#[test]
fn fake_random_bytes_are_deterministic() {
    let random = FakeRandom::new();
    let bytes1 = random.random_bytes(8);
    let bytes2 = random.random_bytes(8);

    assert_eq!(bytes1.len(), 8);
    assert_eq!(bytes2.len(), 8);
    assert_ne!(bytes1, bytes2);
}

#[test]
fn mock_process_returns_preset_response() {
    let mock_proc = MockProcess::new();
    mock_proc.set_response(
        "cargo",
        ProcessOutput {
            stdout: "Compiling...".into(),
            stderr: String::new(),
            exit_code: 0,
        },
    );

    let dir = tempdir().unwrap();
    let result = use_process(&mock_proc, "cargo", dir.path()).unwrap();
    assert_eq!(result.stdout, "Compiling...");
    assert!(result.success());
}

#[test]
fn mock_process_returns_error_for_unmocked_command() {
    let mock_proc = MockProcess::new();
    let dir = tempdir().unwrap();
    let result = use_process(&mock_proc, "unknown-cmd", dir.path());
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind(), ErrorKind::NotFound);
}

#[test]
fn mock_filesystem_append_grows_content() {
    let mock_fs = MockFileSystem::new();
    let path = Path::new("/fake/append.txt");

    mock_fs.append(path, b"part1").unwrap();
    mock_fs.append(path, b" part2").unwrap();

    let content = use_filesystem(&mock_fs, path);
    assert_eq!(content, b"part1 part2");
}

#[test]
fn std_filesystem_append_grows_content() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("append.txt");
    let fs = opca_core::di::StdFileSystem;

    fs.append(&path, b"part1").unwrap();
    fs.append(&path, b" part2").unwrap();

    let content = use_filesystem(&fs, &path);
    assert_eq!(content, b"part1 part2");
}

#[test]
fn di_traits_are_object_safe() {
    let fs: Box<dyn FileSystem> = Box::new(MockFileSystem::new());
    let clock: Box<dyn Clock> = Box::new(FakeClock::default());
    let random: Box<dyn Random> = Box::new(FakeRandom::new());

    assert!(!fs.exists(Path::new("/nope")));
    assert_eq!(clock.now(), UNIX_EPOCH);
    assert_eq!(random.uuid().as_u64_pair().0, 1);
}
