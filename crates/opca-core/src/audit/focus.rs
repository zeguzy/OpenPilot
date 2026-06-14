use crate::focus::FocusContract;
use crate::workspace::ChangeSet;

const STANDARD_DIMENSIONS: &[&str] = &["compilation", "tests", "diff-sanity"];

#[must_use]
pub fn build_audit_focus(
    task_focus: &FocusContract,
    orchestrator_extras: &[String],
) -> Vec<String> {
    let mut dims: Vec<String> = task_focus.dimensions().to_vec();
    for std_dim in STANDARD_DIMENSIONS {
        if !dims.iter().any(|d| d == std_dim) {
            dims.push((*std_dim).to_string());
        }
    }
    dims.extend(orchestrator_extras.iter().cloned());
    dims
}

#[must_use]
pub fn is_diff_suspicious(diff: &ChangeSet) -> bool {
    !diff.deleted.is_empty()
}
