use std::borrow::Cow;
use std::hint::black_box;
use std::time::Instant;

use opca_core::provider::{ContextBuilder, Message, MessageRole, ToolDef, ToolEffects, ToolResult};
use serde_json::json;

fn sample_tool(name: &str) -> ToolDef {
    ToolDef {
        name: name.to_string(),
        description: format!("tool {name}"),
        parameters: json!({"type": "object"}),
        effects: ToolEffects::Read,
    }
}

#[test]
fn context_builder_starts_empty() {
    let builder = ContextBuilder::new();
    let ctx = builder.build();
    assert!(ctx.system_prompt.is_none());
    assert!(ctx.messages.is_empty());
    assert!(ctx.tools.is_empty());
}

#[test]
fn context_builder_accumulates_messages() {
    let mut builder = ContextBuilder::new();
    builder.append_message(Message::user("hello"));
    builder.append_message(Message::assistant("hi"));

    let ctx = builder.build();
    assert_eq!(ctx.messages.len(), 2);
    assert_eq!(ctx.messages[0].text(), "hello");
    assert_eq!(ctx.messages[1].text(), "hi");
}

#[test]
fn context_builder_sets_system_prompt() {
    let mut builder = ContextBuilder::new();
    builder.set_system_prompt("be helpful".to_string());

    let ctx = builder.build();
    assert_eq!(ctx.system_prompt, Some("be helpful"));
}

#[test]
fn context_builder_sets_tools() {
    let mut builder = ContextBuilder::new();
    builder.set_tools(vec![sample_tool("read"), sample_tool("write")]);

    let ctx = builder.build();
    assert_eq!(ctx.tools.len(), 2);
    assert_eq!(ctx.tools[0].name, "read");
    assert_eq!(ctx.tools[1].name, "write");
}

#[test]
fn context_build_returns_cow_borrowed_zero_copy() {
    let mut builder = ContextBuilder::new();
    builder.append_message(Message::user("msg"));

    let ctx = builder.build();
    assert!(
        matches!(ctx.messages, Cow::Borrowed(_)),
        "messages should be Cow::Borrowed (zero-copy)"
    );
    assert!(
        matches!(ctx.tools, Cow::Borrowed(_)),
        "tools should be Cow::Borrowed (zero-copy)"
    );
}

#[test]
fn context_build_large_history_is_borrowed() {
    let mut builder = ContextBuilder::new();
    for i in 0..200 {
        builder.append_message(Message::user(format!("message {i}")));
    }
    builder.append_message(Message::assistant("new response"));

    let ctx = builder.build();
    assert_eq!(ctx.messages.len(), 201);
    assert!(
        matches!(ctx.messages, Cow::Borrowed(_)),
        "201 messages must be Borrowed, not Owned"
    );
}

#[test]
fn context_ref_into_owned_clones_data() {
    let mut builder = ContextBuilder::new();
    builder.append_message(Message::user("data"));

    let ctx = builder.build();
    let owned = ctx.into_owned();
    assert_eq!(owned.messages.len(), 1);
    assert_eq!(owned.messages[0].text(), "data");
}

#[test]
fn message_constructors_set_correct_roles() {
    let u = Message::user("question");
    assert_eq!(u.role, MessageRole::User);
    assert_eq!(u.text(), "question");
    assert!(u.tool_calls().is_empty());
    assert!(u.tool_result_info().is_none());
    assert!(!u.has_thinking());

    let a = Message::assistant("answer");
    assert_eq!(a.role, MessageRole::Assistant);

    let s = Message::system("rules");
    assert_eq!(s.role, MessageRole::System);

    let tr = Message::tool_result(
        "call_1",
        ToolResult {
            content: "result".to_string(),
            is_error: false,
        },
    );
    assert_eq!(tr.role, MessageRole::Tool);
    assert_eq!(tr.tool_result_info().map(|(id, _)| id), Some("call_1"));
    assert!(tr.tool_result_info().is_some());
}

#[test]
fn message_assistant_with_tools_carries_calls() {
    use opca_core::provider::ToolCall;

    let msg = Message::assistant_with_tools(
        "let me check",
        vec![ToolCall {
            id: "call_42".to_string(),
            name: "read".to_string(),
            arguments: json!({"path": "foo.rs"}),
        }],
    );

    assert_eq!(msg.role, MessageRole::Assistant);
    assert_eq!(msg.text(), "let me check");
    assert_eq!(msg.tool_calls().len(), 1);
    assert_eq!(msg.tool_calls()[0].id, "call_42");
    assert_eq!(msg.tool_calls()[0].name, "read");
}

#[test]
#[ignore = "benchmark — run with: cargo test --release -- --ignored context_build_benchmark"]
fn context_build_benchmark_cow_vs_clone() {
    let mut builder = ContextBuilder::new();
    for i in 0..200 {
        builder.append_message(Message::user(format!("message {i}")));
    }
    builder.set_tools(vec![sample_tool("read"), sample_tool("write")]);

    let iterations = 50_000u64;

    let start = Instant::now();
    for _ in 0..iterations {
        let ctx = builder.build();
        black_box(&ctx);
    }
    let cow_elapsed = start.elapsed();

    let start = Instant::now();
    for _ in 0..iterations {
        let owned = builder.build().into_owned();
        black_box(&owned);
    }
    let clone_elapsed = start.elapsed();

    eprintln!(
        "Context build ({iterations} iterations, 200 messages):\n  Cow build:   {cow_elapsed:?}\n  Clone build: {clone_elapsed:?}\n  Ratio:       {:.1}x",
        clone_elapsed.as_secs_f64() / cow_elapsed.as_secs_f64()
    );

    assert!(
        cow_elapsed < clone_elapsed,
        "zero-copy Cow build should be faster than clone"
    );
}
