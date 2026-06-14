use std::path::PathBuf;
use std::sync::Arc;

use opca_core::di::FileSystem;
use opca_core::di::ProcessOutput;
use opca_core::focus::{FocusContract, Highlight, Severity};
use opca_core::provider::ToolEffects;
use opca_core::tools::builtin::{
    BashTool, EditTool, FindTool, GrepTool, LsTool, ReadTool, ReportHighlightTool, WriteTool,
};
use opca_core::tools::{Tool, ToolContext, ToolRegistry};
use opca_test_utils::{MockFileSystem, MockProcess};
use serde_json::json;
use tokio::sync::mpsc;

fn ctx_from(fs: MockFileSystem, proc: MockProcess) -> ToolContext {
    ToolContext {
        workspace_path: PathBuf::from("/workspace"),
        fs: Arc::new(fs),
        proc: Arc::new(proc),
    }
}

#[tokio::test]
async fn read_tool_reads_file_content() {
    let fs = MockFileSystem::new();
    fs.insert_file("/workspace/src/main.rs", b"fn main() {}");
    let ctx = ctx_from(fs, MockProcess::new());

    let tool = ReadTool;
    let args = json!({"path": "src/main.rs"});
    let result = tool.execute(&args, &ctx).await.unwrap();
    assert!(!result.is_error);
    assert_eq!(result.content, "fn main() {}");
}

#[tokio::test]
async fn read_tool_effects_is_read() {
    let tool = ReadTool;
    assert_eq!(tool.effects(), ToolEffects::Read);
}

