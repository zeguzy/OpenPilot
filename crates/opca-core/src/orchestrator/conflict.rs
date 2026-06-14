use std::path::PathBuf;

#[must_use]
pub fn predict_conflict(existing: &[PathBuf], new: &[PathBuf]) -> bool {
    !existing.is_empty() && new.iter().any(|p| existing.iter().any(|e| e == p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_existing_no_conflict() {
        assert!(!predict_conflict(&[], &[PathBuf::from("src/auth.rs")]));
    }

    #[test]
    fn empty_new_no_conflict() {
        assert!(!predict_conflict(&[PathBuf::from("src/auth.rs")], &[]));
    }

    #[test]
    fn overlapping_files_conflict() {
        assert!(predict_conflict(
            &[PathBuf::from("src/auth.rs")],
            &[PathBuf::from("src/auth.rs")]
        ));
    }

    #[test]
    fn non_overlapping_no_conflict() {
        assert!(!predict_conflict(
            &[PathBuf::from("src/auth.rs")],
            &[PathBuf::from("src/utils.rs")]
        ));
    }

    #[test]
    fn partial_overlap_conflict() {
        assert!(predict_conflict(
            &[PathBuf::from("src/auth.rs"), PathBuf::from("src/utils.rs"),],
            &[PathBuf::from("src/utils.rs"), PathBuf::from("src/main.rs"),]
        ));
    }
}
