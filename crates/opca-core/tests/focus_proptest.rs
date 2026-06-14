use opca_core::focus::{FocusContract, FocusError};
use proptest::prelude::*;

fn dim_strategy() -> impl Strategy<Value = String> {
    (0u8..10u8).prop_map(|i| format!("dim_{i}"))
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 512,
        ..ProptestConfig::default()
    })]

    #[test]
    fn arbitrary_add_remove_never_exceeds_cap(
        ops in proptest::collection::vec(
            (0u8..10u8, any::<bool>()),
            0..200,
        ),
    ) {
        let mut contract = FocusContract::empty();
        for (idx, is_add) in ops {
            let dim = format!("dim_{idx}");
            if is_add {
                let result = contract.add(&dim);
                if result.is_ok() {
                    prop_assert!(contract.contains(&dim));
                } else {
                    let err = result.unwrap_err();
                    prop_assert!(
                        matches!(err, FocusError::CapExceeded { .. } | FocusError::Duplicate(_)),
                        "unexpected error: {err:?}"
                    );
                }
            } else {
                let _ = contract.remove(&dim);
            }
            prop_assert!(
                contract.dimensions().len() <= 8,
                "cap exceeded: {} dimensions",
                contract.dimensions().len()
            );
            let dims = contract.dimensions();
            let mut sorted = dims.to_vec();
            sorted.sort();
            let original_len = sorted.len();
            sorted.dedup();
            prop_assert_eq!(
                sorted.len(),
                original_len,
                "duplicates found in {:?}",
                dims
            );
        }
    }

    #[test]
    fn arbitrary_update_never_exceeds_cap(
        updates in proptest::collection::vec(
            (
                proptest::collection::vec(0u8..6, 0..4),
                proptest::collection::vec(0u8..6, 0..4),
            ),
            0..100,
        ),
    ) {
        let mut contract = FocusContract::empty();
        for (add_idxs, remove_idxs) in updates {
            let add: Vec<String> = add_idxs.iter().map(|i| format!("d{i}")).collect();
            let remove: Vec<String> = remove_idxs.iter().map(|i| format!("d{i}")).collect();
            let add_refs: Vec<&str> = add.iter().map(String::as_str).collect();
            let remove_refs: Vec<&str> = remove.iter().map(String::as_str).collect();
            let _ = contract.update(&add_refs, &remove_refs);
            prop_assert!(
                contract.dimensions().len() <= 8,
                "cap exceeded after update: {} dimensions",
                contract.dimensions().len()
            );
            let dims = contract.dimensions();
            let mut sorted = dims.to_vec();
            sorted.sort();
            let original_len = sorted.len();
            sorted.dedup();
            prop_assert_eq!(sorted.len(), original_len);
        }
    }

    #[test]
    fn cap_always_enforced(
        dims in proptest::collection::vec(dim_strategy(), 0..20),
    ) {
        let mut contract = FocusContract::empty();
        let mut count = 0usize;
        for dim in &dims {
            if contract.add(dim).is_ok() {
                count += 1;
            }
        }
        prop_assert!(count <= 8);
        prop_assert!(contract.dimensions().len() <= 8);
    }

    #[test]
    fn no_panic_on_rapid_add_remove(
        ops in proptest::collection::vec(
            (dim_strategy(), any::<bool>()),
            0..500,
        ),
    ) {
        let mut contract = FocusContract::empty();
        for (dim, is_add) in ops {
            if is_add {
                let _ = contract.add(&dim);
            } else {
                let _ = contract.remove(&dim);
            }
            prop_assert!(contract.dimensions().len() <= 8);
        }
    }
}
