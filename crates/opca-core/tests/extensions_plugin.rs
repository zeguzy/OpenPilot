//! Integration tests for the Plugin packaging format (Tasks 13.12–13.15).

use std::fs;
use std::path::Path;

use opca_core::extensions::{
    HookDispatcher, HookEvent, InstalledPlugin, PluginManifest, install_plugin,
    install_plugin_with, select_tools_for_task,
};
use tempfile::tempdir;

fn write(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(path, body).unwrap();
}

// ---------------------------------------------------------------------------
// Task 13.12 — plugin.toml manifest parsing
// ---------------------------------------------------------------------------

#[test]
fn manifest_parses_full_plugin_toml() {
    let raw = r#"
name = "docker"
version = "1.2.0"
author = "ops-team"
keywords = ["docker", "container", "image"]

context = "AGENTS.md"
skills = "skills"
mcp = "mcp.json"
hooks = "hooks.toml"
"#;
    let m = PluginManifest::parse_toml(raw).unwrap();
    assert_eq!(m.name, "docker");
    assert_eq!(m.version, "1.2.0");
    assert_eq!(m.author.as_deref(), Some("ops-team"));
    assert_eq!(m.keywords, vec!["docker", "container", "image"]);
    assert_eq!(m.context.as_deref(), Some(Path::new("AGENTS.md")));
    assert_eq!(m.skills.as_deref(), Some(Path::new("skills")));
    assert_eq!(m.mcp.as_deref(), Some(Path::new("mcp.json")));
    assert_eq!(m.hooks.as_deref(), Some(Path::new("hooks.toml")));
}

#[test]
fn manifest_parses_minimal_plugin_toml() {
    let raw = r#"
name = "core"
version = "0.1.0"
"#;
    let m = PluginManifest::parse_toml(raw).unwrap();
    assert_eq!(m.name, "core");
    assert!(m.context.is_none());
    assert!(m.skills.is_none());
    assert!(m.mcp.is_none());
    assert!(m.hooks.is_none());
}

#[test]
fn manifest_rejects_missing_name() {
    let raw = r#"version = "0.1""#;
    assert!(PluginManifest::parse_toml(raw).is_err());
}

#[test]
fn manifest_from_plugin_dir_reads_file() {
    let dir = tempdir().unwrap();
    write(
        &dir.path().join("plugin.toml"),
        r#"
name = "x"
version = "1.0"
"#,
    );
    let m = PluginManifest::from_plugin_dir(dir.path()).unwrap();
    assert_eq!(m.name, "x");
}

