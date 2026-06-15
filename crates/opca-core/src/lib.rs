//! OPCA Core — Core library for the `OpenPilot` Code Agent.
//!
//! Provides the foundational types, traits, and systems:
//! - Provider abstraction (LLM streaming)
//! - Memory system (Active + Archive + Cold Store)
//! - Task lifecycle state machine
//! - Workspace isolation
//! - Tool registry
//! - Extension system (Context + Capability + Hook)
//! - Orchestrator + Task + Audit agent architecture

#![forbid(unsafe_code)]

pub mod audit;
pub mod completion;
pub mod config;
pub mod continuation;
pub mod di;
pub mod extensions;
pub mod focus;
pub mod lifecycle;
pub mod memory;
pub mod orchestrator;
pub mod provider;
pub mod session;
pub mod task;
pub mod tools;
pub mod workspace;
