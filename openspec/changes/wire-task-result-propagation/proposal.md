## Why

Code exploration of the Orchestrator's result-handling paths uncovered **four disconnected pipes**: Task-side already produces rich, structured results, but the Orchestrator only consumes a thin slice of each. The codebase defines `Highlight.detail`, the `TaskOutput` stream, `TaskOutcome::Completed(Message)`, and a `route(message, _context)` parameter — **all four are dropped on the floor**. The Orchestrator's next-turn context therefore sees roughly ~25 tokens of actual Task output for every ~3000 tokens the Task produced, and routing decisions are made purely from keyword matching against the latest user message with zero memory of prior Task outcomes.

This is not a compression problem (the previous `refactor-orchestrator-prompt-system` change addressed prompt-side structure); it is a **wiring problem**. The structures exist; the wires are not connected. Fixing it requires no new types — only connecting existing pipes and deciding the sampling policy for the one channel (TaskOutput) whose full stream would overwhelm the Orchestrator's context window.

## What Changes

### Bug fixes (pipes defined but not connected)

- **FIX: `Highlight.detail` propagation** — `Orchestrator::drain_highlights` currently formats events as `"[{tag}] {summary}"`, silently dropping `hl.detail`. The OrchestratorEvent text must include the detail payload (when present) so Task-side `report_highlight` calls actually deliver their full finding to the main brain.
- **FIX: `TaskOutcome` → memory bridge** — `TaskEntry.join_handle: Option<JoinHandle<TaskOutcome>>` is stored but never awaited into an `OrchestratorEvent`. A new Orchestrator method awaits the join handle on terminal transitions and pushes `TaskOutcome::Completed(Message)` into memory as a dedicated `EventKind::Delivered` (or equivalent) event, replacing the current indirect signal via `task_completed: true` highlight side-channel.
- **FIX: `route()` honors its `_context` parameter** — the leading underscore is removed; the function signature becomes `route(message, context)` and the body uses `context` (the Orchestrator's active-memory snapshot) to break ties when keyword matching is ambiguous (e.g. user follows up "那顺便也把测试加上" right after a Task dispatch).

### Design decisions (policies, not bugs)

- **DESIGN: `TaskOutput` sampling policy** — the output channel (`TextDelta` / `ThinkingDelta` / `ToolCall` / `ToolResult` / `Highlight` / `StatusChanged` / `Done`) is far too voluminous to push verbatim into Orchestrator memory. A sampling rule decides which variants are promoted to `OrchestratorEvent`:
  - `ToolResult { success: false, .. }` → always promoted (failures are decision-relevant)
  - `ToolCall { .. }` → promoted only when the call is the Nth of its kind (de-dup by name)
  - `TextDelta` / `ThinkingDelta` → never promoted (stream-only; available via `deep_dive`)
  - `Done` → implied by `TaskOutcome` bridge above; no separate event
  - configurable cap per Task to bound Orchestrator context growth
- **DESIGN: Sub-task result enrichment** — `SubTaskResult.summary` currently clones only the child's `latest_heartbeat.summary` (one short string). Enrichment adds:
  - the child's final `TaskOutcome::Completed(Message)` text (via the new bridge above)
  - the child's recorded `highlights` (already in the registry, just not copied)
  - a list of artifact paths (currently hard-coded `artifacts: Vec::new()`; wired through the `Workspace::ChangeSet` summary the child already produces)
- **DESIGN: Orchestrator context-budget guard** — with all four pipes connected, the Orchestrator's active memory can grow O(thousands of tokens) per Task. A hard cap (configurable, default 8 Promoted Outputs per Task retained in active memory; older promoted outputs are compacted into the archive by the existing `OrchestratorCompaction` strategy) prevents context-window blowup. This is the Focus Contract's missing twin for outcomes.

### Out of scope

- Does NOT change the `Memory<T>` data structure (reuses existing `OrchestratorEvent` shape; only adds new `EventKind` variants if needed).
- Does NOT touch provider integration, workspace isolation, or prompt templates.
- Does NOT redesign the three-layer context model (heartbeat / highlight / deep_dive stay); only changes what flows through layers 1 and 2.

## Capabilities

### New Capabilities

(None — all changes extend existing capabilities.)

### Modified Capabilities

- `orchestrator-core`: ADD requirements for (a) `Highlight.detail` propagation in `drain_highlights`, (b) `TaskOutcome` → memory bridge on terminal transition, (c) `route()` consuming a context snapshot parameter, (d) bounded promotion of `TaskOutput` variants into Orchestrator memory.
- `task-context-layering`: MODIFY the Layer 1/Layer 2 contract to formalize what a Highlight carries (summary + optional detail, both delivered to the Orchestrator) and what Layer 3 (`deep_dive`) remains responsible for (full ToolCall/ToolResult stream, never auto-promoted).
- `sub-agent-async`: MODIFY sub-task completion to deliver the child's `TaskOutcome`, recorded highlights, and artifact paths to the parent (not just `latest_heartbeat.summary`).

## Impact

### Code

- `crates/opca-core/src/orchestrator/orchestrator.rs`: modify `drain_highlights`, `drain_outputs`, add `TaskOutcome` await bridge, add `promote_output` helper.
- `crates/opca-core/src/orchestrator/routing.rs`: change `route` signature, implement context-aware tie-breaking.
- `crates/opca-core/src/sub_agent/`: enrich `SubTaskResult`, propagate artifacts.
- `crates/opca-core/src/memory/compact.rs`: extend `OrchestratorCompaction` to handle promoted-output events (new tag, new retention rule).
- `crates/opca-core/src/lifecycle/heartbeat.rs`: no schema change; documentation only.

### APIs

- **BREAKING (internal)**: `route(message: &str, _context: &str)` → `route(message: &str, context: &RouteContext)` where `RouteContext` is a small struct carrying the Orchestrator's active-memory digest (recent task outcomes, pending tasks count, last dispatched task id). All call sites updated.
- **BREAKING (internal)**: `SubTaskResult` gains non-optional `final_message: Option<String>` and `highlights: Vec<Highlight>` and `artifacts: Vec<PathBuf>` (the last transitions from always-empty to populated).
- New `Orchestrator::await_task_outcome(task_id) -> Result<Option<TaskOutcome>>` method.

### Dependencies

- None added. All changes use existing `tokio`, `serde`, `thiserror`.

### Configuration

- `.agent/config.toml` gains:
  - `[orchestrator]` section: `max_promoted_outputs_per_task = 8` (cap on TaskOutput-derived OrchestratorEvents retained in active memory per Task)

### Migration

- No on-disk format changes (OrchestratorEvent is in-memory only).
- `route()` callers (currently only `RealOrchestrator`) are updated atomically with the signature change.
- Existing tests that asserted on `"[{tag}] {summary}"` format are updated to the new detail-aware format.

### Effort

- ~3-5 days. The Highlight.detail fix and TaskOutcome bridge are small (1 day each, mostly tests). The output sampling policy and sub-task enrichment are the bulk (1-2 days). Routing context is the riskiest because it changes a hot path (0.5-1 day plus load testing).
