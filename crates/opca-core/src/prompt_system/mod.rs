//! Prompt system — centralized registry for all LLM-facing prompt templates.
//!
//! Co-locates the prompt strings used by the Orchestrator, Task agent,
//! Audit Agent, Focus Contract, and Continuation Coordinator so they can
//! be discovered, versioned, and audited in one place.
//!
//! See `openspec/changes/refactor-orchestrator-prompt-system/design.md`
//! §D1 for the rationale behind the hierarchical module layout.
//!
//! # Versioning
//!
//! Each prompt area exposes a `PROMPT_VERSION` constant. Consumers log
//! this at initialization so a given model response can be tied back to
//! the exact prompt template that produced it. Bump the version on any
//! material wording change (whitespace-only edits do not require a bump).

pub mod audit;
pub mod continuation;
pub mod orchestrator;
pub mod task;
