const DEFAULT_CAP: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub struct FocusContract {
    dimensions: Vec<String>,
    cap: usize,
}

impl FocusContract {
    pub const DEFAULT_CAP: usize = DEFAULT_CAP;

    pub const fn empty() -> Self {
        Self {
            dimensions: Vec::new(),
            cap: DEFAULT_CAP,
        }
    }

    pub fn new(dimensions: Vec<String>) -> Self {
        let cap = DEFAULT_CAP;
        debug_assert!(
            dimensions.len() <= cap,
            "FocusContract cap ({cap}) exceeded on construction: {} dimensions provided",
            dimensions.len()
        );
        Self { dimensions, cap }
    }

    #[must_use]
    pub fn dimensions(&self) -> &[String] {
        &self.dimensions
    }

    #[must_use]
    pub fn contains(&self, dimension: &str) -> bool {
        self.dimensions.iter().any(|d| d == dimension)
    }

    #[must_use]
    pub const fn cap(&self) -> usize {
        self.cap
    }

    pub const fn with_cap(mut self, cap: usize) -> Self {
        self.cap = cap;
        self
    }

    pub fn add(&mut self, dimension: &str) -> Result<(), FocusError> {
        if self.contains(dimension) {
            return Err(FocusError::Duplicate(dimension.to_string()));
        }
        if self.dimensions.len() >= self.cap {
            return Err(FocusError::CapExceeded {
                cap: self.cap,
                dimension: dimension.to_string(),
            });
        }
        self.dimensions.push(dimension.to_string());
        Ok(())
    }

    pub fn remove(&mut self, dimension: &str) -> bool {
        let len_before = self.dimensions.len();
        self.dimensions.retain(|d| d != dimension);
        self.dimensions.len() < len_before
    }

    pub fn update(&mut self, add: &[&str], remove: &[&str]) -> Result<(), FocusError> {
        for dim in remove {
            self.remove(dim);
        }
        for dim in add {
            self.add(dim)?;
        }
        Ok(())
    }
}

impl Default for FocusContract {
    fn default() -> Self {
        Self::empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FocusError {
    #[error("focus cap ({cap}) exceeded: cannot add \"{dimension}\"")]
    CapExceeded { cap: usize, dimension: String },
    #[error("dimension \"{0}\" not in focus contract")]
    NotInFocus(String),
    #[error("duplicate dimension \"{0}\"")]
    Duplicate(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_contract_has_no_dimensions() {
        let c = FocusContract::empty();
        assert!(c.dimensions().is_empty());
        assert_eq!(c.cap(), 8);
    }

    #[test]
    fn new_with_valid_dimensions() {
        let c = FocusContract::new(vec!["security".to_string(), "breaking changes".to_string()]);
        assert_eq!(c.dimensions().len(), 2);
        assert!(c.contains("security"));
        assert!(c.contains("breaking changes"));
        assert!(!c.contains("docs"));
    }

    #[test]
    fn add_succeeds_under_cap() {
        let mut c = FocusContract::empty();
        assert!(c.add("alpha").is_ok());
        assert!(c.add("beta").is_ok());
        assert_eq!(c.dimensions().len(), 2);
        assert!(c.contains("alpha"));
        assert!(c.contains("beta"));
    }

    #[test]
    fn add_fails_with_duplicate() {
        let mut c = FocusContract::empty();
        assert!(c.add("alpha").is_ok());
        let err = c.add("alpha").unwrap_err();
        assert_eq!(err, FocusError::Duplicate("alpha".to_string()));
    }

    #[test]
    fn add_fails_at_cap() {
        let mut c = FocusContract::empty();
        for i in 0..8 {
            assert!(c.add(&format!("dim{i}")).is_ok());
        }
        assert_eq!(c.dimensions().len(), 8);
        let err = c.add("overflow").unwrap_err();
        assert_eq!(
            err,
            FocusError::CapExceeded {
                cap: 8,
                dimension: "overflow".to_string()
            }
        );
        assert_eq!(c.dimensions().len(), 8);
    }

    #[test]
    fn remove_returns_true_when_present() {
        let mut c = FocusContract::new(vec!["alpha".to_string(), "beta".to_string()]);
        assert!(c.remove("alpha"));
        assert!(!c.contains("alpha"));
        assert_eq!(c.dimensions().len(), 1);
    }

    #[test]
    fn remove_returns_false_when_absent() {
        let mut c = FocusContract::new(vec!["alpha".to_string()]);
        assert!(!c.remove("missing"));
        assert_eq!(c.dimensions().len(), 1);
    }

    #[test]
    fn update_removes_first_then_adds() {
        let mut c = FocusContract::empty();
        for i in 0..8 {
            c.add(&format!("dim{i}")).unwrap();
        }
        assert_eq!(c.dimensions().len(), 8);
        let result = c.update(&["new_dim"], &["dim0"]);
        assert!(result.is_ok());
        assert!(c.contains("new_dim"));
        assert!(!c.contains("dim0"));
        assert_eq!(c.dimensions().len(), 8);
    }

    #[test]
    fn update_add_only() {
        let mut c = FocusContract::empty();
        assert!(c.update(&["alpha", "beta"], &[]).is_ok());
        assert_eq!(c.dimensions().len(), 2);
    }

    #[test]
    fn update_remove_only() {
        let mut c = FocusContract::new(vec!["alpha".to_string(), "beta".to_string()]);
        assert!(c.update(&[], &["alpha"]).is_ok());
        assert!(!c.contains("alpha"));
        assert_eq!(c.dimensions().len(), 1);
    }

    #[test]
    fn update_fails_when_add_exceeds_cap() {
        let mut c = FocusContract::empty();
        for i in 0..8 {
            c.add(&format!("dim{i}")).unwrap();
        }
        let err = c.update(&["extra"], &[]).unwrap_err();
        assert_eq!(
            err,
            FocusError::CapExceeded {
                cap: 8,
                dimension: "extra".to_string()
            }
        );
    }

    #[test]
    fn update_replacement_at_cap_works() {
        let mut c = FocusContract::empty();
        for i in 0..8 {
            c.add(&format!("dim{i}")).unwrap();
        }
        let result = c.update(&["new1", "new2"], &["dim0", "dim1"]);
        assert!(result.is_ok());
        assert!(c.contains("new1"));
        assert!(c.contains("new2"));
        assert!(!c.contains("dim0"));
        assert!(!c.contains("dim1"));
        assert_eq!(c.dimensions().len(), 8);
    }

    #[test]
    fn with_cap_allows_custom_limit() {
        let mut c = FocusContract::empty().with_cap(2);
        assert!(c.add("a").is_ok());
        assert!(c.add("b").is_ok());
        let err = c.add("c").unwrap_err();
        assert_eq!(
            err,
            FocusError::CapExceeded {
                cap: 2,
                dimension: "c".to_string()
            }
        );
    }

    #[test]
    fn default_is_empty() {
        let c = FocusContract::default();
        assert!(c.dimensions().is_empty());
    }
}
