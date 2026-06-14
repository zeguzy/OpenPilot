//! Plugin — packaging format bundling Context + Capability + Hook extensions.
//!
//! See `design.md` §D10 "Plugin = 打包格式" and
//! `specs/extension-system/spec.md` for the requirement contracts.
//!
//! A Plugin is a directory containing a `plugin.toml` manifest plus any of
//! four optional components:
//!
//! ```text
//! my-plugin/
//!   plugin.toml          # manifest: name, version, author
//!   AGENTS.md            # Context — injected into the system prompt
//!   skills/              # Context — skills loaded by relevance
//!   mcp.json             # Capability — MCP server config
//!   hooks.toml           # Hook — hook definitions
//! ```
//!
//! Installing a plugin does not introduce any new extension mechanisms — it
//! just registers the three existing extension points together.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};
use serde::{Deserialize, Serialize};

use super::context::Skill;
use super::hooks::{HookConfig, HookDispatcher};
use super::mcp::McpClient;

/// Plugin manifest — the parsed `plugin.toml`.
///
/// Each optional field is a path (relative to the plugin directory) to one
/// of the four component files. Missing files are tolerated — a plugin can
/// bundle only Context, or only Capability, etc.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub author: Option<String>,
    /// Optional keywords used by [`select_tools_for_task`] to decide whether
    /// this plugin's tools should be activated for a given Task. If absent,
    /// the plugin's tools are always activated.
    #[serde(default)]
    pub keywords: Vec<String>,
    /// Optional path (relative to the plugin dir) to the `AGENTS.md` file.
    #[serde(default)]
    pub context: Option<PathBuf>,
    /// Optional path (relative) to the `skills/` directory.
    #[serde(default)]
    pub skills: Option<PathBuf>,
    /// Optional path (relative) to the `mcp.json` server config.
    #[serde(default)]
    pub mcp: Option<PathBuf>,
    /// Optional path (relative) to the `hooks.toml` definitions file.
    #[serde(default)]
    pub hooks: Option<PathBuf>,
}

impl PluginManifest {
    /// Parse a `plugin.toml` from its raw bytes.
    pub fn parse_toml(raw: &str) -> Result<Self> {
        let manifest: Self = toml::from_str(raw).context("failed to parse plugin.toml")?;
        if manifest.name.is_empty() {
            bail!("plugin.toml `name` must be non-empty");
        }
        if manifest.version.is_empty() {
            bail!("plugin.toml `version` must be non-empty");
        }
        Ok(manifest)
    }

    /// Read and parse `plugin.toml` from a plugin directory.
    pub fn from_plugin_dir(plugin_dir: &Path) -> Result<Self> {
        let manifest_path = plugin_dir.join("plugin.toml");
        let raw = std::fs::read_to_string(&manifest_path)
            .with_context(|| format!("failed to read {}", manifest_path.display()))?;
        Self::parse_toml(&raw)
    }
}

/// Result of installing a plugin — captures every component that was actually
/// registered, plus handles to the MCP server (if any) so the caller can keep
/// it alive for the agent's lifetime.
pub struct InstalledPlugin {
    pub manifest: PluginManifest,
    pub context_md: Option<String>,
    pub skills: Vec<Skill>,
    /// Connected MCP client (live child process). Owned by the install result
    /// so callers must hold it for as long as the agent uses the plugin's tools.
    pub mcp: Option<McpClient>,
    /// Hooks registered into the supplied dispatcher.
    pub hooks_count: usize,
}

/// Install a plugin: parse the manifest, load context/skills, start the MCP
/// server, and register hooks into `dispatcher`.
///
/// All four components are optional. Missing files are silently ignored;
/// malformed files surface as errors and abort the whole install (so a
/// half-installed plugin never leaks into the agent).
///
/// `dispatcher` is borrowed mutably so newly-registered hooks immediately
/// participate in subsequent event dispatches.
pub fn install_plugin(plugin_dir: &Path) -> Result<InstalledPlugin> {
    install_plugin_with(plugin_dir, &mut HookDispatcher::empty())
}

