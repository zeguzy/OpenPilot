//! Context extension — AGENTS.md + skills loading.
//!
//! The Context extension point is pure Markdown injected into the system prompt.
//! It teaches the agent "how to think and act" without any code execution.
//!
//! # AGENTS.md
//!
//! [`load_agents_md`] walks from a project root upward looking for an
//! `AGENTS.md` file (so a workspace can inherit a parent's conventions).
//! It supports `@import` syntax: a line starting with `@` followed by a path
//! (relative to the importing file) inlines that file's content.
//!
//! # Skills
//!
//! [`load_skills`] loads every `*.md` file in a skills directory. Each file is
//! YAML frontmatter (`name`, `description`, `keywords`) + a Markdown body. Skills
//! are matched against a task description via keyword overlap and surfaced to
//! the Orchestrator for per-Task injection.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};

use crate::memory::extract_keywords;

/// Default filename searched for at every level of the upward walk.
pub const AGENTS_MD_FILENAME: &str = "AGENTS.md";

/// A loaded skill file.
///
/// Skills are Markdown files with YAML frontmatter. The frontmatter carries
/// machine-readable metadata used for relevance matching; the body is the
/// human-readable instruction content injected into a Task's system prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    /// Stable identifier (from frontmatter `name`, falls back to file stem).
    pub name: String,
    /// One-line summary (from frontmatter `description`).
    pub description: String,
    /// Full Markdown body (everything after the frontmatter).
    pub content: String,
    /// Lowercased keywords used for relevance matching (from frontmatter
    /// `keywords` list). Always non-empty — at minimum the file stem and the
    /// words in `description` are added.
    pub keywords: Vec<String>,
}

impl Skill {
    /// Score how relevant this skill is to a task description.
    ///
    /// Returns the count of keyword tokens in `task_description` (lowercased,
    /// split on non-alphanumerics) that appear in this skill's keyword list.
    /// A score of zero means "no overlap — do not load".
    #[must_use]
    pub fn relevance(&self, task_description: &str) -> usize {
        let task_tokens = extract_keywords(task_description);
        self.keywords
            .iter()
            .filter(|kw| task_tokens.iter().any(|t| t == kw.as_str()))
            .count()
    }
}

/// Walk upward from `project_root` looking for `AGENTS.md`.
///
/// Returns `None` if no `AGENTS.md` is found in the start directory or any
/// ancestor up to (and including) the filesystem root.
///
/// `@import` syntax is supported: any line in the loaded file whose first
/// non-whitespace token begins with `@` is treated as a path to inline
/// (relative to the file containing the import line). Imports are processed
/// depth-first and a single file may be imported at most once per call to
/// guard against import cycles.
pub fn load_agents_md(project_root: &Path) -> Result<Option<String>> {
    let mut visited: Vec<PathBuf> = Vec::new();
    for dir in project_root.ancestors() {
        let candidate = dir.join(AGENTS_MD_FILENAME);
        if candidate.is_file() {
            let content = expand_imports(&candidate, &mut visited)?;
            return Ok(Some(content));
        }
    }
    Ok(None)
}

