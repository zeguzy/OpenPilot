//! E2E smoke tests for `AnthropicProvider`.
//!
//! These tests hit the real Anthropic API and are `#[ignore = "requires ANTHROPIC_API_KEY"]` by default.
//! Run with: `cargo test -p opca-core --test anthropic_smoke -- --ignored`
//!
//! Requires `ANTHROPIC_API_KEY` environment variable.

use opca_core::provider::{AnthropicProvider, Message, Provider, ProviderEvent};
use tokio_stream::StreamExt;

#[tokio::test]
#[ignore = "requires ANTHROPIC_API_KEY"]
async fn anthropic_simple_conversation() {
    let key = std::env::var("ANTHROPIC_API_KEY").expect("ANTHROPIC_API_KEY must be set");
    let provider = AnthropicProvider::new(&key, "claude-sonnet-4-20250514");

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
#[ignore = "requires ANTHROPIC_API_KEY"]
async fn anthropic_system_prompt() {
    let key = std::env::var("ANTHROPIC_API_KEY").expect("ANTHROPIC_API_KEY must be set");
    let provider = AnthropicProvider::new(&key, "claude-sonnet-4-20250514");

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
