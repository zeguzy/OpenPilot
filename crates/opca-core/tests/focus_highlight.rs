use opca_core::focus::{FocusContract, FocusError, Highlight, ReportHighlightTool, Severity};

fn contract_with(dims: &[&str]) -> FocusContract {
    let mut c = FocusContract::empty();
    for d in dims {
        c.add(d).unwrap();
    }
    c
}

#[test]
fn valid_tag_passes_validation() {
    let contract = contract_with(&["security risks", "breaking changes"]);
    let hl = Highlight::new(
        "security risks",
        Severity::Warning,
        "hardcoded client_secret",
    );
    assert!(hl.validate(&contract).is_ok());
}

#[test]
fn invalid_tag_rejected_with_not_in_focus() {
    let contract = contract_with(&["security risks", "breaking changes"]);
    let hl = Highlight::new("documentation", Severity::Info, "missing README");
    let err = hl.validate(&contract).unwrap_err();
    assert_eq!(err, FocusError::NotInFocus("documentation".to_string()));
}

#[test]
fn severity_info_works() {
    let contract = contract_with(&["performance"]);
    let hl = Highlight::new("performance", Severity::Info, "baseline measured at 50ms");
    assert!(hl.validate(&contract).is_ok());
}

#[test]
fn severity_warning_works() {
    let contract = contract_with(&["performance"]);
    let hl = Highlight::new(
        "performance",
        Severity::Warning,
        "p99 latency spike detected",
    );
    assert!(hl.validate(&contract).is_ok());
}

#[test]
fn severity_blocking_works() {
    let contract = contract_with(&["security risks"]);
    let hl = Highlight::new(
        "security risks",
        Severity::Blocking,
        "SQL injection vulnerability in user input",
    );
    assert!(hl.validate(&contract).is_ok());
}

#[test]
fn detail_is_optional() {
    let hl = Highlight::new("security", Severity::Info, "found issue");
    assert!(hl.detail.is_none());

    let hl_with_detail = hl.with_detail("see line 42 of auth.rs");
    assert_eq!(
        hl_with_detail.detail.as_deref(),
        Some("see line 42 of auth.rs")
    );
}

#[test]
fn report_highlight_tool_name() {
    assert_eq!(ReportHighlightTool::name(), "report_highlight");
}

#[test]
fn report_highlight_tool_description() {
    let desc = ReportHighlightTool::description();
    assert!(desc.contains("finding"));
    assert!(desc.contains("focus contract"));
}

#[test]
fn report_highlight_tool_schema() {
    let schema = ReportHighlightTool::parameters_schema();
    assert_eq!(schema["type"], "object");
    assert_eq!(schema["properties"]["tag"]["type"], "string");
    assert_eq!(schema["properties"]["severity"]["type"], "string");
    assert!(
        schema["properties"]["severity"]["enum"]
            .as_array()
            .is_some()
    );
    assert_eq!(schema["properties"]["summary"]["type"], "string");
    assert_eq!(schema["properties"]["detail"]["type"], "string");
    let required = schema["required"].as_array().unwrap();
    assert!(required.contains(&serde_json::json!("tag")));
    assert!(required.contains(&serde_json::json!("severity")));
    assert!(required.contains(&serde_json::json!("summary")));
}

#[test]
fn highlight_serde_roundtrip() {
    let hl = Highlight::new("security risks", Severity::Blocking, "critical issue")
        .with_detail("more context here");
    let json = serde_json::to_string(&hl).unwrap();
    let deserialized: Highlight = serde_json::from_str(&json).unwrap();
    assert_eq!(hl, deserialized);
}

#[test]
fn severity_serde_lowercase() {
    let json = serde_json::to_string(&Severity::Warning).unwrap();
    assert_eq!(json, "\"warning\"");

    let info: Severity = serde_json::from_str("\"info\"").unwrap();
    assert_eq!(info, Severity::Info);

    let blocking: Severity = serde_json::from_str("\"blocking\"").unwrap();
    assert_eq!(blocking, Severity::Blocking);
}

#[test]
fn validation_empty_contract_rejects_all() {
    let contract = FocusContract::empty();
    let hl = Highlight::new("anything", Severity::Info, "test");
    assert!(matches!(
        hl.validate(&contract),
        Err(FocusError::NotInFocus(_))
    ));
}
