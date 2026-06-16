//! Benchmark: zero-copy context build vs naive clone.
//!
//! Run with: `cargo bench --bench context_build` (harness = false, so this
//! is just a normal binary).
//!
//! Compares two ways of producing a context ready to hand to a Provider:
//! - **Zero-copy**: `ContextBuilder::build` returns `Cow::Borrowed`
//!   slices over the cached messages and tools. No allocation.
//! - **Naive clone**: build the same shape by `clone()`-ing every
//!   message and tool, the way a naive implementation would.
//!
//! At 200 messages the difference is dominated by `Vec` allocation +
//! `String` clones, which is exactly the cost the zero-copy path avoids.

use std::hint::black_box;
use std::time::{Duration, Instant};

use opca_core::provider::{ContextBuilder, ContextRef, Message, ToolDef, ToolEffects};
use serde_json::json;

const MESSAGE_COUNT: usize = 200;
const TOOL_COUNT: usize = 8;
const ITERATIONS: u32 = 1_000;

fn main() {
    let messages = make_messages(MESSAGE_COUNT);
    let tools = make_tools(TOOL_COUNT);

    println!(
        "context_build: {MESSAGE_COUNT} messages, {TOOL_COUNT} tools, {ITERATIONS} iterations\n"
    );

    let zero = time_zero_copy(&messages, &tools);
    let clone = time_naive_clone(&messages, &tools);

    println!(
        "  zero-copy build : {:>8.2} ns / build",
        zero.as_secs_f64() * 1e9
    );
    println!(
        "  naive clone     : {:>8.2} ns / build",
        clone.as_secs_f64() * 1e9
    );
    println!(
        "  speedup         : {:>8.2}x",
        clone.as_secs_f64() / zero.as_secs_f64()
    );
}

fn time_zero_copy(messages: &[Message], tools: &[ToolDef]) -> Duration {
    let mut builder = ContextBuilder::new();
    builder.set_system_prompt("You are a helpful agent.".to_string());
    for m in messages {
        builder.append_message(m.clone());
    }
    builder.set_tools(tools.to_vec());

    let warm = builder.build();
    let _ = black_box(content_checksum(&warm));

    let mut sink: u64 = 0;
    let start = Instant::now();
    for _ in 0..ITERATIONS {
        let ctx = builder.build();
        sink = sink.wrapping_add(content_checksum(&ctx));
    }
    black_box(sink);
    start.elapsed() / ITERATIONS
}

fn time_naive_clone(messages: &[Message], tools: &[ToolDef]) -> Duration {
    let system_prompt = "You are a helpful agent.".to_string();

    let warm = naive_build(&system_prompt, messages, tools);
    let _ = black_box(naive_checksum(&warm));

    let mut sink: u64 = 0;
    let start = Instant::now();
    for _ in 0..ITERATIONS {
        let ctx = naive_build(&system_prompt, messages, tools);
        sink = sink.wrapping_add(naive_checksum(&ctx));
    }
    black_box(sink);
    start.elapsed() / ITERATIONS
}

/// Mirror of what `ContextBuilder::build` produces, but force every
/// message and tool to be cloned.
fn naive_build(
    system_prompt: &str,
    messages: &[Message],
    tools: &[ToolDef],
) -> (String, Vec<Message>, Vec<ToolDef>) {
    (system_prompt.to_string(), messages.to_vec(), tools.to_vec())
}

/// Force a real read of every borrowed message and tool so the optimiser
/// cannot prove the build has no observable effect.
fn content_checksum(ctx: &ContextRef<'_>) -> u64 {
    let mut h: u64 = 0;
    for m in ctx.messages.iter() {
        h = h.wrapping_add(m.text().len() as u64);
    }
    for t in ctx.tools.iter() {
        h = h.wrapping_add(t.name.len() as u64);
    }
    h
}

fn naive_checksum(ctx: &(String, Vec<Message>, Vec<ToolDef>)) -> u64 {
    let mut h: u64 = 0;
    h = h.wrapping_add(ctx.0.len() as u64);
    for m in &ctx.1 {
        h = h.wrapping_add(m.text().len() as u64);
    }
    for t in &ctx.2 {
        h = h.wrapping_add(t.name.len() as u64);
    }
    h
}

fn make_messages(n: usize) -> Vec<Message> {
    (0..n)
        .map(|i| {
            if i % 2 == 0 {
                Message::user(format!("user message number {i} with some payload text"))
            } else {
                Message::assistant(format!("assistant reply {i} ack the previous turn"))
            }
        })
        .collect()
}

fn make_tools(n: usize) -> Vec<ToolDef> {
    (0..n)
        .map(|i| ToolDef {
            name: format!("tool_{i}"),
            description: format!("tool number {i} does something useful"),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"}
                }
            }),
            effects: if i % 2 == 0 {
                ToolEffects::Read
            } else {
                ToolEffects::Write
            },
        })
        .collect()
}
