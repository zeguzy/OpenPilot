//! Continuation prompt seed templates.
//!
//! Co-locates the continuation seed builder and its sanitization helpers
//! so the prompt content lives with the other templates. The coordinator
//! delegates here to keep prompt strings out of business logic.

pub mod retrospective;
