//! `.agentignore` parser (Task 5.10).
//!
//! Same syntax as `.gitignore`. Paths matching the patterns are excluded from
//! mirror/worktree import. Excluded directories that the runtime still needs
//! read access to (e.g. `node_modules/`, `target/`) are symlinked from the
//! source project after the worktree is created.

use std::path::Path;

/// Compiled `.agentignore` matcher.
#[derive(Debug, Clone, Default)]
pub struct AgentIgnore {
    patterns: Vec<Pattern>,
}

#[derive(Debug, Clone)]
struct Pattern {
    /// True if the pattern ends with `/` (directory only).
    dir_only: bool,
    /// True if the pattern contains a path separator (anchored to root).
    anchored: bool,
    /// True if the pattern starts with `!` (negation).
    negated: bool,
    /// Pattern body split on `/` for path-aware matching.
    segments: Vec<String>,
}

impl AgentIgnore {
    /// Parse a `.agentignore` file from raw text.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let mut patterns = Vec::new();
        for raw_line in text.lines() {
            let line = strip_comment(raw_line).trim_end_matches('\r');
            if line.is_empty() {
                continue;
            }
            if let Some(p) = Pattern::parse(line) {
                patterns.push(p);
            }
        }
        Self { patterns }
    }

    /// Read `.agentignore` from `dir/.agentignore` if present.
    ///
    /// Returns an empty matcher when the file does not exist.
    pub fn load_from_dir(dir: &Path) -> std::io::Result<Self> {
        let path = dir.join(".agentignore");
        match std::fs::read_to_string(&path) {
            Ok(text) => Ok(Self::parse(&text)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(err) => Err(err),
        }
    }

    /// Returns `true` if `rel_path` (relative to the project root) is ignored.
    ///
    /// `rel_path` should use forward slashes regardless of platform.
    pub fn is_ignored(&self, rel_path: &str) -> bool {
        let is_dir = rel_path.ends_with('/');
        let trimmed = rel_path.trim_end_matches('/');
        let segments: Vec<&str> = trimmed.split('/').filter(|s| !s.is_empty()).collect();
        if segments.is_empty() {
            return false;
        }

        let mut ignored = false;
        for pat in &self.patterns {
            if pat.dir_only && !is_dir && segments.len() == 1 {
                // dir-only pattern, but the candidate is a file at top level
                // still allow matches for nested files under the directory
            }
            if pat.matches(&segments, is_dir) {
                ignored = !pat.negated;
            }
        }
        ignored
    }

    /// Returns whether `rel_path` is ignored; treats `rel_path` as a directory
    /// when either `is_dir` is true or any pattern with the same root segment
    /// is dir-only and is a prefix of `rel_path`.
    pub fn is_path_ignored(&self, rel_path: &Path, is_dir: bool) -> bool {
        let normalized = normalize(rel_path);
        if normalized.is_empty() {
            return false;
        }
        // Try as file first.
        if self.is_ignored_file(&normalized) {
            return true;
        }
        // Try progressively longer directory prefixes.
        let segments: Vec<&str> = normalized.split('/').collect();
        for end in 1..=segments.len() {
            let prefix = segments[..end].join("/") + "/";
            if self.is_ignored(&prefix) {
                return true;
            }
        }
        if is_dir {
            return self.is_ignored(&(normalized + "/"));
        }
        false
    }

    fn is_ignored_file(&self, normalized: &str) -> bool {
        self.is_ignored(normalized)
    }

    /// Convenience: iterate `entries` (paths relative to root) and return the
    /// ones that should be **excluded**.
    pub fn excluded<'a, I>(&self, entries: I) -> Vec<String>
    where
        I: IntoIterator<Item = (&'a String, bool)>,
    {
        entries
            .into_iter()
            .filter_map(|(p, is_dir)| {
                if self.is_path_ignored(Path::new(p), is_dir) {
                    Some(p.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    /// Return the list of directory-only patterns (used to know which dirs to
    /// symlink back into the worktree after creation).
    #[must_use]
    pub fn symlink_targets(&self) -> Vec<String> {
        self.patterns
            .iter()
            .filter(|p| p.dir_only)
            .map(|p| p.segments.first().cloned().unwrap_or_default())
            .filter(|s| !s.is_empty())
            .collect()
    }
}

impl Pattern {
    fn parse(line: &str) -> Option<Self> {
        let mut src = line;
        let mut negated = false;
        if let Some(rest) = src.strip_prefix('!') {
            negated = true;
            src = rest;
        }
        let dir_only = src.ends_with('/');
        if dir_only {
            src = src.trim_end_matches('/');
        }
        // Trim leading slashes → anchored by definition.
        let mut anchored = false;
        if let Some(rest) = src.strip_prefix('/') {
            anchored = true;
            src = rest;
        }
        if src.is_empty() {
            return None;
        }
        if src.contains('/') {
            anchored = true;
        }
        let segments: Vec<String> = src.split('/').map(str::to_string).collect();
        Some(Self {
            dir_only,
            anchored,
            negated,
            segments,
        })
    }

    fn matches(&self, segments: &[&str], is_dir: bool) -> bool {
        if self.dir_only && !is_dir && segments.len() <= self.segments.len() {
            // dir-only patterns only match directories or their descendants
            return false;
        }
        if self.anchored {
            // Match from the root.
            match_segments(&self.segments, segments)
        } else {
            // Match any suffix of the path whose last segment matches the
            // single-segment pattern.
            if self.segments.len() == 1 {
                let pat = &self.segments[0];
                segments.iter().any(|s| glob_match(pat, s))
            } else {
                // For multi-segment non-anchored patterns, attempt to match
                // starting at every position.
                (0..=segments.len().saturating_sub(self.segments.len()))
                    .any(|start| match_segments(&self.segments, &segments[start..]))
            }
        }
    }
}

fn match_segments(pat: &[String], candidate: &[&str]) -> bool {
    if pat.len() != candidate.len() {
        return false;
    }
    pat.iter()
        .zip(candidate.iter())
        .all(|(p, c)| glob_match(p, c))
}

/// Glob match supporting `*` (within a segment) and `?`.
fn glob_match(pattern: &str, candidate: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let c: Vec<char> = candidate.chars().collect();
    glob_match_inner(&p, 0, &c, 0)
}

fn glob_match_inner(p: &[char], mut pi: usize, c: &[char], mut ci: usize) -> bool {
    while pi < p.len() {
        match p[pi] {
            '*' => {
                // Try to match the rest of the pattern at every remaining
                // position of the candidate.
                if pi == p.len() - 1 {
                    return true;
                }
                while ci <= c.len() {
                    if glob_match_inner(p, pi + 1, c, ci) {
                        return true;
                    }
                    ci += 1;
                }
                return false;
            }
            '?' => {
                if ci >= c.len() {
                    return false;
                }
                ci += 1;
                pi += 1;
            }
            ch => {
                if ci >= c.len() || c[ci] != ch {
                    return false;
                }
                ci += 1;
                pi += 1;
            }
        }
    }
    ci == c.len()
}

fn strip_comment(line: &str) -> &str {
    // gitignore: backslash-escaped `#` is literal. We keep it simple: an
    // unescaped `#` at the start of a line begins a comment.
    if let Some(rest) = line.strip_prefix('\\') {
        if rest.starts_with('#') {
            // Escaped hash → literal '#' pattern.
            return rest;
        }
    }
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') {
        return "";
    }
    line
}

fn normalize(p: &Path) -> String {
    let mut buf = String::new();
    let mut first = true;
    for comp in p.components() {
        if let std::path::Component::Normal(s) = comp {
            if !first {
                buf.push('/');
            }
            buf.push_str(&s.to_string_lossy());
            first = false;
        }
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_dir_patterns() {
        let ai = AgentIgnore::parse("node_modules/\ntarget/\ndist/\n");
        assert!(ai.is_path_ignored(Path::new("node_modules"), true));
        assert!(ai.is_path_ignored(Path::new("node_modules/react/index.js"), false));
        assert!(ai.is_path_ignored(Path::new("target/debug/app"), false));
        assert!(!ai.is_path_ignored(Path::new("src/main.rs"), false));
    }

    #[test]
    fn handles_comments_and_blanks() {
        let ai = AgentIgnore::parse("# a comment\n\n  \n*.log\n");
        assert!(ai.is_path_ignored(Path::new("debug.log"), false));
        assert!(ai.is_path_ignored(Path::new("a/b/c.log"), false));
        assert!(!ai.is_path_ignored(Path::new("src/main.rs"), false));
    }

    #[test]
    fn negation_un_ignores() {
        let ai = AgentIgnore::parse("*.log\n!important.log\n");
        assert!(ai.is_path_ignored(Path::new("debug.log"), false));
        assert!(!ai.is_path_ignored(Path::new("important.log"), false));
    }

    #[test]
    fn anchored_patterns_only_match_root() {
        let ai = AgentIgnore::parse("/build\n");
        assert!(ai.is_path_ignored(Path::new("build/out"), false));
        assert!(!ai.is_path_ignored(Path::new("src/build/out"), false));
    }

    #[test]
    fn symlink_targets_lists_dir_patterns() {
        let ai = AgentIgnore::parse("node_modules/\ntarget/\n*.log\n");
        let mut targets = ai.symlink_targets();
        targets.sort();
        assert_eq!(
            targets,
            vec!["node_modules".to_string(), "target".to_string()]
        );
    }

    #[test]
    fn glob_star_matches_within_segment() {
        assert!(glob_match("*.rs", "main.rs"));
        assert!(!glob_match("*.rs", "main.rs.txt"));
        assert!(glob_match("a*b", "axxxb"));
        assert!(glob_match("a?c", "abc"));
        assert!(!glob_match("a?c", "ac"));
    }

    #[test]
    fn load_from_dir_returns_empty_when_missing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let ai = AgentIgnore::load_from_dir(tmp.path()).expect("load");
        assert!(!ai.is_path_ignored(Path::new("anything"), false));
    }

    #[test]
    fn load_from_dir_reads_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join(".agentignore"), "target/\n").expect("write");
        let ai = AgentIgnore::load_from_dir(tmp.path()).expect("load");
        assert!(ai.is_path_ignored(Path::new("target/debug"), false));
    }
}
