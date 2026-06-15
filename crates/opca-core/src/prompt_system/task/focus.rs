//! Focus Contract prompt section.
//!
//! Moved verbatim from `focus/prompt.rs`; this is the canonical home
//! for the focus prompt builder. The `focus/` module re-exports it for
//! backward compatibility with existing call sites.

use crate::focus::FocusContract;

/// Prompt template version. Bump on any material wording change so
/// consumers can correlate model responses with the exact template.
pub const PROMPT_VERSION: &str = "focus-v1";

/// Builds the Focus Contract section appended to a Task system prompt.
///
/// Returns an empty `String` when the contract has no dimensions, so
/// callers can cheaply detect the no-op case (`task.rs::build_system_prompt`
/// skips the trailing newline join).
#[must_use]
pub fn build_focus_prompt(focus: &FocusContract) -> String {
    if focus.dimensions().is_empty() {
        return String::new();
    }
    let dims = focus
        .dimensions()
        .iter()
        .map(|d| format!("  - [{d}]"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "You have a reporting tool `report_highlight`. \
         You must monitor and report findings on these dimensions:\n{dims}\n\n\
         When calling report_highlight, the `tag` field MUST match one of the dimensions above."
    )
}
