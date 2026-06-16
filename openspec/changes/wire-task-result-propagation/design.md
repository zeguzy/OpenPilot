## Context

The Orchestrator currently consumes Task output through four channels, but only two are wired end-to-end, and even those two drop a structured payload on the floor. Concrete current state (verified by reading `orchestrator.rs`, `channels.rs`, `compact.rs`, `routing.rs`, `sub_agent/`):

| Channel | Produces | Orchestrator does | Reaches memory? |
|---|---|---|---|
| `heartbeat_tx` | `Heartbeat { status, progress, summary, todo, subtasks }` | `drain_heartbeats` → `OrchestratorEvent { kind: Heartbeat, text: "[N%] summary" }` | ✅ |
| `highlight_tx` | `Highlight { tag, severity, summary, detail }` | `drain_highlights` → `OrchestratorEvent { text: "[tag] summary" }` | ✅ but **`detail` is dropped** |
| `output_tx` | `TaskOutput::{TextDelta, ThinkingDelta, ToolCall, ToolResult, Highlight, StatusChanged, Done}` | `drain_outputs` returns `Vec<...>` to caller | ❌ never enters memory |
| `TaskOutcome` (`JoinHandle`) | `Completed(Message) \| Cancelled \| Error(String)` | stored in `TaskEntry.join_handle`, **never awaited by Orchestrator** | ❌ no bridge |

Compounding this, `routing.rs::route(message, _context)` takes a context parameter but ignores it — routing is pure keyword matching with no memory of prior outcomes.

The previous change `refactor-orchestrator-prompt-system` addressed the **upstream** side (how Tasks are prompted, how they self-verify, how sub-tasks spawn). It deliberately did **not** touch the downstream propagation. This change closes that gap. Every fix here connects a pipe that already exists in the type system — no new types are introduced except (a) two `EventKind` variants for differential retention and (b) a small `RouteContext` struct.

**Stakeholders:**
- **Orchestrator** — gains real Task output in its context window; routing becomes context-aware
- **Tasks** — no behavior change; their existing outputs are simply consumed instead of dropped
- **Parent Tasks (sub-agents)** — receive full child results instead of one summary line
- **CLI / TUI** — indirectly benefits: Orchestrator replies to "what did task X do?" questions accurately instead of hallucinating from a 10-token summary

## Goals / Non-Goals

**Goals:**
- Eliminate the four disconnected pipes: `Highlight.detail`, `TaskOutput` (sampled), `TaskOutcome`, sub-task enrichment
- Give the Orchestrator's next-turn decision access to (a bounded view of) prior Task outcomes
- Make routing tie-breaking use a small, structured context snapshot
- Preserve the Focus Contract / three-layer model: heartbeat (Layer 1) and highlight (Layer 2) stay lightweight; this change adds a **bounded outcome stream** that lives alongside them, not a replacement
- Stay non-breaking at the on-disk format level (OrchestratorEvent is in-memory only; serde additions are additive)

**Non-Goals:**
- NOT redesigning `Memory<T>` or the compaction archive format — reuses existing infrastructure
- NOT replacing keyword routing with an LLM call — `route()` stays cheap and deterministic, context only tie-breaks
- NOT streaming every `TaskOutput` variant into memory — explicit sampling policy (D3) bounds growth
- NOT touching prompt templates or the phase protocol — orthogonal to `refactor-orchestrator-prompt-system`
- NOT changing workspace isolation, the `Workspace::ChangeSet` shape, or provider integration

## Decisions

### D1: `Highlight.detail` propagation format

**Choice:** Append `detail` to the `OrchestratorEvent.text` field with a delimiter, conditionally:

```rust
let text = match &hl.detail {
    Some(d) => format!("[{}] {}\n---\n{}", hl.tag, hl.summary, d),
    None => format!("[{}] {}", hl.tag, hl.summary),
};
```

**Rationale:** `OrchestratorEvent.text` is already a free-form `String` consumed by `summarize()` and the Orchestrator's context renderer. No struct change, no migration. The `\n---\n` delimiter is greppable and survives single-line rendering by collapsing to a separator in TUI output.

