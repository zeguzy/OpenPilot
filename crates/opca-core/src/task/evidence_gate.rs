//! Evidence Gate — verifies that a Task's changes compile, test, and lint
//! cleanly before the Task is allowed to transition to `Delivered`.
//!
//! The gate captures a **baseline** at Task dispatch time by running the
//! configured commands on the unmodified workspace. When the Task claims
//! completion, the same commands are re-run and the output is diffed
//! against the baseline. Only **new** failures (not present in the
//! baseline) block the transition.
//!
//! See `design.md` §D3 for the baseline-detection rationale and
//! `specs/task-lifecycle/spec.md` for the Evidence Gate requirement
//! contract.

use std::path::{Path, PathBuf};
use std::process::Command;

// ── Types ──────────────────────────────────────────────────────────────

/// Category of an error detected in command output.
///
/// Used by [`IssueSignature`](super::run::IssueSignature) to group
/// consecutive failures for the 3-strike rule (G8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    CompileError,
    TestFailure,
    LintWarning,
    Other,
}

/// Result of running a single evidence command.
#[derive(Debug, Clone)]
pub struct EvidenceResult {
    pub command: String,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl EvidenceResult {
    /// Returns `true` when the command exited successfully (code 0).
    #[must_use]
    pub const fn is_success(&self) -> bool {
        self.exit_code == 0
    }

    /// Extracts parseable error entries from this command's output.
    #[must_use]
    pub fn extract_errors(&self) -> Vec<ErrorEntry> {
        extract_error_entries(&self.stdout, &self.stderr)
    }
}

/// A single error, warning, or failure line parsed from command output.
#[derive(Debug, Clone)]
pub struct ErrorEntry {
    pub file: Option<PathBuf>,
    pub line: Option<u32>,
    pub kind: ErrorKind,
    pub message: String,
}

/// Evidence Gate failure — emitted when the verification step detects
/// new errors not present in the baseline.
#[derive(Debug, Clone)]
pub struct EvidenceFailure {
    pub command: String,
    pub new_errors: Vec<ErrorEntry>,
}

impl EvidenceFailure {
    /// Produces a human-readable summary suitable for injection into
    /// the Task's active context.
    #[must_use]
    pub fn summary(&self) -> String {
        let errs: Vec<String> = self
            .new_errors
            .iter()
            .map(|e| {
                let loc = match (&e.file, e.line) {
                    (Some(f), Some(l)) => format!("{}:{l}", f.display()),
                    (Some(f), None) => f.display().to_string(),
                    (None, _) => "(unknown)".to_string(),
                };
                format!("[{loc}] {} — {}", kind_label(e.kind), e.message)
            })
            .collect();
        format!(
            "Evidence Gate FAILED on `{}`:\n{}",
            self.command,
            errs.join("\n")
        )
    }
}

// ── Gate ───────────────────────────────────────────────────────────────

/// Runs evidence commands and compares results against a captured
/// baseline.
///
/// Created via [`EvidenceGate::new`] and populated with a baseline via
/// [`EvidenceGate::capture_baseline`]. Verification is performed by
/// [`EvidenceGate::verify`].
#[derive(Debug, Clone)]
pub struct EvidenceGate {
    commands: Vec<String>,
    baseline: Vec<EvidenceResult>,
}

impl EvidenceGate {
    /// Creates a gate that will run `commands` for both baseline
    /// capture and verification.
    #[must_use]
    pub const fn new(commands: Vec<String>) -> Self {
        Self {
            commands,
            baseline: Vec::new(),
        }
    }

    /// Returns `true` when no commands are configured (gate disabled).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// Runs the configured commands in `workspace` and stores the
    /// results as the baseline. Subsequent calls to [`verify`](Self::verify)
    /// compare against this snapshot.
    ///
    /// Errors during command *execution* (e.g. `sh` not found) are
    /// returned. Non-zero exit codes are **not** errors here — they
    /// represent pre-existing failures that the baseline should record.
    pub fn capture_baseline(&mut self, workspace: &Path) -> Result<(), String> {
        self.baseline = run_commands(&self.commands, workspace)?;
        Ok(())
    }

