//! Context-Completion Gate — a guard rail that checks whether a user
//! message has enough context to dispatch a Task.
//!
//! Before the Orchestrator dispatches a background Task, it verifies
//! three conditions:
//!
//! 1. The message contains an explicit action verb (implement, add,
//!    fix, refactor, etc.).
//! 2. The scope/objective is concrete enough (basic length heuristic).
//! 3. No blocking specialist consultation is pending.
//!
//! If any condition fails, the Orchestrator asks for clarification
//! instead of dispatching. This is intentionally simple — keyword
//! match + length — not sophisticated NLP.
//!
//! See `design.md` §D6 and `specs/orchestrator-core/spec.md`
//! "Context-Completion Gate" requirement.

const MIN_SCOPE_LENGTH: usize = 10;

const IMPLEMENTATION_VERBS: &[&str] = &[
    "implement",
    "add",
    "create",
    "fix",
    "change",
    "write",
    "update",
    "refactor",
    "build",
    "make",
    "migrate",
    "rewrite",
    "remove",
    "delete",
    "optimize",
    "generate",
    "configure",
    "setup",
    "install",
    "deploy",
];

/// Reason the Context-Completion Gate rejected a dispatch.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DispatchRejection {
    #[error("no implementation verb found in user message")]
    NoImplementationVerb,
    #[error("scope too vague: message needs more detail")]
    ScopeTooVague,
    #[error("a specialist consultation is still pending")]
    SpecialistPending,
}

/// Checks whether a user message has sufficient context to dispatch.
///
/// Returns `Ok(())` if all three gate conditions pass, or
/// `Err(DispatchRejection)` with the first failure.
pub fn can_dispatch(
    user_message: &str,
    pending_specialists: usize,
) -> Result<(), DispatchRejection> {
    if !has_implementation_verb(user_message) {
        return Err(DispatchRejection::NoImplementationVerb);
    }
    if !has_concrete_scope(user_message) {
        return Err(DispatchRejection::ScopeTooVague);
    }
    if pending_specialists > 0 {
        return Err(DispatchRejection::SpecialistPending);
    }
    Ok(())
}

fn has_implementation_verb(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    IMPLEMENTATION_VERBS
        .iter()
        .any(|verb| message_contains_word(&lower, verb))
}

fn message_contains_word(lower_message: &str, word: &str) -> bool {
    lower_message
        .split(|c: char| !c.is_alphanumeric())
        .any(|token| token == word)
}

fn has_concrete_scope(message: &str) -> bool {
    message.trim().chars().count() >= MIN_SCOPE_LENGTH
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Condition 1: implementation verb ────────────────────────────

    #[test]
    fn gate_passes_with_implementation_verb() {
        assert!(can_dispatch("implement JWT auth for the API", 0).is_ok());
    }

    #[test]
    fn gate_fails_without_implementation_verb() {
        let result = can_dispatch("hello there, how are you?", 0);
        assert_eq!(result, Err(DispatchRejection::NoImplementationVerb));
    }

    #[test]
    fn gate_detects_verb_case_insensitive() {
        assert!(can_dispatch("FIX the login bug now", 0).is_ok());
        assert!(can_dispatch("ReFactor the auth module", 0).is_ok());
    }

    #[test]
    fn gate_rejects_question_without_verb() {
        let result = can_dispatch("what does this code do?", 0);
        assert_eq!(result, Err(DispatchRejection::NoImplementationVerb));
    }

    // ── Condition 2: concrete scope ─────────────────────────────────

    #[test]
    fn gate_passes_with_concrete_scope() {
        assert!(can_dispatch("add a new test file for the auth module", 0).is_ok());
    }

    #[test]
    fn gate_fails_with_vague_scope() {
        let result = can_dispatch("fix it", 0);
        assert_eq!(result, Err(DispatchRejection::ScopeTooVague));
    }

    // ── Condition 3: no pending specialist ──────────────────────────

    #[test]
    fn gate_passes_with_no_pending_specialists() {
        assert!(can_dispatch("implement the new feature properly", 0).is_ok());
    }

    #[test]
    fn gate_fails_with_pending_specialist() {
        let result = can_dispatch("implement the new feature properly", 1);
        assert_eq!(result, Err(DispatchRejection::SpecialistPending));
    }

    // ── Ordering: first failure wins ────────────────────────────────

    #[test]
    fn verb_check_precedes_scope_check() {
        let result = can_dispatch("hi", 0);
        assert_eq!(result, Err(DispatchRejection::NoImplementationVerb));
    }

    #[test]
    fn scope_check_precedes_specialist_check() {
        let result = can_dispatch("fix it", 1);
        assert_eq!(result, Err(DispatchRejection::ScopeTooVague));
    }
}