**Alternatives considered:**
- *Add `OrchestratorEvent.detail: Option<String>` field.* Rejected — ripples through `MemoryItem`, `MemoryMeta`, serde snapshots, and compaction. Out of proportion to the fix.
- *Promote `Highlight` itself into a new `EventKind::Highlight` variant carrying the full struct.* Rejected — `EventKind` would grow to carry `Highlight` data, breaking the "kind is a classifier" invariant.

### D2: `TaskOutcome` bridge site

**Choice:** A new method on `Orchestrator`:

```rust
pub fn poll_task_outcome(&mut self, task_id: &str) -> Result<Option<TaskOutcome>> {
    let entry = self.tasks.get_mut(task_id)?;
    let handle = entry.join_handle.take().context("no join_handle")?;
    match handle.try_now_or_extend() {  // non-blocking
        Ok(Ok(outcome)) => { self.record_outcome(task_id, outcome)?; Ok(Some(outcome)) }
        Ok(Err(join_err)) => Err(join_err.into()),
        Err(NotFinished) => { entry.join_handle = Some(handle); Ok(None) }
    }
}
```

Called from `RealOrchestrator`'s turn loop **before** each Orchestrator LLM call, for every Task whose status is terminal (`Delivered | Archived | Stuck | Axed`) but whose outcome has not yet been recorded. The Orchestrator does NOT block on this — `try_now_or_extend` returns `NotFinished` immediately if the Task hasn't finished.

**Rationale:** The Orchestrator's turn loop already polls heartbeat/highlight channels non-blockingly (`drain_heartbeats` / `drain_highlights` use `try_recv`). Adding a `try_poll` for join handles mirrors that pattern. Blocking the Orchestrator on a slow Task's `JoinHandle::await` would re-introduce the very latency the background-first model exists to avoid.

**Alternatives considered:**
- *Spawn a long-lived `outcome_collector` tokio task that awaits all join handles and pushes events into a channel.* Rejected — adds another concurrency source, complicates shutdown ordering, and the Orchestrator still needs a drain method, so the win is marginal.
- *Await the join handle lazily inside `route()` when the routing layer needs the outcome.* Rejected — `route()` must stay cheap (microseconds, not the seconds a long Task might block).
- *Drop `TaskOutcome` entirely and rely on the heartbeat's final summary.* Rejected — the heartbeat's `summary` is a free-text string the Task writes; `TaskOutcome::Completed(Message)` is the actual final assistant message with full deliverable content. They are not interchangeable.

### D3: `TaskOutput` sampling policy

**Choice:** A pure function `promote_output(out: &TaskOutput, seen_tools: &mut HashSet<String>) -> Option<PromotedOutput>` decides what enters memory:

| `TaskOutput` variant | Promoted? | Notes |
|---|---|---|
| `ToolResult { success: false, .. }` | **Always** | Failures are decision-relevant; cap applies after |
| `ToolCall { name, args }` | **First occurrence per `name`** | De-dup via `seen_tools`; subsequent calls of the same tool are summarised as `(name ×N)` in compaction |
| `TextDelta(_)` | Never | Available via `deep_dive` |
| `ThinkingDelta(_)` | Never | Available via `deep_dive` |
| `Highlight(_)` | Never here | Already has its own channel |
| `StatusChanged { .. }` | Never here | Already covered by heartbeat channel |
| `Done` | Never here | Implied by `TaskOutcome` bridge (D2) |

Promoted outputs are wrapped in a new `EventKind::ToolActivity { success: bool }` (D6) and pushed into memory. A per-Task counter in `TaskEntry` caps promotion at `max_promoted_outputs_per_task` (configurable, default 8). When the cap is hit, the oldest `ToolActivity` for that Task triggers archival via the compaction strategy (D6).

**Rationale:** Failures and first-time tool uses carry the most signal; subsequent identical tool calls and streaming tokens are noise at the Orchestrator level. The cap bounds the worst-case contribution of any single Task to the Orchestrator's active memory.

**Alternatives considered:**
- *Promote everything, let compaction handle overflow.* Rejected — a single chatty Task (e.g. one that calls `ReadTool` 50 times) would evict everyone else's state before compaction runs. Compaction is per-turn, not per-event.
- *LLM-based summarisation of the stream.* Rejected — adds latency and cost on the hot path; the Orchestrator already pays for its own LLM turn.
- *Time-window sampling (one event per N seconds).* Rejected — risks missing a burst of failures in a tight window.

### D4: Sub-task result enrichment scope

