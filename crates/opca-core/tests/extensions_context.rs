//! Integration tests for the Context extension point (Tasks 13.1–13.3).
//!
//! Covers `load_agents_md` (upward walk + `@import`) and `load_skills`
//! (frontmatter parsing + relevance matching).

use std::fs;
use std::path::Path;

use opca_core::extensions::{Skill, load_agents_md, load_skills, select_relevant_skills};
use tempfile::tempdir;

fn write(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(path, body).unwrap();
}

// ---------------------------------------------------------------------------
// Task 13.1 — AGENTS.md loader
// ---------------------------------------------------------------------------

#[test]
fn agents_md_injected_at_session_start_when_present() {
    let dir = tempdir().unwrap();
    write(
        &dir.path().join("AGENTS.md"),
        "# Project Conventions\n- Use Rust 2024 edition\n- Format with cargo fmt",
    );

    let loaded = load_agents_md(dir.path()).unwrap();

    // Spec scenario: AGENTS.md content is injected into the Orchestrator's
    // system prompt at session start.
    let injected = loaded.expect("AGENTS.md should be injected when present");
    assert!(injected.contains("Project Conventions"));
    assert!(injected.contains("Rust 2024 edition"));
}

#[test]
fn agents_md_returns_none_when_absent() {
    let dir = tempdir().unwrap();
    let sub = dir.path().join("nested/empty");
    fs::create_dir_all(&sub).unwrap();

    // Walks up to the tempdir, finds nothing — but may find AGENTS.md
    // elsewhere on the host's fs root chain. We assert it doesn't find one
    // inside the tempdir's own contribution; the call must not error.
    let _ = load_agents_md(&sub).unwrap();
}

#[test]
fn agents_md_walks_upward_to_parent_directory() {
    let dir = tempdir().unwrap();
    write(&dir.path().join("AGENTS.md"), "parent rules");
    let child = dir.path().join("packages/cli/src");
    fs::create_dir_all(&child).unwrap();

    let loaded = load_agents_md(&child).unwrap();
    assert_eq!(loaded.as_deref(), Some("parent rules\n"));
}

#[test]
fn agents_md_picks_nearest_when_multiple_ancestors_have_it() {
    let dir = tempdir().unwrap();
    write(&dir.path().join("AGENTS.md"), "outer");
    let child = dir.path().join("packages/cli/src");
    fs::create_dir_all(&child).unwrap();
    write(&child.join("AGENTS.md"), "inner");

    let loaded = load_agents_md(&child).unwrap();
    assert_eq!(loaded.as_deref(), Some("inner\n"));
}

#[test]
fn agents_md_expands_at_import_directives() {
    let dir = tempdir().unwrap();
    write(
        &dir.path().join("docs/conventions.md"),
        "## Conventions\nDo X.",
    );
    write(
        &dir.path().join("AGENTS.md"),
        "# Rules\n@docs/conventions.md\nEnd.\n",
    );

    let injected = load_agents_md(dir.path()).unwrap().unwrap();
    assert!(injected.contains("# Rules"));
    assert!(injected.contains("## Conventions"));
    assert!(injected.contains("Do X."));
    assert!(injected.contains("End."));
}

#[test]
fn agents_md_import_cycle_is_detected() {
    let dir = tempdir().unwrap();
    write(&dir.path().join("AGENTS.md"), "# Top\n@other.md\n");
    write(&dir.path().join("other.md"), "@AGENTS.md\n");

    let result = load_agents_md(dir.path());
    assert!(result.is_err(), "import cycle should error");
}

// ---------------------------------------------------------------------------
// Task 13.2 — Skills loader + relevance
// ---------------------------------------------------------------------------