#[tokio::test]
async fn read_tool_returns_error_for_missing_file() {
    let ctx = ctx_from(MockFileSystem::new(), MockProcess::new());
    let tool = ReadTool;
    let args = json!({"path": "missing.rs"});
    let result = tool.execute(&args, &ctx).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn read_tool_schema_is_valid() {
    let tool = ReadTool;
    let schema = tool.parameters_schema();
    assert_eq!(schema["type"], "object");
    assert_eq!(schema["properties"]["path"]["type"], "string");
    let required = schema["required"].as_array().unwrap();
    assert!(required.contains(&json!("path")));
}

#[tokio::test]
async fn write_tool_writes_file_content() {
    let fs = MockFileSystem::new();
    let ctx = ctx_from(fs.clone(), MockProcess::new());

    let tool = WriteTool;
    let args = json!({"path": "new.rs", "content": "fn new() {}"});
    let result = tool.execute(&args, &ctx).await.unwrap();
    assert!(!result.is_error);
    assert!(result.content.contains("wrote"));

    let written = fs.read(std::path::Path::new("/workspace/new.rs")).unwrap();
    assert_eq!(written, b"fn new() {}");
}

#[tokio::test]
async fn write_tool_effects_is_write() {
    let tool = WriteTool;
    assert_eq!(tool.effects(), ToolEffects::Write);
}

#[tokio::test]
async fn write_tool_overwrites_existing_file() {
    let fs = MockFileSystem::new();
    fs.insert_file("/workspace/old.rs", b"old content");
    let ctx = ctx_from(fs.clone(), MockProcess::new());

    let tool = WriteTool;
    let args = json!({"path": "old.rs", "content": "new content"});
    tool.execute(&args, &ctx).await.unwrap();

    let written = fs.read(std::path::Path::new("/workspace/old.rs")).unwrap();
    assert_eq!(written, b"new content");
}

#[tokio::test]
async fn edit_tool_replaces_text() {
    let fs = MockFileSystem::new();
    fs.insert_file("/workspace/main.rs", b"fn old_name() {}");
    let ctx = ctx_from(fs.clone(), MockProcess::new());

    let tool = EditTool;
    let args = json!({
        "path": "main.rs",
        "old_text": "old_name",
        "new_text": "new_name"
    });
    let result = tool.execute(&args, &ctx).await.unwrap();
    assert!(!result.is_error);

    let written = fs.read(std::path::Path::new("/workspace/main.rs")).unwrap();
    assert_eq!(written, b"fn new_name() {}");
}

#[tokio::test]
async fn edit_tool_effects_is_write() {
    let tool = EditTool;
    assert_eq!(tool.effects(), ToolEffects::Write);
}

#[tokio::test]
async fn edit_tool_returns_error_when_text_not_found() {
    let fs = MockFileSystem::new();
    fs.insert_file("/workspace/main.rs", b"fn foo() {}");
    let ctx = ctx_from(fs, MockProcess::new());

    let tool = EditTool;
    let args = json!({
        "path": "main.rs",
        "old_text": "missing",
        "new_text": "x"
    });
    let result = tool.execute(&args, &ctx).await.unwrap();
    assert!(result.is_error);
    assert!(result.content.contains("not found"));
}

#[tokio::test]
async fn bash_tool_runs_command_via_process() {
    let proc = MockProcess::new();
    proc.set_response(
        "cargo",
        ProcessOutput {
            stdout: "Compiling opca-core\nFinished".to_string(),
            stderr: String::new(),
            exit_code: 0,
        },
    );
    let ctx = ctx_from(MockFileSystem::new(), proc);

    let tool = BashTool;
    let args = json!({"command": "cargo build", "cwd": "."});
    let result = tool.execute(&args, &ctx).await.unwrap();
    assert!(!result.is_error);
    assert!(result.content.contains("Compiling"));
}

#[tokio::test]
async fn bash_tool_effects_is_process() {
    let tool = BashTool;
    assert_eq!(tool.effects(), ToolEffects::Process);
}

#[tokio::test]
async fn bash_tool_reports_failure_with_exit_code() {
    let proc = MockProcess::new();
    proc.set_response(
        "cargo",
        ProcessOutput {
            stdout: String::new(),
            stderr: "compile error".to_string(),
            exit_code: 101,
        },
    );
    let ctx = ctx_from(MockFileSystem::new(), proc);

    let tool = BashTool;
    let args = json!({"command": "cargo build"});
    let result = tool.execute(&args, &ctx).await.unwrap();
    assert!(result.is_error);
    assert!(result.content.contains("101"));
    assert!(result.content.contains("compile error"));
}

#[tokio::test]
async fn grep_tool_finds_pattern_in_single_file() {
    let fs = MockFileSystem::new();
    fs.insert_file("/workspace/main.rs", b"fn main() {}\nfn other() {}\n");
    let ctx = ctx_from(fs, MockProcess::new());

    let tool = GrepTool;
    let args = json!({"pattern": "fn main", "path": "main.rs"});
    let result = tool.execute(&args, &ctx).await.unwrap();
    assert!(!result.is_error);
    assert!(result.content.contains("main.rs:1"));
    assert!(result.content.contains("fn main()"));
    assert!(!result.content.contains("fn other"));
}

#[tokio::test]
async fn grep_tool_effects_is_read() {
    let tool = GrepTool;
    assert_eq!(tool.effects(), ToolEffects::Read);
}

#[tokio::test]
async fn grep_tool_searches_directory_with_include_filter() {
    let fs = MockFileSystem::new();
    fs.insert_file("/workspace/src/a.rs", b"fn alpha() {}\n");
    fs.insert_file("/workspace/src/b.rs", b"fn beta() {}\n");
    fs.insert_file("/workspace/src/c.txt", b"fn gamma() {}\n");
    fs.create_dir_all(std::path::Path::new("/workspace/src"))
        .unwrap();
    let ctx = ctx_from(fs, MockProcess::new());

    let tool = GrepTool;
    let args = json!({"pattern": "fn", "path": "src", "include": "*.rs"});
    let result = tool.execute(&args, &ctx).await.unwrap();
    assert!(!result.is_error);
    assert!(result.content.contains("a.rs"));
    assert!(result.content.contains("b.rs"));
    assert!(!result.content.contains("c.txt"));
}

#[tokio::test]
async fn grep_tool_returns_no_matches_when_pattern_absent() {
    let fs = MockFileSystem::new();
    fs.insert_file("/workspace/main.rs", b"fn main() {}\n");
    let ctx = ctx_from(fs, MockProcess::new());

    let tool = GrepTool;
    let args = json!({"pattern": "nonexistent", "path": "main.rs"});
    let result = tool.execute(&args, &ctx).await.unwrap();
    assert!(!result.is_error);
    assert_eq!(result.content, "no matches");
}

#[tokio::test]
async fn find_tool_finds_files_matching_pattern() {
    let fs = MockFileSystem::new();
    fs.insert_file("/workspace/src/a.rs", b"");
    fs.insert_file("/workspace/src/b.rs", b"");
    fs.insert_file("/workspace/src/c.txt", b"");
    fs.create_dir_all(std::path::Path::new("/workspace/src"))
        .unwrap();
    let ctx = ctx_from(fs, MockProcess::new());

    let tool = FindTool;
    let args = json!({"pattern": "*.rs", "path": "src"});
    let result = tool.execute(&args, &ctx).await.unwrap();
    assert!(!result.is_error);
    assert!(result.content.contains("a.rs"));
    assert!(result.content.contains("b.rs"));
    assert!(!result.content.contains("c.txt"));
}

#[tokio::test]
async fn find_tool_effects_is_read() {
    let tool = FindTool;
    assert_eq!(tool.effects(), ToolEffects::Read);
}

#[tokio::test]
async fn find_tool_returns_no_files_when_nothing_matches() {
    let fs = MockFileSystem::new();
    fs.insert_file("/workspace/a.txt", b"");
    let ctx = ctx_from(fs, MockProcess::new());

    let tool = FindTool;
    let args = json!({"pattern": "*.rs"});
    let result = tool.execute(&args, &ctx).await.unwrap();
    assert_eq!(result.content, "no files matched");
}

#[tokio::test]
async fn ls_tool_lists_directory_contents() {
    let fs = MockFileSystem::new();
    fs.insert_file("/workspace/src/main.rs", b"");
    fs.insert_file("/workspace/src/lib.rs", b"");
    let ctx = ctx_from(fs, MockProcess::new());

    let tool = LsTool;
    let args = json!({"path": "src"});
    let result = tool.execute(&args, &ctx).await.unwrap();
    assert!(!result.is_error);
    assert!(result.content.contains("main.rs"));
    assert!(result.content.contains("lib.rs"));
}

#[tokio::test]
async fn ls_tool_effects_is_read() {
    let tool = LsTool;
    assert_eq!(tool.effects(), ToolEffects::Read);
}

#[tokio::test]
async fn ls_tool_default_path_is_workspace() {
    let fs = MockFileSystem::new();
    fs.insert_file("/workspace/foo.rs", b"");
    let ctx = ctx_from(fs, MockProcess::new());

    let tool = LsTool;
    let args = json!({});
    let result = tool.execute(&args, &ctx).await.unwrap();
    assert!(result.content.contains("foo.rs"));
}

#[tokio::test]
async fn report_highlight_pushes_valid_highlight_to_channel() {
    let mut focus = FocusContract::empty();
    focus.add("security risks").unwrap();
    let focus = Arc::new(focus);

    let (tx, mut rx) = mpsc::unbounded_channel::<Highlight>();
    let tool = ReportHighlightTool::new(focus, tx);
    let ctx = ctx_from(MockFileSystem::new(), MockProcess::new());

    let args = json!({
        "tag": "security risks",
        "severity": "warning",
        "summary": "hardcoded secret"
    });
    let result = tool.execute(&args, &ctx).await.unwrap();
    assert!(!result.is_error);

    let hl = rx.try_recv().expect("highlight should be queued");
    assert_eq!(hl.tag, "security risks");
    assert_eq!(hl.severity, Severity::Warning);
    assert_eq!(hl.summary, "hardcoded secret");
}

#[tokio::test]
async fn report_highlight_effects_is_read() {
    let focus = Arc::new(FocusContract::empty());
    let (tx, _rx) = mpsc::unbounded_channel::<Highlight>();
    let tool = ReportHighlightTool::new(focus, tx);
    assert_eq!(tool.effects(), ToolEffects::Read);
}

#[tokio::test]
async fn report_highlight_rejects_tag_not_in_focus() {
    let mut focus = FocusContract::empty();
    focus.add("security risks").unwrap();
    let focus = Arc::new(focus);

    let (tx, mut rx) = mpsc::unbounded_channel::<Highlight>();
    let tool = ReportHighlightTool::new(focus, tx);
    let ctx = ctx_from(MockFileSystem::new(), MockProcess::new());

    let args = json!({
        "tag": "documentation",
        "severity": "info",
        "summary": "missing README"
    });
    let result = tool.execute(&args, &ctx).await;
    assert!(result.is_err());
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn report_highlight_accepts_blocking_severity() {
    let mut focus = FocusContract::empty();
    focus.add("breaking changes").unwrap();
    let focus = Arc::new(focus);

    let (tx, mut rx) = mpsc::unbounded_channel::<Highlight>();
    let tool = ReportHighlightTool::new(focus, tx);
    let ctx = ctx_from(MockFileSystem::new(), MockProcess::new());

    let args = json!({
        "tag": "breaking changes",
        "severity": "blocking",
        "summary": "API signature changed",
        "detail": "function foo removed"
    });
    let result = tool.execute(&args, &ctx).await.unwrap();
    assert!(!result.is_error);

    let hl = rx.try_recv().unwrap();
    assert_eq!(hl.severity, Severity::Blocking);
    assert_eq!(hl.detail.as_deref(), Some("function foo removed"));
}

#[tokio::test]
async fn report_highlight_rejects_invalid_severity() {
    let focus = Arc::new(FocusContract::empty());
    let (tx, _rx) = mpsc::unbounded_channel::<Highlight>();
    let tool = ReportHighlightTool::new(focus, tx);
    let ctx = ctx_from(MockFileSystem::new(), MockProcess::new());

    let args = json!({
        "tag": "anything",
        "severity": "critical",
        "summary": "x"
    });
    let result = tool.execute(&args, &ctx).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn registry_register_and_get_tool() {
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(ReadTool));
    assert_eq!(registry.len(), 1);
    assert!(registry.get("read").is_some());
    assert!(registry.get("missing").is_none());
}

#[tokio::test]
async fn registry_definitions_round_trip() {
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(ReadTool));
    registry.register(Box::new(WriteTool));

    let defs = registry.definitions();
    assert_eq!(defs.len(), 2);
    let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
    assert!(names.contains(&"read"));
    assert!(names.contains(&"write"));

    let read_def = defs.iter().find(|d| d.name == "read").unwrap();
    assert_eq!(read_def.effects, ToolEffects::Read);
    let write_def = defs.iter().find(|d| d.name == "write").unwrap();
    assert_eq!(write_def.effects, ToolEffects::Write);
}

#[tokio::test]
async fn registry_execute_dispatches_to_tool() {
    let fs = MockFileSystem::new();
    fs.insert_file("/workspace/foo.txt", b"hello");
    let ctx = ctx_from(fs, MockProcess::new());

    let mut registry = ToolRegistry::new();
    registry.register(Box::new(ReadTool));

    let args = json!({"path": "foo.txt"});
    let result = registry.execute("read", &args, &ctx).await.unwrap();
    assert_eq!(result.content, "hello");
    assert!(!result.is_error);
}

#[tokio::test]
async fn registry_execute_returns_error_for_unknown_tool() {
    let registry = ToolRegistry::new();
    let ctx = ctx_from(MockFileSystem::new(), MockProcess::new());
    let result = registry.execute("missing", &json!({}), &ctx).await;
    assert!(result.is_err());
}