**Choice:** Extend `SubTaskResult` to carry three new fields, all backward-compatible (Options / Vecs defaulting to empty):

```rust
pub struct SubTaskResult {
    pub task_id: String,
    pub summary: String,                                  // unchanged: latest_heartbeat.summary
    pub final_message: Option<String>,                    // NEW: from TaskOutcome::Completed
    pub highlights: Vec<Highlight>,                       // NEW: copied from TaskEntry.highlights
    pub artifacts: Vec<PathBuf>,                          // NEW: from Workspace::ChangeSet
}
```

The injected steering message to the parent changes from a single line to a multi-line block:

```
[Sub-task result] task-007
summary: 修复了登录 bug
final_message: 重构了 auth 模块到 OAuth2，新增 5 个文件...
highlights:
  - [security] 用了 PKCE
artifacts:
  - src/auth/oauth2.rs
  - src/auth/pkce.rs
```

**Rationale:** The parent Task is the one making decisions about whether to re-dispatch, merge, or escalate. With the current single-line injection, the parent has no information to discriminate between "child succeeded trivially" and "child succeeded with major architectural changes". The enriched block gives it both the headline (`summary`), the deliverable (`final_message`), the findings (`highlights`), and the on-disk footprint (`artifacts`).

**Token budget:** A typical enriched block is 200-500 tokens. With max 3 concurrent sub-tasks per parent (existing limit), worst case is ~1.5K tokens injected — well within the parent's context budget and far less than the 9K-token worst case of forwarding the full `TaskOutcome::Completed` Message verbatim.

**Alternatives considered:**
- *Forward the full `TaskOutcome::Completed(Message)` Message.* Rejected — Messages can be 3K+ tokens; multiplied by 3 concurrent children, this dominates the parent's context.
- *Only add `final_message`, skip highlights and artifacts.* Rejected — `final_message` is prose; without `highlights` (structured findings) and `artifacts` (verifiable paths), the parent can't make grounded decisions about merging or escalating.
- *Leave `SubTaskResult` alone and rely on parent calling `deep_dive(child_id)`.* Rejected — shifts the burden to the parent's prompt to know when to call `deep_dive`, which is exactly the kind of "Task has to guess" pattern the Focus Contract exists to eliminate.

### D5: `route()` context shape and tie-breaking rules

**Choice:** Replace the underscored parameter with a small struct:

```rust
#[derive(Debug, Clone, Default)]
pub struct RouteContext {
    pub recent_outcomes: Vec<TaskOutcomeDigest>,   // last N completed tasks (id, summary, terminal_status)
    pub pending_task_count: usize,
    pub last_dispatched_task_id: Option<String>,
    pub last_dispatched_at: Option<Instant>,
}

pub struct TaskOutcomeDigest {
    pub task_id: String,
    pub summary: String,                // first 200 chars of TaskOutcome::Completed
    pub terminal_status: TaskStatus,
}

pub fn route(message: &str, ctx: &RouteContext) -> RouteDecision { ... }
```

Tie-breaking rules (applied **only** when keyword matching is ambiguous — both `has_background` and `has_foreground` are true, or both are false):

1. **Follow-up detection:** if `last_dispatched_at` is within 60 seconds AND `message.len() < 100` AND message starts with a follow-up word (`也`, `另外`, `顺便`, `and`, `also`, `then`, `接着`) → route to **Foreground** (treat as steering for an in-flight Task).
2. **Recent-failure amplification:** if any `recent_outcomes` entry has `terminal_status == Stuck | Axed` AND its `summary` contains a substring overlapping with `message` (bigram overlap > 0.3) → route to **Background** with focus inherited from the failed Task.
3. **Otherwise:** default to current keyword-only behavior.

**Rationale:** Routing stays O(message length) — the expensive parts (LLM, archive recall) are not on this path. The context only breaks ties that keyword matching currently gets wrong. Rule 1 fixes the "那顺便也把测试加上" case. Rule 2 fixes the "user re-asks about a stuck task and we should re-dispatch" case.

**Alternatives considered:**
- *LLM routing (small model classifies intent).* Rejected — every user message would incur a model call before the Orchestrator even starts thinking.
- *Replace keywords entirely with learned classifier.* Rejected — out of scope, requires training data.
- *Pass the full Orchestrator memory as `context: &str`.* Rejected — unbounded string; the Orchestrator would have to render its whole active region just to call `route`. The digest struct is bounded and cheap.