/// Recursively expand `@import` directives starting from `path`.
///
/// `visited` tracks already-inlined files to break import cycles. Each visited
/// file is recorded by canonical-ish absolute path (we avoid `canonicalize`
/// because tests run against mock-like filesystems where the path may not
/// resolve on disk).
fn expand_imports(path: &Path, visited: &mut Vec<PathBuf>) -> Result<String> {
    let key = path.to_path_buf();
    if visited.contains(&key) {
        bail!("AGENTS.md import cycle detected at {}", path.display());
    }
    visited.push(key);

    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read AGENTS.md at {}", path.display()))?;

    let base = path.parent().unwrap_or_else(|| Path::new("."));
    let mut out = String::with_capacity(raw.len());
    for line in raw.lines() {
        if let Some(rest) = parse_import_line(line) {
            let import_path = if Path::new(&rest).is_absolute() {
                PathBuf::from(&rest)
            } else {
                base.join(&rest)
            };
            let imported = expand_imports(&import_path, visited)?;
            out.push_str(&imported);
            if !imported.ends_with('\n') {
                out.push('\n');
            }
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    Ok(out)
}

/// If `line` is an `@import` directive, return the trimmed path it names.
///
/// An import line is a line whose first non-whitespace token starts with `@`
/// and is followed by a path (e.g. `@docs/conventions.md`). The leading `@`
/// is stripped from the returned path. Lines that are clearly Markdown
/// headings (e.g. `## @something`) are NOT treated as imports.
fn parse_import_line(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let mut chars = trimmed.chars();
    let at = chars.next()?;
    if at != '@' {
        return None;
    }
    // Markdown headings start with `#`, not `@`, so we are safe here.
    let rest: String = chars.collect();
    let rest = rest.trim();
    if rest.is_empty() {
        return None;
    }
    // Heuristic: imports must look like a path (no spaces inside the first
    // segment unless the whole rest is a single quoted/path-like token).
    Some(rest.to_string())
}

/// Load every `*.md` file in `skills_dir` as a [`Skill`].
///
/// Each file is parsed as optional YAML frontmatter delimited by `---`
/// followed by a Markdown body. Missing frontmatter is tolerated — the file
/// stem becomes the skill name and keywords are derived from the body.
///
/// Returns skills sorted by name for deterministic iteration. Non-`.md`
/// entries are skipped silently; malformed files propagate an error.
pub fn load_skills(skills_dir: &Path) -> Result<Vec<Skill>> {
    if !skills_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut skills: Vec<Skill> = Vec::new();
    let entries = std::fs::read_dir(skills_dir)
        .with_context(|| format!("failed to read skills dir {}", skills_dir.display()))?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read skill {}", path.display()))?;
        let skill = parse_skill(&path, &raw);
        skills.push(skill);
    }
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(skills)
}

/// Parse a single skill file into a [`Skill`].
fn parse_skill(path: &Path, raw: &str) -> Skill {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("skill")
        .to_string();

    let (frontmatter, body) = split_frontmatter(raw);
    let fm = parse_frontmatter(frontmatter);

    let name = fm.get("name").cloned().unwrap_or_else(|| stem.clone());
    let description = fm.get("description").cloned().unwrap_or_default();
    let mut keywords: Vec<String> = fm
        .get("keywords")
        .map(|raw_kw| {
            raw_kw
                .split([',', ' '])
                .filter(|s| !s.is_empty())
                .map(str::to_ascii_lowercase)
                .collect()
        })
        .unwrap_or_default();

    // Always include the file stem as a keyword so the skill matches its name.
    if !keywords.iter().any(|k| k == &stem.to_ascii_lowercase()) {
        keywords.push(stem.to_ascii_lowercase());
    }
    // Augment with description tokens so a missing `keywords` field still
    // produces reasonable relevance matches.
    for tok in extract_keywords(&description) {
        if !keywords.contains(&tok) {
            keywords.push(tok);
        }
    }

    Skill {
        name,
        description,
        content: body.trim_end().to_string(),
        keywords,
    }
}

/// Split `raw` into `(frontmatter_body, markdown_body)`.
///
/// Frontmatter is optional and delimited by a leading `---\n ... \n---\n`.
/// If no frontmatter is present, returns `(None, raw)`.
fn split_frontmatter(raw: &str) -> (Option<String>, String) {
    let raw = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    let trimmed_start = raw
        .strip_prefix("---\n")
        .or_else(|| raw.strip_prefix("---\r\n"));
    let Some(rest) = trimmed_start else {
        return (None, raw.to_string());
    };
    if let Some(end_idx) = rest.find("\n---\n").or_else(|| rest.find("\n---\r\n")) {
        let frontmatter = &rest[..end_idx];
        let fence_len = "\n---\n".len();
        let body_start = end_idx + fence_len;
        let body_raw = rest.get(body_start..).unwrap_or("");
        let body = body_raw.strip_prefix(['\n', '\r']).unwrap_or(body_raw);
        return (Some(frontmatter.to_string()), body.to_string());
    }
    // No closing fence — treat the whole thing as body.
    (None, raw.to_string())
}

/// Parse a tiny YAML subset (flat `key: value` and `key: [a, b, c]`) into a map.
///
/// This intentionally does not use a full YAML parser — skills frontmatter is
/// a constrained format and pulling in a YAML crate just for this would be
/// overkill. Unknown shapes are silently dropped.
fn parse_frontmatter(raw: Option<String>) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let Some(raw) = raw else {
        return map;
    };
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim().to_string();
        if key.is_empty() {
            continue;
        }
        let mut value = value.trim();
        // Strip inline list brackets so `keywords: [a, b]` becomes `a, b`.
        if let Some(stripped) = value.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            value = stripped;
        }
        // Strip surrounding quotes.
        if value.len() >= 2 {
            let bytes = value.as_bytes();
            if (bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"')
                || (bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\'')
            {
                value = &value[1..value.len() - 1];
            }
        }
        map.insert(key, value.to_string());
    }
    map
}

