//! E2E smoke tests for `GeminiProvider`.
//!
//! These tests hit the real Google Gemini API and are `#[ignore = "requires GEMINI_API_KEY"]` by default.
//! Run with: `cargo test -p opca-core --test gemini_smoke -- --ignored`
//!
//! Requires `GEMINI_API_KEY` environment variable.

use opca_core::provider::{GeminiProvider, Message, Provider, ProviderEvent};
use tokio_stream::StreamExt;

#[tokio::test]
#[ignore = "requires GEMINI_API_KEY"]
async fn gemini_simple_conversation() {
    let key = std::env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY must be set");
    let provider = GeminiProvider::new(&key, "gemini-2.0-flash");

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
#[ignore = "requires GEMINI_API_KEY"]
async fn gemini_system_prompt() {
    let key = std::env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY must be set");
    let provider = GeminiProvider::new(&key, "gemini-2.0-flash");

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