### D6: `EventKind` extension and compaction integration

**Choice:** Extend `EventKind` with two variants:

```rust
pub enum EventKind {
    Heartbeat,
    Highlight { task_completed: bool },
    Other,
    Delivered,                  // NEW: TaskOutcome::Completed bridge
    ToolActivity { success: bool },  // NEW: promoted TaskOutput
}
```

Compaction rules (in `OrchestratorCompaction`):

| Variant | Retention |
|---|---|
| `Heartbeat` | unchanged: keep latest per task |
| `Highlight { task_completed: false }` | unchanged: keep last 3 per task |
| `Highlight { task_completed: true }` | unchanged: collapse to single summary per task |
| `Delivered` | **never archived** — Task's final outcome is the single most important event |
| `ToolActivity { success: false }` | keep last 3 per task |
| `ToolActivity { success: true }` | keep first occurrence per tool name per task; archive rest |
| `Other` | unchanged |

Plus the existing `max_promoted_outputs_per_task` cap (D3) triggers archival proactively when a Task exceeds its budget.

**Rationale:** Differential retention requires differential classification. `Delivered` is irreducible — it IS the answer. `ToolActivity` failures matter more than successes for the Orchestrator's planning, but the first success per tool name is still useful evidence ("the Task did read files, did edit, did run tests").

**Serde compatibility:** new variants are added with `#[serde(default)]` fallback to `Other` for forward compatibility with archived rows that predate this change.

**Alternatives considered:**
- *Reuse `EventKind::Other` with tag-based differentiation.* Rejected — `Other` is a catch-all; compaction would need string matching on text, defeating the type-level classifier purpose.
- *Reuse `EventKind::Highlight` with a new flag.* Rejected — conflates Task-emitted highlights with Orchestrator-derived outcome events, breaking the "highlights come from `report_highlight` tool" invariant.

### D7: Configuration surface

**Choice:** Single new config key under the existing `[orchestrator]` section:

```toml
[orchestrator]
max_promoted_outputs_per_task = 8   # default; range 0..=32
```

Setting it to `0` disables `ToolActivity` promotion entirely (reverting that one piece of behavior to the current drop-everything mode), useful for users on very small context models.

**Rationale:** Different models have wildly different context budgets (Gemini Flash vs Claude Opus). A single knob with a sensible default covers the spectrum without per-variant config explosion.

**Alternatives considered:**
- *Per-variant caps (separate limits for `ToolActivity`, `Delivered`, etc.).* Rejected — YAGNI; the global cap plus differential retention (D6) already handles the cases users care about.
- *Token-budget cap instead of event-count cap.* Rejected — requires a tokeniser call on every promotion, adding per-event cost; event counts are a good enough proxy.

## Risks / Trade-offs

- **[Risk] Orchestrator context growth.** With all pipes connected, a busy session (5 concurrent Tasks × 8 promoted outputs × 200 tokens each) adds ~8K tokens to active memory. → *Mitigation:* `max_promoted_outputs_per_task` cap (D3) + differential retention (D6) + the existing compaction threshold. Configurable to 0 for small-context models. Test: `compaction_respects_promoted_output_cap`.

- **[Risk] `route()` regression.** Adding context-aware tie-breaking changes a hot path. → *Mitigation:* context rules only fire when keyword matching is ambiguous (the cases keyword matching currently gets wrong by definition). All existing `routing.rs` tests stay green unchanged; new tests cover only the tie-break cases. The keyword path remains the default.

- **[Risk] `SubTaskResult` field additions break callers.** → *Mitigation:* all new fields are `Option` or `Vec` (default-empty). Existing deserialisation of old payloads still works. Test: `subtask_result_deserialises_old_format`.

- **[Risk] `EventKind` serde breakage for archived rows.** → *Mitigation:* `#[serde(default)]` fallback to `Other`; integration test loads a pre-change archive snapshot and verifies it parses. The archive is append-only, so old rows are never rewritten in a way that would lose the new variants.

