//! Task 17.8 — E2E: plugin install → context + MCP + hooks all active.
//!
//! Creates a temporary plugin directory with `plugin.toml`, `AGENTS.md`, a
//! skill, and `hooks.toml`. Installs the plugin and verifies every component
//! is loaded: context markdown, skills, and hooks registered in the dispatcher.

use opca_core::extensions::{HookDispatcher, InstalledPlugin, install_plugin_with};

fn write_file(dir: &std::path::Path, rel: &str, body: &str) {
    let path = dir.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("mkdir");
    }
    std::fs::write(&path, body).expect("write");
}

#[tokio::test]
#[ignore = "E2E: plugin install with all components"]
async fn e2e_plugin_install_all_components() {
    let dir = tempfile::tempdir().expect("tempdir");

    write_file(
        dir.path(),
        "plugin.toml",
        r#"
name = "e2e-demo"
version = "0.1.0"
author = "e2e"
keywords = ["rust", "refactor"]

context = "AGENTS.md"
skills = "skills"
hooks = "hooks.toml"
"#,
    );

    write_file(
        dir.path(),
        "AGENTS.md",
        "# E2E Demo Plugin\n- Always run clippy\n- Use edition 2024",
    );

    write_file(
        dir.path(),
        "skills/refactor.md",
        "---\nname: refactor\ndescription: Safe refactoring patterns\nkeywords: rust\n---\n# Refactoring\nKeep changes minimal.",
    );

    write_file(
        dir.path(),
        "skills/testing.md",
        "---\nname: testing\ndescription: Test patterns\nkeywords: test\n---\n# Testing\nWrite tests first.",
    );

    write_file(
        dir.path(),
        "hooks.toml",
        r#"
[[hooks]]
event = "pre_tool_use"
matcher = "rm -rf"
timeout_ms = 5000
can_block = true

[hooks.handler]
type = "command"
command = "true"

[[hooks]]
event = "merge_pre"
can_block = true
timeout_ms = 3000

[hooks.handler]
type = "command"
command = "echo"
args = ["merge-ok"]
"#,
    );

    let mut dispatcher = HookDispatcher::empty();
    let installed: InstalledPlugin =
        install_plugin_with(dir.path(), &mut dispatcher).expect("plugin install");

    assert_eq!(installed.manifest.name, "e2e-demo");
    assert_eq!(installed.manifest.version, "0.1.0");

    let context = installed.context_md.as_deref().unwrap_or_default();
    assert!(
        context.contains("Always run clippy"),
        "context should contain AGENTS.md content"
    );

    assert_eq!(installed.skills.len(), 2, "two skills should load");
    assert!(
        installed.skills.iter().any(|s| s.name == "refactor"),
        "refactor skill should be present"
    );
    assert!(
        installed.skills.iter().any(|s| s.name == "testing"),
        "testing skill should be present"
    );

    assert_eq!(installed.hooks_count, 2, "two hooks should register");
    assert_eq!(dispatcher.len(), 2, "dispatcher should have two hooks");
}
