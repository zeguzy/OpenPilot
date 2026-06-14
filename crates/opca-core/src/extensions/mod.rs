//! Extension system — three separated extension points plus Plugin packaging.
//!
//! See `design.md` §D10 for the "three separated extensions + Plugin packaging"
//! rationale and `specs/extension-system/spec.md` for the requirement contracts.
//!
//! # The three extension points
//!
//! 1. **Context** ([`context`]) — pure-Markdown injection into the system prompt.
//!    - [`context::load_agents_md`] — walks from a project root upward looking
//!      for an `AGENTS.md` file (supports `@import` syntax).
//!    - [`context::load_skills`] — loads `skills/*.md` files (YAML frontmatter +
//!      Markdown body) for relevance matching.
//! 2. **Capability** ([`mcp`]) — MCP (Model Context Protocol) servers spawned as
//!    external processes exposing tools over JSON-RPC.
//! 3. **Hook** ([`hooks`]) — lifecycle interception across four levels
//!    (Session/Orchestrator/Task/Audit). Five handler types:
//!    `command`/`http`/`mcp_tool`/`prompt`/`agent`.
//!
//! # Plugin = packaging format
//!
//! A [`plugin::PluginManifest`] bundles a Context (AGENTS.md, skills/),
//! Capability (mcp.json), and Hook (hooks.toml) component into one
//! installable directory. Installation just registers the three extension
//! points — it does not introduce any new mechanisms.

pub mod context;
pub mod hooks;
pub mod mcp;
pub mod plugin;

pub use context::{Skill, load_agents_md, load_skills, select_relevant_skills};
pub use hooks::{HookConfig, HookDispatcher, HookEvent, HookHandler, HookPayload, HookResult};
pub use mcp::{McpClient, McpToolDef};
pub use plugin::{
    InstalledPlugin, PluginManifest, install_plugin, install_plugin_with, select_tools_for_task,
};