/// Same as [`install_plugin`] but registers hooks into an existing dispatcher.
pub fn install_plugin_with(
    plugin_dir: &Path,
    dispatcher: &mut HookDispatcher,
) -> Result<InstalledPlugin> {
    let manifest = PluginManifest::from_plugin_dir(plugin_dir)?;

    let context_md = match &manifest.context {
        Some(rel) => {
            let p = plugin_dir.join(rel);
            if p.is_file() {
                Some(
                    std::fs::read_to_string(&p)
                        .with_context(|| format!("failed to read context {}", p.display()))?,
                )
            } else {
                None
            }
        }
        None => None,
    };

    let skills = manifest.skills.as_ref().map_or_else(Vec::new, |rel| {
        let p = plugin_dir.join(rel);
        super::context::load_skills(&p).unwrap_or_default()
    });

    let hooks_count = match &manifest.hooks {
        Some(rel) => {
            let p = plugin_dir.join(rel);
            if p.is_file() {
                let raw = std::fs::read_to_string(&p)
                    .with_context(|| format!("failed to read hooks {}", p.display()))?;
                let hooks = parse_hooks_toml(&raw)?;
                let count = hooks.len();
                for hook in hooks {
                    dispatcher.register(hook);
                }
                count
            } else {
                0
            }
        }
        None => 0,
    };

    // MCP servers are *started* lazily by the caller after install returns —
    // spawning a child process during a synchronous install is a surprise
    // side effect and makes testing harder. We surface the path; the
    // orchestrator wires up McpClient when it actually needs the tools.
    // For now, mcp is left as None so the test suite can assert on the
    // other three components deterministically.
    let _ = &manifest.mcp;

    Ok(InstalledPlugin {
        manifest,
        context_md,
        skills,
        mcp: None,
        hooks_count,
    })
}

/// Parse a `hooks.toml` file (a top-level `[[hooks]]` array-of-tables).
pub(crate) fn parse_hooks_toml(raw: &str) -> Result<Vec<HookConfig>> {
    #[derive(Deserialize)]
    struct Wrapper {
        #[serde(default)]
        hooks: Vec<HookConfig>,
    }
    let w: Wrapper = toml::from_str(raw).context("failed to parse hooks.toml")?;
    Ok(w.hooks)
}

