use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use opca_core::provider::{ToolCall, ToolEffects, ToolResult};
use opca_core::tools::dispatch::dispatch_batch;
use opca_core::tools::{Tool, ToolContext, ToolRegistry};
use opca_test_utils::{MockFileSystem, MockProcess};
use serde_json::Value;

struct SlowTool {
    name: &'static str,
    effects: ToolEffects,
    delay: Duration,
}

#[async_trait]
impl Tool for SlowTool {
    fn name(&self) -> &'static str {
        self.name
    }
    fn description(&self) -> &'static str {
        "slow test tool"
    }
    fn parameters_schema(&self) -> Value {
        Value::Object(serde_json::Map::new())
    }
    fn effects(&self) -> ToolEffects {
        self.effects
    }
    async fn execute(&self, _args: &Value, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        tokio::time::sleep(self.delay).await;
        Ok(ToolResult {
            content: format!("{} done", self.name),
            is_error: false,
        })
    }
}

fn make_ctx() -> ToolContext {
    let fs = MockFileSystem::new();
    let proc = MockProcess::new();
    ToolContext {
        workspace_path: PathBuf::from("/workspace"),
        fs: Arc::new(fs),
        proc: Arc::new(proc),
    }
}

fn make_call(id: &str, name: &str) -> ToolCall {
    ToolCall {
        id: id.to_string(),
        name: name.to_string(),
        arguments: Value::Object(serde_json::Map::new()),
    }
}

#[tokio::test]
async fn parallel_read_dispatch_runs_concurrently() {
    let delay = Duration::from_millis(120);
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(SlowTool {
        name: "slow_read_a",
        effects: ToolEffects::Read,
        delay,
    }));
    registry.register(Box::new(SlowTool {
        name: "slow_read_b",
        effects: ToolEffects::Read,
        delay,
    }));
    registry.register(Box::new(SlowTool {
        name: "slow_read_c",
        effects: ToolEffects::Read,
        delay,
    }));

    let calls = vec![
        make_call("1", "slow_read_a"),
        make_call("2", "slow_read_b"),
        make_call("3", "slow_read_c"),
    ];

    let ctx = make_ctx();
    let start = Instant::now();
    let results = dispatch_batch(&registry, &calls, &ctx).await;
    let elapsed = start.elapsed();

    assert_eq!(results.len(), 3);
    for (_id, r) in &results {
        assert!(r.is_ok(), "expected ok, got {r:?}");
    }
    assert!(
        elapsed < delay * 2,
        "parallel should be faster than 2x: elapsed={elapsed:?}"
    );
}

#[tokio::test]
async fn serial_write_dispatch_takes_full_time() {
    let delay = Duration::from_millis(120);
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(SlowTool {
        name: "slow_write_a",
        effects: ToolEffects::Write,
        delay,
    }));
    registry.register(Box::new(SlowTool {
        name: "slow_write_b",
        effects: ToolEffects::Write,
        delay,
    }));

    let calls = vec![
        make_call("w1", "slow_write_a"),
        make_call("w2", "slow_write_b"),
    ];

    let ctx = make_ctx();
    let start = Instant::now();
    let results = dispatch_batch(&registry, &calls, &ctx).await;
    let elapsed = start.elapsed();

    assert_eq!(results.len(), 2);
    for (_id, r) in &results {
        assert!(r.is_ok());
    }
    assert!(
        elapsed >= delay * 2,
        "serial should take >= 2x: elapsed={elapsed:?}"
    );
}

#[tokio::test]
async fn serial_process_dispatch_takes_full_time() {
    let delay = Duration::from_millis(80);
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(SlowTool {
        name: "slow_proc",
        effects: ToolEffects::Process,
        delay,
    }));

    let calls = vec![
        make_call("p1", "slow_proc"),
        make_call("p2", "slow_proc"),
        make_call("p3", "slow_proc"),
    ];

    let ctx = make_ctx();
    let start = Instant::now();
    let results = dispatch_batch(&registry, &calls, &ctx).await;
    let elapsed = start.elapsed();

    assert_eq!(results.len(), 3);
    assert!(
        elapsed >= delay * 3,
        "serial should take >= 3x: elapsed={elapsed:?}"
    );
}

#[tokio::test]
async fn mixed_batch_parallel_then_serial_preserves_results() {
    let delay = Duration::from_millis(60);
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(SlowTool {
        name: "r1",
        effects: ToolEffects::Read,
        delay,
    }));
    registry.register(Box::new(SlowTool {
        name: "r2",
        effects: ToolEffects::Read,
        delay,
    }));
    registry.register(Box::new(SlowTool {
        name: "w1",
        effects: ToolEffects::Write,
        delay,
    }));
    registry.register(Box::new(SlowTool {
        name: "w2",
        effects: ToolEffects::Write,
        delay,
    }));

    let calls = vec![
        make_call("c1", "r1"),
        make_call("c2", "w1"),
        make_call("c3", "r2"),
        make_call("c4", "w2"),
    ];

    let ctx = make_ctx();
    let results = dispatch_batch(&registry, &calls, &ctx).await;

    assert_eq!(results.len(), 4);
    let ids: Vec<&str> = results.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, vec!["c1", "c2", "c3", "c4"]);
    for (_id, r) in &results {
        assert!(r.is_ok());
    }
}

#[tokio::test]
async fn unknown_tool_returns_error_for_that_call() {
    let registry = ToolRegistry::new();
    let calls = vec![make_call("u1", "nope")];

    let ctx = make_ctx();
    let results = dispatch_batch(&registry, &calls, &ctx).await;

    assert_eq!(results.len(), 1);
    let (id, res) = &results[0];
    assert_eq!(id, "u1");
    assert!(res.is_err());
}

#[tokio::test]
async fn unknown_tool_does_not_block_other_calls() {
    let delay = Duration::from_millis(50);
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(SlowTool {
        name: "ok_read",
        effects: ToolEffects::Read,
        delay,
    }));

    let calls = vec![make_call("ok", "ok_read"), make_call("bad", "missing")];

    let ctx = make_ctx();
    let results = dispatch_batch(&registry, &calls, &ctx).await;

    assert_eq!(results.len(), 2);
    assert!(results[0].1.is_ok());
    assert!(results[1].1.is_err());
}

#[tokio::test]
async fn empty_batch_returns_empty() {
    let registry = ToolRegistry::new();
    let ctx = make_ctx();
    let results = dispatch_batch(&registry, &[], &ctx).await;
    assert!(results.is_empty());
}

#[tokio::test]
async fn append_effect_runs_in_parallel() {
    let delay = Duration::from_millis(80);
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(SlowTool {
        name: "app1",
        effects: ToolEffects::Append,
        delay,
    }));
    registry.register(Box::new(SlowTool {
        name: "app2",
        effects: ToolEffects::Append,
        delay,
    }));
    registry.register(Box::new(SlowTool {
        name: "app3",
        effects: ToolEffects::Append,
        delay,
    }));

    let calls = vec![
        make_call("a1", "app1"),
        make_call("a2", "app2"),
        make_call("a3", "app3"),
    ];

    let ctx = make_ctx();
    let start = Instant::now();
    let results = dispatch_batch(&registry, &calls, &ctx).await;
    let elapsed = start.elapsed();

    assert_eq!(results.len(), 3);
    assert!(
        elapsed < delay * 2,
        "append should be parallel: elapsed={elapsed:?}"
    );
}
