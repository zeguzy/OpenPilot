use insta::assert_snapshot;
use opca_core::focus::{FocusContract, build_focus_prompt};

#[test]
fn empty_contract_produces_empty_string() {
    let contract = FocusContract::empty();
    let prompt = build_focus_prompt(&contract);
    assert_snapshot!(prompt);
}

#[test]
fn single_dimension() {
    let contract = FocusContract::new(vec!["security risks".to_string()]);
    let prompt = build_focus_prompt(&contract);
    assert_snapshot!(prompt);
}

#[test]
fn multiple_dimensions() {
    let contract = FocusContract::new(vec![
        "security risks".to_string(),
        "breaking changes".to_string(),
        "performance regression".to_string(),
    ]);
    let prompt = build_focus_prompt(&contract);
    assert_snapshot!(prompt);
}

#[test]
fn max_dimensions() {
    let contract = FocusContract::new(vec![
        "security risks".to_string(),
        "breaking changes".to_string(),
        "performance".to_string(),
        "correctness".to_string(),
        "test coverage".to_string(),
        "code style".to_string(),
        "dependencies".to_string(),
        "error handling".to_string(),
    ]);
    let prompt = build_focus_prompt(&contract);
    assert_snapshot!(prompt);
}