#[test]
fn manifest_from_plugin_dir_errors_when_missing() {
    let dir = tempdir().unwrap();
    let result = PluginManifest::from_plugin_dir(dir.path());
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Task 13.13 — plugin install registers context + skills + hooks
// ---------------------------------------------------------------------------

#[test]
fn install_registers_context_skills_and_hooks() {
    let dir = tempdir().unwrap();
    write(
        &dir.path().join("plugin.toml"),
        r#"
name = "demo"
version = "0.1.0"
context = "AGENTS.md"
skills = "skills"
hooks = "hooks.toml"
"#,
    );
    write(
        &dir.path().join("AGENTS.md"),
        "# Demo\n- Use rust 2024\n- Run clippy",
    );
    let skills = dir.path().join("skills");
    fs::create_dir_all(&skills).unwrap();
    write(
        &skills.join("rust.md"),
        "---\nname: rust\ndescription: rust tips\nkeywords: rust\n---\nBe rusty.",
    );
    write(
        &dir.path().join("hooks.toml"),
        r#"
[[hooks]]
event = "pre_tool_use"
matcher = "rm -rf"
timeout_ms = 5000
can_block = true

[hooks.handler]
type = "command"
command = "true"
"#,
    );

    let mut dispatcher = HookDispatcher::empty();
    let installed: InstalledPlugin = install_plugin_with(dir.path(), &mut dispatcher).unwrap();

    assert_eq!(installed.manifest.name, "demo");
    assert!(
        installed
            .context_md
            .as_deref()
            .unwrap_or_default()
            .contains("Use rust 2024")
    );
    assert_eq!(installed.skills.len(), 1);
    assert_eq!(installed.skills[0].name, "rust");
    assert_eq!(installed.hooks_count, 1);
    assert_eq!(dispatcher.len(), 1);
}

#[test]
fn install_succeeds_with_only_manifest() {
    let dir = tempdir().unwrap();
    write(
        &dir.path().join("plugin.toml"),
        r#"
name = "stub"
version = "0.0.1"
"#,
    );

    let installed = install_plugin(dir.path()).unwrap();
    assert_eq!(installed.manifest.name, "stub");
    assert!(installed.context_md.is_none());
    assert!(installed.skills.is_empty());
    assert_eq!(installed.hooks_count, 0);
}

#[test]
fn install_errors_on_malformed_manifest() {
    let dir = tempdir().unwrap();
    write(&dir.path().join("plugin.toml"), "this is not toml {{{");
    let result = install_plugin(dir.path());
    assert!(result.is_err());
}

#[test]
fn install_errors_on_missing_manifest() {
    let dir = tempdir().unwrap();
    let result = install_plugin(dir.path());
    assert!(result.is_err());
}

#[test]
fn install_multiple_hooks_all_register() {
    let dir = tempdir().unwrap();
    write(
        &dir.path().join("plugin.toml"),
        r#"
name = "guarded"
version = "0.1"
hooks = "hooks.toml"
"#,
    );
    write(
        &dir.path().join("hooks.toml"),
        r#"
[[hooks]]
event = "pre_tool_use"
matcher = "rm"
can_block = true
timeout_ms = 1000
[hooks.handler]
type = "command"
command = "true"

[[hooks]]
event = "merge_pre"
can_block = true
timeout_ms = 1000
[hooks.handler]
type = "command"
command = "cargo"
args = ["test"]
"#,
    );

    let mut dispatcher = HookDispatcher::empty();
    let installed = install_plugin_with(dir.path(), &mut dispatcher).unwrap();
    assert_eq!(installed.hooks_count, 2);
    assert_eq!(dispatcher.len(), 2);
}

// ---------------------------------------------------------------------------
// Task 13.14 / 13.15 — per-Task tool activation
// ---------------------------------------------------------------------------

fn manifest_with_keywords(name: &str, keywords: &[&str]) -> PluginManifest {
    PluginManifest {
        name: name.to_string(),
        version: "1".to_string(),
        author: None,
        keywords: keywords
            .iter()
            .map(std::string::ToString::to_string)
            .collect(),
        context: None,
        skills: None,
        mcp: None,
        hooks: None,
    }
}

#[test]
fn docker_plugin_activated_for_containerize_task() {
    // Spec scenario: containerize-the-app Task activates the docker plugin.
    let plugins = vec![
        manifest_with_keywords("docker", &["docker", "container"]),
        manifest_with_keywords("rust", &["rust", "refactor"]),
    ];
    let picked = select_tools_for_task(&plugins, "containerize the app for deployment");
    assert_eq!(picked, vec!["docker".to_string()]);
}

#[test]
fn docker_plugin_not_activated_for_auth_refactor() {
    // Spec scenario: refactor-auth Task does NOT activate the docker plugin.
    let plugins = vec![manifest_with_keywords("docker", &["docker", "container"])];
    let picked = select_tools_for_task(&plugins, "refactor the auth module");
    assert!(
        picked.is_empty(),
        "docker must not activate for auth refactor"
    );
}

#[test]
fn always_on_plugin_activated_for_every_task() {
    let plugins = vec![
        manifest_with_keywords("core", &[]),
        manifest_with_keywords("docker", &["docker"]),
    ];

    // Always-on activates for any task description.
    let picked = select_tools_for_task(&plugins, "refactor auth");
    assert_eq!(picked, vec!["core".to_string()]);

    // And it's still present alongside docker for container tasks.
    let picked = select_tools_for_task(&plugins, "build the docker image");
    assert_eq!(picked.len(), 2);
    assert!(picked.contains(&"core".to_string()));
    assert!(picked.contains(&"docker".to_string()));
}

#[test]
fn keyword_matching_is_case_insensitive() {
    let plugins = vec![manifest_with_keywords("docker", &["docker"])];
    let picked = select_tools_for_task(&plugins, "Build a DOCKER image");
    assert_eq!(picked, vec!["docker".to_string()]);
}

#[test]
fn substring_matching_catches_inflected_forms() {
    // `container` keyword matches `containerize`.
    let plugins = vec![manifest_with_keywords("docker", &["container"])];
    let picked = select_tools_for_task(&plugins, "containerize the app");
    assert_eq!(picked, vec!["docker".to_string()]);
}

#[test]
fn select_returns_empty_when_no_plugins_match() {
    let plugins = vec![manifest_with_keywords("docker", &["docker"])];
    let picked = select_tools_for_task(&plugins, "write documentation");
    assert!(picked.is_empty());
}

#[test]
fn select_returns_empty_for_empty_plugin_list() {
    let picked = select_tools_for_task(&[], "anything");
    assert!(picked.is_empty());
}

// ---------------------------------------------------------------------------
// Hook event coverage smoke test — verify each of the four levels is
// dispatchable via the public API.
// ---------------------------------------------------------------------------

#[test]
fn all_four_hook_levels_are_constructible() {
    let _ = (
        HookEvent::SessionStart,
        HookEvent::PreDispatch,
        HookEvent::PreToolUse,
        HookEvent::AuditReport,
    );
}