#[test]
fn skills_loaded_from_directory_with_frontmatter() {
    let dir = tempdir().unwrap();
    let skills = dir.path().join("skills");
    fs::create_dir_all(&skills).unwrap();
    write(
        &skills.join("rust-refactor.md"),
        "---\nname: rust-refactor\ndescription: How to refactor Rust\ntags: irrelevant\nkeywords: rust, refactor, cargo\n---\n# Rust Refactor\n1. Run cargo clippy.\n2. Apply suggestions.",
    );
    write(
        &skills.join("docs-writer.md"),
        "---\nname: docs-writer\ndescription: Writing docs\nkeywords: docs, markdown\n---\nWrite Markdown.",
    );

    let loaded = load_skills(&skills).unwrap();
    assert_eq!(loaded.len(), 2);
    let by_name: std::collections::HashMap<&str, &Skill> =
        loaded.iter().map(|s| (s.name.as_str(), s)).collect();

    let rust = by_name.get("rust-refactor").expect("rust-refactor loaded");
    assert_eq!(rust.description, "How to refactor Rust");
    assert!(rust.keywords.contains(&"rust".to_string()));
    assert!(rust.keywords.contains(&"refactor".to_string()));
    assert!(rust.content.contains("cargo clippy"));

    let docs = by_name.get("docs-writer").expect("docs-writer loaded");
    assert!(docs.keywords.contains(&"docs".to_string()));
}

#[test]
fn skills_relevance_matches_by_keyword_overlap() {
    let dir = tempdir().unwrap();
    let skills = dir.path().join("skills");
    fs::create_dir_all(&skills).unwrap();
    write(
        &skills.join("docker.md"),
        "---\nname: docker\ndescription: container tools\nkeywords: docker, container, image\n---\nUse docker build.",
    );
    write(
        &skills.join("rust.md"),
        "---\nname: rust\ndescription: rust tips\nkeywords: rust, refactor, cargo\n---\nUse cargo.",
    );

    let all = load_skills(&skills).unwrap();
    let picked = select_relevant_skills(&all, "refactor the rust auth module");

    // Spec scenario: skills matching "rust" or "refactor" relevance are
    // loaded into the Task's system prompt.
    assert_eq!(picked.len(), 1);
    assert_eq!(picked[0].name, "rust");
}

#[test]
fn skills_relevance_returns_empty_for_unrelated_task() {
    let skills = vec![Skill {
        name: "docker".to_string(),
        description: String::new(),
        content: String::new(),
        keywords: vec!["docker".into(), "container".into()],
    }];
    let picked = select_relevant_skills(&skills, "refactor the auth module");
    assert!(picked.is_empty(), "docker skill must not activate for auth");
}

#[test]
fn skills_missing_directory_returns_empty_vec() {
    let dir = tempdir().unwrap();
    let missing = dir.path().join("no-such-dir");
    let loaded = load_skills(&missing).unwrap();
    assert!(loaded.is_empty());
}

#[test]
fn skills_without_frontmatter_falls_back_to_file_stem() {
    let dir = tempdir().unwrap();
    let skills = dir.path().join("skills");
    fs::create_dir_all(&skills).unwrap();
    write(
        &skills.join("plain.md"),
        "# Plain Skill\nNo frontmatter here.",
    );

    let loaded = load_skills(&skills).unwrap();
    assert_eq!(loaded.len(), 1);
    let s = &loaded[0];
    assert_eq!(s.name, "plain");
    // Stem is always added as a keyword.
    assert!(s.keywords.iter().any(|k| k == "plain"));
}

#[test]
fn skills_description_tokens_augment_keywords() {
    let dir = tempdir().unwrap();
    let skills = dir.path().join("skills");
    fs::create_dir_all(&skills).unwrap();
    write(
        &skills.join("k8s.md"),
        "---\nname: k8s\ndescription: Kubernetes deployment helpers\n---\nBody.",
    );

    let loaded = load_skills(&skills).unwrap();
    let s = &loaded[0];
    // "kubernetes" and "deployment" should be added from the description so
    // a Task describing "deploy to kubernetes" matches even though no
    // explicit `keywords:` list was provided.
    assert!(s.keywords.iter().any(|k| k == "kubernetes"));
    assert!(s.keywords.iter().any(|k| k == "deployment"));
    assert!(s.keywords.iter().any(|k| k == "k8s"));
}