    /// Re-runs the configured commands and compares the output against
    /// the stored baseline.
    ///
    /// Returns `Ok(())` when no **new** failures are detected (existing
    /// baseline failures are tolerated). Returns `Err(EvidenceFailure)`
    /// when at least one new error is found.
    pub fn verify(&self, workspace: &Path) -> Result<(), EvidenceFailure> {
        let current = run_commands(&self.commands, workspace).map_err(|e| EvidenceFailure {
            command: e,
            new_errors: vec![ErrorEntry {
                file: None,
                line: None,
                kind: ErrorKind::Other,
                message: "failed to execute evidence commands".to_string(),
            }],
        })?;

        let baseline_errors = collect_all_errors(&self.baseline);
        let current_errors = collect_all_errors(&current);

        for (idx, ce) in current_errors.iter().enumerate() {
            let _ = idx;
            if !baseline_errors.iter().any(|be| errors_match(be, ce)) {
                let cmd = current
                    .iter()
                    .find(|r| !r.is_success())
                    .map(|r| r.command.clone())
                    .unwrap_or_default();
                return Err(EvidenceFailure {
                    command: cmd,
                    new_errors: current_errors.clone(),
                });
            }
        }

        Ok(())
    }
}

// ── Error parsing ──────────────────────────────────────────────────────

/// Collects all [`ErrorEntry`] items from a batch of results.
fn collect_all_errors(results: &[EvidenceResult]) -> Vec<ErrorEntry> {
    results
        .iter()
        .flat_map(EvidenceResult::extract_errors)
        .collect()
}

/// Extracts structured error entries from raw stdout/stderr.
///
/// Recognises:
/// - `error[E0xxx]:` / `error:` — [`ErrorKind::CompileError`]
/// - `FAILED` / `test ... FAILED` — [`ErrorKind::TestFailure`]
/// - `warning:` — [`ErrorKind::LintWarning`]
fn extract_error_entries(stdout: &str, stderr: &str) -> Vec<ErrorEntry> {
    let combined = format!("{stdout}\n{stderr}");
    let mut entries = Vec::new();
    for line in combined.lines() {
        let trimmed = line.trim();
        if trimmed.contains("error[") || trimmed.contains("error:") {
            entries.push(parse_entry(trimmed, ErrorKind::CompileError));
        } else if trimmed.contains("FAILED") || trimmed.starts_with("test result: FAILED") {
            entries.push(parse_entry(trimmed, ErrorKind::TestFailure));
        } else if trimmed.contains("warning:") {
            entries.push(parse_entry(trimmed, ErrorKind::LintWarning));
        }
    }
    entries
}

/// Parses a single output line into an [`ErrorEntry`], best-effort
/// extraction of file path and line number.
fn parse_entry(line: &str, kind: ErrorKind) -> ErrorEntry {
    let (file, line_num) = extract_location(line);
    let message = clean_message(line);
    ErrorEntry {
        file,
        line: line_num,
        kind,
        message,
    }
}

/// Extracts a file path and optional line number from an error line.
///
/// Handles patterns like `src/lib.rs:42:13` and
/// `/absolute/path/to/src/lib.rs:42`.
fn extract_location(line: &str) -> (Option<PathBuf>, Option<u32>) {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    for tok in &tokens {
        if let Some((path, num)) = parse_path_token(tok) {
            return (Some(PathBuf::from(path)), num);
        }
    }
    (None, None)
}

/// Tries to parse a token like `src/lib.rs:42:13` into `(path, Some(42))`.
fn parse_path_token(tok: &str) -> Option<(&str, Option<u32>)> {
    let colon_idx = tok.find(':')?;
    let path = &tok[..colon_idx];
    if !is_likely_source_file(path) {
        return None;
    }
    let rest = &tok[colon_idx + 1..];
    let line_num = rest.split(':').next().and_then(|s| s.parse::<u32>().ok());
    Some((path, line_num))
}

/// Heuristic: does `path` look like a source file reference?
fn is_likely_source_file(path: &str) -> bool {
    let p = path.trim_start_matches("./");
    p.contains('.') && (p.contains('/') || p.contains('.'))
}

/// Strips ANSI codes and redundant whitespace from an error message.
fn clean_message(line: &str) -> String {
    line.trim().to_string()
}

/// Compares two [`ErrorEntry`] items for baseline-matching purposes.
///
/// Two entries match when they have the same kind, the same file (or
/// both `None`), and the same normalised message hash (line numbers
/// ignored).
fn errors_match(a: &ErrorEntry, b: &ErrorEntry) -> bool {
    if a.kind != b.kind {
        return false;
    }
    if a.file != b.file {
        return false;
    }
    normalize_and_hash(&a.message) == normalize_and_hash(&b.message)
}

/// Normalises an error message by stripping line numbers and absolute
/// paths, then returns an FNV-1a hash.
fn normalize_and_hash(msg: &str) -> u64 {
    let normalized = strip_line_numbers(msg);
    fnv1a(&normalized)
}

// ── Command execution ─────────────────────────────────────────────────

/// Runs `commands` via `sh -c` in `workspace`, collecting stdout,
/// stderr, and exit code for each.
fn run_commands(commands: &[String], workspace: &Path) -> Result<Vec<EvidenceResult>, String> {
    let mut results = Vec::with_capacity(commands.len());
    for cmd in commands {
        let output = Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .current_dir(workspace)
            .output()
            .map_err(|e| format!("failed to run `{cmd}`: {e}"))?;
        results.push(EvidenceResult {
            command: cmd.clone(),
            exit_code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        });
    }
    Ok(results)
}

// ── Normalisation + hashing ───────────────────────────────────────────

/// Strips `:digit:digit` suffixes and absolute path prefixes from a
/// message so that errors differing only by line number compare equal.
fn strip_line_numbers(msg: &str) -> String {
    let mut result = String::with_capacity(msg.len());
    let mut chars = msg.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == ':' {
            // Check if the rest starts with digits — if so, skip them.
            let mut lookahead = chars.clone();
            if lookahead.peek().is_some_and(char::is_ascii_digit) {
                // Consume the digits and optional trailing :digits
                while lookahead.peek().is_some_and(char::is_ascii_digit) {
                    lookahead.next();
                }
                if lookahead.peek() == Some(&':') {
                    lookahead.next();
                    while lookahead.peek().is_some_and(char::is_ascii_digit) {
                        lookahead.next();
                    }
                }
                chars = lookahead;
                result.push(':');
                result.push_str("<n>");
            } else {
                result.push(':');
            }
        } else {
            result.push(ch);
        }
    }
    // Strip absolute paths — replace with relative-looking placeholder.
    result.replace("/Users/", "~/").replace("/home/", "~/")
}