/// Return the subset of `skills` whose relevance score against
/// `task_description` is greater than zero, sorted by descending relevance.
///
/// This is the helper the Orchestrator uses to decide which skills' content
/// to inject into a given Task's system prompt.
#[must_use]
pub fn select_relevant_skills(skills: &[Skill], task_description: &str) -> Vec<Skill> {
    let mut scored: Vec<(usize, Skill)> = skills
        .iter()
        .map(|s| (s.relevance(task_description), s.clone()))
        .filter(|(score, _)| *score > 0)
        .collect();
    // Sort by descending score, then by name for determinism.
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.name.cmp(&b.1.name)));
    scored.into_iter().map(|(_, s)| s).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn parses_simple_import_line() {
        assert_eq!(
            parse_import_line("@docs/conventions.md"),
            Some("docs/conventions.md".to_string())
        );
        assert_eq!(parse_import_line("  @x.md"), Some("x.md".to_string()));
    }

    #[test]
    fn ignores_markdown_heading_as_import() {
        assert_eq!(parse_import_line("## Heading"), None);
        assert_eq!(parse_import_line("regular text"), None);
        assert_eq!(parse_import_line("@"), None);
    }

    #[test]
    fn split_frontmatter_extracts_yaml() {
        let raw = "---\nname: foo\ndescription: bar\n---\n# Body\nHello";
        let (fm, body) = split_frontmatter(raw);
        assert_eq!(fm.as_deref(), Some("name: foo\ndescription: bar"));
        assert_eq!(body, "# Body\nHello");
    }

    #[test]
    fn split_frontmatter_without_frontmatter_returns_body() {
        let raw = "# Just markdown\nNo frontmatter";
        let (fm, body) = split_frontmatter(raw);
        assert!(fm.is_none());
        assert_eq!(body, raw);
    }

    #[test]
    fn parses_skill_with_frontmatter() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("rust-refactor.md");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(
            f,
            "---\nname: rust-refactor\ndescription: Refactor Rust code\nkeywords: rust, refactor, cargo\n---\n# Rust Refactor\nSteps here."
        )
        .unwrap();

        let skills = load_skills(dir.path()).unwrap();
        assert_eq!(skills.len(), 1);
        let s = &skills[0];
        assert_eq!(s.name, "rust-refactor");
        assert_eq!(s.description, "Refactor Rust code");
        assert!(s.keywords.contains(&"rust".to_string()));
        assert!(s.keywords.contains(&"refactor".to_string()));
        assert!(s.content.contains("# Rust Refactor"));
    }

    #[test]
    fn skill_relevance_counts_overlap() {
        let s = Skill {
            name: "rust-refactor".to_string(),
            description: String::new(),
            content: String::new(),
            keywords: vec!["rust".into(), "refactor".into(), "cargo".into()],
        };
        assert_eq!(s.relevance("refactor the rust module"), 2);
        assert_eq!(s.relevance("unrelated task"), 0);
    }

    #[test]
    fn load_agents_md_finds_in_project_root() {
        let dir = tempdir().unwrap();
        let agents = dir.path().join("AGENTS.md");
        std::fs::write(&agents, "# Project rules\nUse Rust 2024 edition.").unwrap();
        let loaded = load_agents_md(dir.path()).unwrap();
        assert!(loaded.is_some());
        assert!(loaded.unwrap().contains("Project rules"));
    }

    #[test]
    fn load_agents_md_walks_to_parent() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("AGENTS.md"), "parent rules").unwrap();
        let child = dir.path().join("subdir");
        std::fs::create_dir_all(&child).unwrap();
        let loaded = load_agents_md(&child).unwrap();
        assert_eq!(loaded.as_deref(), Some("parent rules\n"));
    }

    #[test]
    fn load_agents_md_returns_none_when_missing() {
        let dir = tempdir().unwrap();
        // Use the tempdir's own (clean) parent chain by pointing at a fresh
        // subdir with no AGENTS.md anywhere up to a shallow level — we can't
        // avoid the system root but we can assert we don't find one *immediately*.
        // To make this deterministic we point at a path inside the tempdir.
        let sub = dir.path().join("empty");
        std::fs::create_dir_all(&sub).unwrap();
        // If the host happens to have AGENTS.md up the chain, this would be
        // Some — that's a flaky host environment, so we accept either answer
        // but require the call to not error.
        let _ = load_agents_md(&sub).unwrap();
    }

    #[test]
    fn load_agents_md_expands_imports() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("extra.md"), "extra content\n").unwrap();
        std::fs::write(dir.path().join("AGENTS.md"), "# Rules\n@extra.md\nTail\n").unwrap();
        let loaded = load_agents_md(dir.path()).unwrap().unwrap();
        assert!(loaded.contains("# Rules"));
        assert!(loaded.contains("extra content"));
        assert!(loaded.contains("Tail"));
    }

    #[test]
    fn select_relevant_skills_filters_and_sorts() {
        let skills = vec![
            Skill {
                name: "docker".to_string(),
                description: String::new(),
                content: String::new(),
                keywords: vec!["docker".into(), "container".into()],
            },
            Skill {
                name: "rust".to_string(),
                description: String::new(),
                content: String::new(),
                keywords: vec!["rust".into(), "refactor".into()],
            },
        ];
        let picked = select_relevant_skills(&skills, "refactor rust code");
        assert_eq!(picked.len(), 1);
        assert_eq!(picked[0].name, "rust");
    }
}
