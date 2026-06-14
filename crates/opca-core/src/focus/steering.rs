use serde::{Deserialize, Serialize};

use super::{FocusContract, FocusError};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FocusUpdate {
    pub add: Vec<String>,
    pub remove: Vec<String>,
    pub reason: Option<String>,
}

impl FocusUpdate {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            add: Vec::new(),
            remove: Vec::new(),
            reason: None,
        }
    }

    #[must_use]
    pub fn with_add(mut self, add: Vec<String>) -> Self {
        self.add = add;
        self
    }

    #[must_use]
    pub fn with_remove(mut self, remove: Vec<String>) -> Self {
        self.remove = remove;
        self
    }

    #[must_use]
    pub fn with_reason(mut self, reason: &str) -> Self {
        self.reason = Some(reason.to_string());
        self
    }

    pub fn apply(&self, contract: &mut FocusContract) -> Result<(), FocusError> {
        let add: Vec<&str> = self.add.iter().map(String::as_str).collect();
        let remove: Vec<&str> = self.remove.iter().map(String::as_str).collect();
        contract.update(&add, &remove)
    }
}

impl Default for FocusUpdate {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_add_only() {
        let mut contract = FocusContract::empty();
        let update = FocusUpdate::new().with_add(vec!["alpha".to_string(), "beta".to_string()]);
        assert!(update.apply(&mut contract).is_ok());
        assert!(contract.contains("alpha"));
        assert!(contract.contains("beta"));
    }

    #[test]
    fn apply_remove_only() {
        let mut contract = FocusContract::new(vec![
            "alpha".to_string(),
            "beta".to_string(),
            "gamma".to_string(),
        ]);
        let update = FocusUpdate::new().with_remove(vec!["beta".to_string()]);
        assert!(update.apply(&mut contract).is_ok());
        assert!(!contract.contains("beta"));
        assert!(contract.contains("alpha"));
        assert_eq!(contract.dimensions().len(), 2);
    }

    #[test]
    fn apply_add_and_remove() {
        let mut contract = FocusContract::empty();
        for i in 0..8 {
            contract.add(&format!("dim{i}")).unwrap();
        }
        let update = FocusUpdate::new()
            .with_add(vec!["new1".to_string()])
            .with_remove(vec!["dim0".to_string()]);
        assert!(update.apply(&mut contract).is_ok());
        assert!(contract.contains("new1"));
        assert!(!contract.contains("dim0"));
        assert_eq!(contract.dimensions().len(), 8);
    }

    #[test]
    fn apply_replacement_at_cap() {
        let mut contract = FocusContract::empty();
        for i in 0..8 {
            contract.add(&format!("dim{i}")).unwrap();
        }
        let update = FocusUpdate::new()
            .with_remove(vec!["dim0".to_string(), "dim1".to_string()])
            .with_add(vec!["new1".to_string(), "new2".to_string()]);
        assert!(update.apply(&mut contract).is_ok());
        assert!(contract.contains("new1"));
        assert!(contract.contains("new2"));
        assert_eq!(contract.dimensions().len(), 8);
    }

    #[test]
    fn apply_fails_when_cap_exceeded() {
        let mut contract = FocusContract::empty();
        for i in 0..8 {
            contract.add(&format!("dim{i}")).unwrap();
        }
        let update = FocusUpdate::new().with_add(vec!["overflow".to_string()]);
        let err = update.apply(&mut contract).unwrap_err();
        assert_eq!(
            err,
            FocusError::CapExceeded {
                cap: 8,
                dimension: "overflow".to_string()
            }
        );
    }

    #[test]
    fn reason_is_optional() {
        let update = FocusUpdate::new();
        assert!(update.reason.is_none());
    }

    #[test]
    fn reason_is_stored() {
        let update = FocusUpdate::new().with_reason("user requested security focus");
        assert_eq!(
            update.reason.as_deref(),
            Some("user requested security focus")
        );
    }

    #[test]
    fn empty_update_is_noop() {
        let mut contract = FocusContract::new(vec!["alpha".to_string()]);
        let update = FocusUpdate::new();
        assert!(update.apply(&mut contract).is_ok());
        assert_eq!(contract.dimensions().len(), 1);
    }

    #[test]
    fn serde_roundtrip() {
        let update = FocusUpdate::new()
            .with_add(vec!["a".to_string()])
            .with_remove(vec!["b".to_string()])
            .with_reason("test");
        let json = serde_json::to_string(&update).unwrap();
        let deserialized: FocusUpdate = serde_json::from_str(&json).unwrap();
        assert_eq!(update, deserialized);
    }
}
