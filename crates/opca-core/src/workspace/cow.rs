//! Copy-on-Write detection + directory copy (Task 5.11).
//!
//! Detects filesystem `CoW` support and exposes [`copy_dir_cow`] which prefers
//! `CoW` when available (APFS `clonefile` via `cp -c` on macOS, btrfs/xfs
//! reflink via `cp --reflink=auto` on Linux) and falls back to a full
//! recursive copy.

use std::path::Path;
use std::process::Command;

use super::r#trait::WorkspaceError;

/// Result of `CoW` support detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CowSupport {
    /// `CoW` clone succeeded for a probe copy.
    Supported,
    /// `CoW` flag is accepted by `cp` but copy may have been a normal copy
    /// (e.g. Linux `--reflink=auto` on a non-reflink filesystem).
    Maybe,
    /// `CoW` flag unsupported.
    Unsupported,
}

/// Detect `CoW` support for the filesystem hosting `probe_path`.
///
/// `probe_path` should be a directory where the eventual destination will live
/// (e.g. the parent of the target copy).
pub fn detect_cow(probe_path: &Path) -> CowSupport {
    if probe_path.as_os_str().is_empty() || !probe_path.exists() {
        return CowSupport::Unsupported;
    }
    let probe_src = probe_path.join(".opca-cow-probe-src");
    let probe_dst = probe_path.join(".opca-cow-probe-dst");
    // Cleanup any leftovers.
    let _ = std::fs::remove_dir_all(&probe_src);
    let _ = std::fs::remove_dir_all(&probe_dst);

    if std::fs::create_dir(&probe_src).is_err() {
        return CowSupport::Unsupported;
    }
    if std::fs::write(probe_src.join("payload.bin"), b"probe").is_err() {
        let _ = std::fs::remove_dir_all(&probe_src);
        return CowSupport::Unsupported;
    }

    let res = run_cow_copy(&probe_src, &probe_dst);
    let _ = std::fs::remove_dir_all(&probe_src);
    let _ = std::fs::remove_dir_all(&probe_dst);
    res
}

fn run_cow_copy(src: &Path, dst: &Path) -> CowSupport {
    let os = std::env::consts::OS;
    match os {
        "macos" => {
            let out = Command::new("cp")
                .args(["-R", "-c"])
                .arg(src)
                .arg(dst)
                .output();
            match out {
                Ok(o) if o.status.success() => CowSupport::Supported,
                _ => CowSupport::Unsupported,
            }
        }
        "linux" => {
            let out = Command::new("cp")
                .args(["-R", "--reflink=auto"])
                .arg(src)
                .arg(dst)
                .output();
            match out {
                Ok(o) if o.status.success() => CowSupport::Maybe,
                _ => CowSupport::Unsupported,
            }
        }
        _ => CowSupport::Unsupported,
    }
}

/// Copy `src` directory into `dst` (created if missing). Attempts `CoW` first,
/// then falls back to a plain recursive copy.
pub fn copy_dir_cow(src: &Path, dst: &Path) -> Result<(), WorkspaceError> {
    if !src.exists() {
        return Err(WorkspaceError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("source not found: {}", src.display()),
        )));
    }
    let parent = dst
        .parent()
        .ok_or_else(|| WorkspaceError::Other(format!("invalid dst: {}", dst.display())))?;
    let support = if parent.as_os_str().is_empty() {
        CowSupport::Unsupported
    } else {
        detect_cow(parent)
    };
    match support {
        CowSupport::Supported | CowSupport::Maybe => {
            if try_cow_copy(src, dst).is_ok() {
                return Ok(());
            }
            // fall through to full copy
        }
        CowSupport::Unsupported => {}
    }
    full_copy(src, dst)
}

fn try_cow_copy(src: &Path, dst: &Path) -> std::result::Result<(), ()> {
    let os = std::env::consts::OS;
    let args: Vec<&str> = match os {
        "macos" => vec!["-R", "-c"],
        "linux" => vec!["-R", "--reflink=auto"],
        _ => return Err(()),
    };
    let out = Command::new("cp")
        .args(&args)
        .arg(src)
        .arg(dst)
        .output()
        .map_err(|_| ())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(())
    }
}

fn full_copy(src: &Path, dst: &Path) -> Result<(), WorkspaceError> {
    if src.is_file() {
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(src, dst)?;
        return Ok(());
    }
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            full_copy(&from, &to)?;
        } else if from.is_symlink() {
            let target = std::fs::read_link(&from)?;
            let _ = std::os::unix::fs::symlink(target, to);
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_cow_runs_without_panic() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let support = detect_cow(tmp.path());
        // We can't assert a specific value cross-platform, but it should be a
        // valid variant. On macOS APFS it should be Supported.
        match support {
            CowSupport::Supported | CowSupport::Maybe | CowSupport::Unsupported => {}
        }
    }

    #[test]
    fn copy_dir_cow_copies_files() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let src = tmp.path().join("src");
        std::fs::create_dir_all(src.join("nested")).expect("mkdir");
        std::fs::write(src.join("a.txt"), b"hello").expect("write");
        std::fs::write(src.join("nested/b.txt"), b"world").expect("write");

        let dst = tmp.path().join("dst");
        copy_dir_cow(&src, &dst).expect("copy");

        assert_eq!(std::fs::read(dst.join("a.txt")).unwrap(), b"hello");
        assert_eq!(std::fs::read(dst.join("nested/b.txt")).unwrap(), b"world");
    }

    #[test]
    fn copy_dir_cow_missing_src_errors() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let res = copy_dir_cow(&tmp.path().join("missing"), &tmp.path().join("dst"));
        assert!(res.is_err());
    }

    #[test]
    fn copy_dir_cow_handles_single_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let src = tmp.path().join("file.txt");
        std::fs::write(&src, b"data").expect("write");
        let dst = tmp.path().join("out/file.txt");
        copy_dir_cow(&src, &dst).expect("copy");
        assert_eq!(std::fs::read(&dst).unwrap(), b"data");
    }
}