- **[Risk] Non-blocking `try_poll` on join handles leaks unfinished Tasks.** If the Orchestrator shuts down before a Task's join handle is awaited, the Task is orphaned. → *Mitigation:* the existing shutdown sequence already aborts in-flight Tasks; on next startup, orphaned join handles are not restored (in-memory only), so there is no consistency issue. Document this in `docs/architecture.md`.

- **[Trade-off] Sampling policy is heuristic and will sometimes miss signal.** A Task that calls `ReadTool` once on a critical file and never again will have that read promoted, but a Task that calls `ReadTool` 50 times across 50 different files will only show the first. → *Acceptance:* `deep_dive` remains the escape hatch for full detail. The sampling policy optimises for the Orchestrator's planning, not for forensic reconstruction.

- **[Trade-off] Follow-up detection in `route()` uses time + length + word list.** This will occasionally mis-classify (e.g. user types a 200-char follow-up and we miss it). → *Acceptance:* the cost of a wrong Foreground classification is small (Orchestrator answers inline, user can re-dispatch); the cost of a wrong Background classification is also small (Task runs, user waits). Imperfect tie-breaking is strictly better than no tie-breaking.

## Migration Plan

Each phase is independently shippable. Land in this order:

### Phase 1 — Highlight.detail fix (lowest risk, highest embarrassment ratio)
1. Modify `drain_highlights` per D1.
2. Update existing tests that asserted on `"[{tag}] {summary}"` format.
3. Ship.

### Phase 2 — TaskOutcome bridge + EventKind extension
1. Add `EventKind::Delivered` and `EventKind::ToolActivity` per D6.
2. Add `Orchestrator::poll_task_outcome` per D2.
3. Wire into `RealOrchestrator` turn loop.
4. Extend `OrchestratorCompaction` with new retention rules.
5. Ship.

### Phase 3 — TaskOutput sampling
1. Add `promote_output` function per D3.
2. Wire into `drain_outputs` (promote before returning to caller).
3. Add `max_promoted_outputs_per_task` config.
4. Ship.

### Phase 4 — Sub-task enrichment
1. Extend `SubTaskResult` per D4.
2. Update `check_subtask_completions` to populate new fields.
3. Update parent-side steering injection format.
4. Ship (behind existing `sub-agents` feature flag).

### Phase 5 — Context-aware routing
1. Add `RouteContext` and `TaskOutcomeDigest` per D5.
2. Update `route()` signature and rules.
3. Update the single call site in `RealOrchestrator`.
4. Ship.

### Rollback
Each phase reverts cleanly (no on-disk format changes). The `EventKind` serde addition is forward-compatible (old code reads new variants as `Other`).

## Open Questions

1. **Default value for `max_promoted_outputs_per_task`.** 8 is a guess based on "typical Task makes ~12 tool calls, ~half are first-of-name, ~2 fail". Needs validation against real sessions once Phase 3 lands. → *Defer:* ship with 8, revisit after 2 weeks of dogfooding.

2. **Should `Delivered` events also feed the Cold Store?** Currently Cold Store recall is best-effort and the `Delivered` payload is large. Cross-session "what did task X produce last week?" is a compelling use case but doubles the storage footprint. → *Defer:* out of scope for this change; revisit when Cold Store semantic search lands.

3. **`RouteContext` and Cold Store integration.** Should `recent_outcomes` include recalled Cold Store entries, or only active-memory outcomes? Cold Store recall is async and would slow `route()` unacceptably. → *Resolution (proposed):* active-memory only. If the user explicitly says "remember when X happened", the Orchestrator can call `recall` in its reply turn; routing does not.

4. **`artifacts` extraction from inherited workspaces.** When a sub-task inherits the parent's workspace, the `Workspace::ChangeSet` includes both parent's and child's changes. How do we attribute paths to the child only? → *Resolution (proposed):* snapshot the change set at child dispatch time, diff against current state at child completion, attribute the delta to the child. Implementation detail for Phase 4; if it proves brittle, fall back to `artifacts: Vec::new()` (no regression from current state).

5. **Compaction interaction with `Delivered` events across many Tasks.** If 20 Tasks complete in a session, that's 20 `Delivered` events permanently in active memory. At ~500 tokens each, that's 10K tokens just from outcomes. → *Resolution (proposed):* add a sub-cap: keep `Delivered` events for the last N=5 completed Tasks in active memory; older ones get a one-line digest in active and the full payload in archive. Revisit if real usage shows this is too aggressive.
