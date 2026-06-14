//! E2E smoke tests for `OpenAIProvider`.
//!
//! These tests hit the real `OpenAI` API and are `#[ignore = "requires OPENAI_API_KEY"]` by default.
//! Run with: `cargo test -p opca-core --test openai_smoke -- --ignored`
//!
//! Requires `OPENAI_API_KEY` environment variable.

use opca_core::provider::{Message, OpenAIProvider, Provider, ProviderEvent};
use tokio_stream::StreamExt;

#[tokio::test]
#[ignore = "requires OPENAI_API_KEY"]
async fn openai_simple_conversation() {
    let key = std::env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY must be set");
    let provider = OpenAIProvider::new(&key, "gpt-4o");

    let stream = provider
        .stream(
            &[Message::user("Reply with exactly the word: hello")],
            &[],
            None,
        )
        .await
        .expect("stream should start");

    let events: Vec<_> = stream.collect().await;

    let text: String = events
        .iter()
        .filter_map(|e| match e.as_ref().ok()? {
            ProviderEvent::TextDelta(t) => Some(t.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("");

    assert!(
        !text.is_empty(),
        "expected at least one TextDelta, got events: {events:?}"
    );

    let has_done = events
        .iter()
        .any(|e| matches!(e.as_ref().ok(), Some(ProviderEvent::Done { .. })));
    assert!(has_done, "stream should end with Done, got: {events:?}");
}

#[tokio::test]
#[ignore = "requires OPENAI_API_KEY"]
async fn openai_system_prompt() {
    let key = std::env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY must be set");
    let provider = OpenAIProvider::new(&key, "gpt-4o");

    let stream = provider
        .stream(
            &[Message::user("What is your name?")],
            &[],
            Some(
                "You are a helpful assistant named TestBot. Always introduce yourself as TestBot.",
            ),
        )
        .await
        .expect("stream should start");

    let events: Vec<_> = stream.collect().await;

    let text: String = events
        .iter()
        .filter_map(|e| match e.as_ref().ok()? {
            ProviderEvent::TextDelta(t) => Some(t.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("");

    assert!(
        !text.is_empty(),
        "expected non-empty response, got: {events:?}"
    );
}
