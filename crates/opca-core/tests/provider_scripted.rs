use opca_core::provider::{Message, Provider, ProviderEvent, StopReason, ToolDef, ToolEffects};
use opca_test_utils::ScriptedProvider;
use serde_json::json;
use tokio_stream::StreamExt;

fn sample_tools() -> Vec<ToolDef> {
    vec![ToolDef {
        name: "read".to_string(),
        description: "read a file".to_string(),
        parameters: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
        effects: ToolEffects::Read,
    }]
}

#[tokio::test]
async fn single_text_response() {
    let provider = ScriptedProvider::new().then_text("hello").then_done();

    let stream = provider
        .stream(&[], &[], None)
        .await
        .expect("stream should start");
    let events: Vec<_> = stream.collect().await;

    assert_eq!(events.len(), 2);
    assert!(events[0].is_ok());
    assert_eq!(
        events[0].as_ref().unwrap(),
        &ProviderEvent::TextDelta("hello".to_string())
    );
    assert!(events[1].is_ok());
    assert_eq!(
        events[1].as_ref().unwrap(),
        &ProviderEvent::Done {
            stop_reason: StopReason::EndTurn,
        }
    );
}

#[tokio::test]
async fn tool_call_then_text_sequence() {
    let provider = ScriptedProvider::new()
        .then_tool_call("read", json!({"path": "foo.rs"}))
        .then_text("done")
        .then_done();

    let stream = provider
        .stream(&[], &sample_tools(), None)
        .await
        .expect("stream should start");
    let events: Vec<_> = stream.collect().await;

    assert_eq!(events.len(), 5);

    let e0 = events[0].as_ref().expect("event 0 ok");
    let id = match e0 {
        ProviderEvent::ToolCallStart { id, name } => {
            assert_eq!(name, "read");
            id.clone()
        }
        other => panic!("expected ToolCallStart, got {other:?}"),
    };

    match events[1].as_ref().expect("event 1 ok") {
        ProviderEvent::ToolCallArgs { id: aid, args } => {
            assert_eq!(aid, &id);
            assert!(args.contains("foo.rs"));
        }
        other => panic!("expected ToolCallArgs, got {other:?}"),
    }

    match events[2].as_ref().expect("event 2 ok") {
        ProviderEvent::ToolCallEnd { id: eid } => assert_eq!(eid, &id),
        other => panic!("expected ToolCallEnd, got {other:?}"),
    }

    assert_eq!(
        events[3].as_ref().unwrap(),
        &ProviderEvent::TextDelta("done".to_string())
    );
    assert_eq!(
        events[4].as_ref().unwrap(),
        &ProviderEvent::Done {
            stop_reason: StopReason::EndTurn,
        }
    );
}

#[tokio::test]
async fn exhausted_returns_error() {
    let provider = ScriptedProvider::new();
    let result = provider.stream(&[], &[], None).await;
    assert!(result.is_err(), "empty script should error");
    let msg = result.err().expect("should be error").to_string();
    assert!(
        msg.contains("exhausted"),
        "error should mention 'exhausted', got: {msg}"
    );
}

#[tokio::test]
async fn multiple_tool_calls_and_text_in_order() {
    let provider = ScriptedProvider::new()
        .then_tool_call("read", json!({"path": "a.rs"}))
        .then_tool_call("write", json!({"path": "b.rs", "content": "x"}))
        .then_text("both done")
        .then_done();

    let stream = provider
        .stream(&[], &[], None)
        .await
        .expect("stream should start");
    let events: Vec<_> = stream.collect().await;

    assert_eq!(events.len(), 8);

    let kinds: Vec<&str> = events
        .iter()
        .map(|e| match e.as_ref().unwrap() {
            ProviderEvent::ToolCallStart { .. } => "start",
            ProviderEvent::ToolCallArgs { .. } => "args",
            ProviderEvent::ToolCallEnd { .. } => "end",
            ProviderEvent::TextDelta(_) => "text",
            ProviderEvent::ThinkingDelta(_) => "thinking",
            ProviderEvent::Usage { .. } => "usage",
            ProviderEvent::Done { .. } => "done",
            ProviderEvent::Error(_) => "error",
        })
        .collect();
    assert_eq!(
        kinds,
        vec![
            "start", "args", "end", "start", "args", "end", "text", "done"
        ]
    );

    let id1 = match events[0].as_ref().unwrap() {
        ProviderEvent::ToolCallStart { id, name } => {
            assert_eq!(name, "read");
            id.clone()
        }
        other => panic!("expected ToolCallStart, got {other:?}"),
    };
    let id2 = match events[3].as_ref().unwrap() {
        ProviderEvent::ToolCallStart { id, name } => {
            assert_eq!(name, "write");
            id.clone()
        }
        other => panic!("expected ToolCallStart, got {other:?}"),
    };
    assert_ne!(id1, id2, "tool call ids must be unique");

    match events[1].as_ref().unwrap() {
        ProviderEvent::ToolCallArgs { id, .. } => assert_eq!(id, &id1),
        other => panic!("expected ToolCallArgs, got {other:?}"),
    }
    match events[2].as_ref().unwrap() {
        ProviderEvent::ToolCallEnd { id } => assert_eq!(id, &id1),
        other => panic!("expected ToolCallEnd, got {other:?}"),
    }
    match events[4].as_ref().unwrap() {
        ProviderEvent::ToolCallArgs { id, .. } => assert_eq!(id, &id2),
        other => panic!("expected ToolCallArgs, got {other:?}"),
    }
    match events[5].as_ref().unwrap() {
        ProviderEvent::ToolCallEnd { id } => assert_eq!(id, &id2),
        other => panic!("expected ToolCallEnd, got {other:?}"),
    }
}

#[tokio::test]
async fn multi_turn_streaming() {
    let provider = ScriptedProvider::new()
        .then_tool_call("read", json!({"path": "foo.rs"}))
        .then_done()
        .then_text("here is the result")
        .then_done();

    let stream1 = provider
        .stream(&[], &[], None)
        .await
        .expect("turn 1 stream");
    let events1: Vec<_> = stream1.collect().await;
    assert_eq!(events1.len(), 4);

    let stream2 = provider
        .stream(&[], &[], None)
        .await
        .expect("turn 2 stream");
    let events2: Vec<_> = stream2.collect().await;
    assert_eq!(events2.len(), 2);
    assert_eq!(
        events2[0].as_ref().unwrap(),
        &ProviderEvent::TextDelta("here is the result".to_string())
    );

    let result = provider.stream(&[], &[], None).await;
    assert!(result.is_err(), "third call should be exhausted");
}

#[tokio::test]
async fn tool_result_in_script_produces_no_events() {
    let provider = ScriptedProvider::new()
        .then_tool_call("read", json!({"path": "x.rs"}))
        .then_tool_result("file contents here", false)
        .then_text("analyzed")
        .then_done();

    let stream = provider
        .stream(&[], &[], None)
        .await
        .expect("stream should start");
    let events: Vec<_> = stream.collect().await;

    assert_eq!(
        events.len(),
        5,
        "ToolResult should produce no stream events"
    );
}

#[tokio::test]
async fn script_ignores_messages_and_tools() {
    let provider = ScriptedProvider::new().then_text("ok").then_done();
    let messages = vec![Message::user("hello"), Message::assistant("hi there")];

    let stream = provider
        .stream(&messages, &sample_tools(), Some("be helpful"))
        .await
        .expect("stream should start");
    let events: Vec<_> = stream.collect().await;
    assert_eq!(events.len(), 2);
}
