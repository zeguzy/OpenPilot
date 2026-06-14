use insta::assert_json_snapshot;
use serde_json::json;

#[test]
fn snapshot_convention_works() {
    let sample = json!({
        "task": "refactor auth",
        "status": "on-it",
        "progress": 0.6,
        "summary": "rewriting token_validator.rs"
    });
    assert_json_snapshot!(sample);
}