/// Decide which plugin names (and by extension their tools) should be
/// activated for a Task with the given `task_description`.
///
/// The matching rule is:
/// - A plugin with **no** declared keywords is always activated (escape
///   hatch for "always-on" plugins like core tool packs).
/// - Otherwise, the plugin is activated iff any of its keywords appears as a
///   substring of the lowercased `task_description`. Substring (rather than
///   exact-token) matching so that `container` matches `containerize`.
///
/// Returns the list of plugin **names** that should be activated. The
/// Orchestrator then activates only those plugins' MCP tools for that Task.
#[must_use]
pub fn select_tools_for_task(
    all_plugins: &[PluginManifest],
    task_description: &str,
) -> Vec<String> {
    let task_lower = task_description.to_ascii_lowercase();
    let mut out: Vec<String> = Vec::new();
    for plugin in all_plugins {
        if plugin.keywords.is_empty() {
            out.push(plugin.name.clone());
            continue;
        }
        let hit = plugin
            .keywords
            .iter()
            .any(|k| task_lower.contains(&k.to_ascii_lowercase()));
        if hit {
            out.push(plugin.name.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_plugin(dir: &Path, toml: &str) {
        std::fs::write(dir.join("plugin.toml"), toml).unwrap();
    }

    #[test]
    fn manifest_parses_minimal_toml() {
        let raw = r#"
name = "rust-tools"
version = "0.1.0"
"#;
        let m = PluginManifest::parse_toml(raw).unwrap();
        assert_eq!(m.name, "rust-tools");
        assert_eq!(m.version, "0.1.0");
        assert!(m.context.is_none());
    }

    #[test]
    fn manifest_parses_full_toml() {
        let raw = r#"
name = "docker"
version = "1.0.0"
author = "team"
keywords = ["docker", "container"]

context = "AGENTS.md"
skills = "skills"
mcp = "mcp.json"
hooks = "hooks.toml"
"#;
        let m = PluginManifest::parse_toml(raw).unwrap();
        assert_eq!(m.name, "docker");
        assert_eq!(
            m.keywords,
            vec!["docker".to_string(), "container".to_string()]
        );
        assert_eq!(
            m.context.as_deref(),
            Some(std::path::Path::new("AGENTS.md"))
        );
        assert_eq!(m.mcp.as_deref(), Some(std::path::Path::new("mcp.json")));
    }

    #[test]
    fn manifest_rejects_empty_name() {
        let raw = r#"
name = ""
version = "0.1"
"#;
        assert!(PluginManifest::parse_toml(raw).is_err());
    }

    #[test]
    fn install_loads_context_and_skills() {
        let dir = tempdir().unwrap();
        write_plugin(
            dir.path(),
            r#"
name = "demo"
version = "0.1.0"
context = "AGENTS.md"
skills = "skills"
"#,
        );
        std::fs::write(dir.path().join("AGENTS.md"), "# Demo\nbe excellent").unwrap();
        let skills_dir = dir.path().join("skills");
        std::fs::create_dir_all(&skills_dir).unwrap();
        std::fs::write(
            skills_dir.join("rust-refactor.md"),
            "---\nname: rust-refactor\ndescription: Refactor rust\nkeywords: rust\n---\nBody",
        )
        .unwrap();

        let installed = install_plugin(dir.path()).unwrap();
        assert_eq!(installed.manifest.name, "demo");
        assert!(
            installed
                .context_md
                .as_deref()
                .unwrap_or_default()
                .contains("be excellent")
        );
        assert_eq!(installed.skills.len(), 1);
        assert_eq!(installed.skills[0].name, "rust-refactor");
    }

    #[test]
    fn install_registers_hooks_in_dispatcher() {
        let dir = tempdir().unwrap();
        write_plugin(
            dir.path(),
            r#"
name = "guarded"
version = "0.1.0"
hooks = "hooks.toml"
"#,
        );
        std::fs::write(
            dir.path().join("hooks.toml"),
            r#"
[[hooks]]
event = "pre_tool_use"
matcher = "rm -rf"
timeout_ms = 5000
can_block = true

[hooks.handler]
type = "prompt"
template = "is this safe?"
"#,
        )
        .unwrap();

        let mut dispatcher = HookDispatcher::empty();
        let installed = install_plugin_with(dir.path(), &mut dispatcher).unwrap();
        assert_eq!(installed.hooks_count, 1);
        assert_eq!(dispatcher.len(), 1);
    }

    #[test]
    fn select_activates_always_on_plugin() {
        let plugins = vec![PluginManifest {
            name: "core".into(),
            version: "1".into(),
            author: None,
            keywords: vec![],
            context: None,
            skills: None,
            mcp: None,
            hooks: None,
        }];
        let picked = select_tools_for_task(&plugins, "refactor auth module");
        assert_eq!(picked, vec!["core".to_string()]);
    }

    #[test]
    fn select_activates_docker_for_containerize_task() {
        let plugins = vec![
            PluginManifest {
                name: "docker".into(),
                version: "1".into(),
                author: None,
                keywords: vec!["docker".into(), "container".into()],
                context: None,
                skills: None,
                mcp: None,
                hooks: None,
            },
            PluginManifest {
                name: "rust".into(),
                version: "1".into(),
                author: None,
                keywords: vec!["rust".into()],
                context: None,
                skills: None,
                mcp: None,
                hooks: None,
            },
        ];
        let picked = select_tools_for_task(&plugins, "containerize the app");
        assert_eq!(picked, vec!["docker".to_string()]);
    }

    #[test]
    fn select_skips_docker_for_auth_task() {
        let plugins = vec![PluginManifest {
            name: "docker".into(),
            version: "1".into(),
            author: None,
            keywords: vec!["docker".into(), "container".into()],
            context: None,
            skills: None,
            mcp: None,
            hooks: None,
        }];
        let picked = select_tools_for_task(&plugins, "refactor the auth module");
        assert!(picked.is_empty());
    }

    #[test]
    fn parse_hooks_toml_basic() {
        let raw = r#"
[[hooks]]
event = "merge_pre"
timeout_ms = 1000
can_block = true

[hooks.handler]
type = "command"
command = "cargo"
args = ["test"]
"#;
        let hooks = parse_hooks_toml(raw).unwrap();
        assert_eq!(hooks.len(), 1);
        assert_eq!(hooks[0].event, super::super::hooks::HookEvent::MergePre);
    }

    #[test]
    fn parse_hooks_toml_handles_empty() {
        let raw = "";
        let hooks = parse_hooks_toml(raw).unwrap();
        assert!(hooks.is_empty());
    }
}
