use super::FocusContract;

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