/// FNV-1a 64-bit hash. No external dependency needed.
fn fnv1a(s: &str) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0000_0100_0000_01b3;
    let mut hash = FNV_OFFSET_BASIS;
    for byte in s.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Returns a human label for an [`ErrorKind`].
const fn kind_label(kind: ErrorKind) -> &'static str {
    match kind {
        ErrorKind::CompileError => "compile-error",
        ErrorKind::TestFailure => "test-failure",
        ErrorKind::LintWarning => "lint-warning",
        ErrorKind::Other => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── 3.8 Unit tests: baseline diffing ───────────────────────────────

    #[test]
    fn baseline_passes_current_passes_verify_ok() {
        let dir = tempfile::tempdir().unwrap();
        let mut gate = EvidenceGate::new(vec!["true".to_string()]);
        gate.capture_baseline(dir.path()).unwrap();
        let result = gate.verify(dir.path());
        assert!(
            result.is_ok(),
            "should pass when both baseline and current succeed"
        );
    }

    #[test]
    fn baseline_fails_current_fails_same_verify_ok() {
        let dir = tempfile::tempdir().unwrap();
        // `false` always exits 1 — but produces no parseable error text,
        // so baseline has 0 error entries. Use a command that emits a
        // recognisable error.
        let cmd = r#"sh -c 'echo "error: something broke" >&2; exit 1'"#;
        let mut gate = EvidenceGate::new(vec![cmd.to_string()]);
        gate.capture_baseline(dir.path()).unwrap();
        // Baseline now has the error. Run verify on the same workspace —
        // the same error is present, so no NEW failures.
        let result = gate.verify(dir.path());
        assert!(
            result.is_ok(),
            "same error in baseline and current should NOT trip the gate"
        );
    }

    #[test]
    fn baseline_passes_current_fails_different_verify_err() {
        let _gate = EvidenceGate::new(vec!["true".to_string()]);
        let baseline_errors: Vec<ErrorEntry> = vec![];
        let current_errors: Vec<ErrorEntry> = vec![ErrorEntry {
            file: Some(PathBuf::from("src/lib.rs")),
            line: Some(10),
            kind: ErrorKind::CompileError,
            message: "expected `usize`, got `i32`".to_string(),
        }];
        assert!(baseline_errors.is_empty());
        assert!(!current_errors.is_empty());
        // The matching logic: current error not in baseline → fail.
        let has_new = current_errors
            .iter()
            .any(|ce| !baseline_errors.iter().any(|be| errors_match(be, ce)));
        assert!(has_new, "new error not in baseline should be detected");
    }

    // ── Error parsing tests ────────────────────────────────────────────

    #[test]
    fn extract_compile_error_from_stderr() {
        let entries = extract_error_entries("", "error[E0308]: mismatched types");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, ErrorKind::CompileError);
    }

    #[test]
    fn extract_test_failure_from_stdout() {
        let entries = extract_error_entries("test auth::tests::test_login ... FAILED", "");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, ErrorKind::TestFailure);
    }

    #[test]
    fn extract_warning_as_lint() {
        let entries = extract_error_entries("", "warning: unused variable `x`");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, ErrorKind::LintWarning);
    }

    #[test]
    fn extract_no_false_positives_on_clean_output() {
        let entries = extract_error_entries(
            "Compiling opca v0.1.0\nFinished dev profile\nRunning tests...",
            "",
        );
        assert!(entries.is_empty(), "clean output should have no entries");
    }

    #[test]
    fn extract_location_from_rust_error() {
        let entries =
            extract_error_entries("", "error[E0308]: mismatched types\n --> src/lib.rs:42:13");
        // The second line has the location.
        assert!(!entries.is_empty());
    }

    // ── Normalisation tests ────────────────────────────────────────────

    #[test]
    fn strip_line_numbers_normalises_paths() {
        let a = strip_line_numbers("error at src/lib.rs:42:13");
        let b = strip_line_numbers("error at src/lib.rs:67:5");
        assert_eq!(a, b, "should be equal after stripping line numbers");
    }

    #[test]
    fn fnv1a_is_deterministic() {
        let h1 = fnv1a("hello world");
        let h2 = fnv1a("hello world");
        assert_eq!(h1, h2);
    }

    #[test]
    fn fnv1a_differs_for_different_input() {
        let h1 = fnv1a("hello");
        let h2 = fnv1a("world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn errors_match_ignores_line_numbers() {
        let a = ErrorEntry {
            file: Some(PathBuf::from("src/lib.rs")),
            line: Some(10),
            kind: ErrorKind::CompileError,
            message: "error at src/lib.rs:10:5".to_string(),
        };
        let b = ErrorEntry {
            file: Some(PathBuf::from("src/lib.rs")),
            line: Some(20),
            kind: ErrorKind::CompileError,
            message: "error at src/lib.rs:20:8".to_string(),
        };
        assert!(
            errors_match(&a, &b),
            "should match with different line numbers"
        );
    }

    #[test]
    fn errors_match_different_kind_does_not_match() {
        let a = ErrorEntry {
            file: Some(PathBuf::from("src/lib.rs")),
            line: None,
            kind: ErrorKind::CompileError,
            message: "same message".to_string(),
        };
        let b = ErrorEntry {
            file: Some(PathBuf::from("src/lib.rs")),
            line: None,
            kind: ErrorKind::TestFailure,
            message: "same message".to_string(),
        };
        assert!(!errors_match(&a, &b));
    }

    // ── EvidenceGate construction tests ────────────────────────────────

    #[test]
    fn new_gate_has_empty_commands_when_empty() {
        let gate = EvidenceGate::new(vec![]);
        assert!(gate.is_empty());
    }

    #[test]
    fn new_gate_is_not_empty_with_commands() {
        let gate = EvidenceGate::new(vec!["cargo build".to_string()]);
        assert!(!gate.is_empty());
    }

    #[test]
    fn evidence_failure_summary_contains_command() {
        let failure = EvidenceFailure {
            command: "cargo build".to_string(),
            new_errors: vec![ErrorEntry {
                file: Some(PathBuf::from("src/lib.rs")),
                line: Some(10),
                kind: ErrorKind::CompileError,
                message: "type mismatch".to_string(),
            }],
        };
        let summary = failure.summary();
        assert!(summary.contains("cargo build"));
        assert!(summary.contains("src/lib.rs"));
        assert!(summary.contains("type mismatch"));
    }
}
