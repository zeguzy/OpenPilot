## 1. Phase 1 — Highlight.detail propagation (D1)

- [ ] 1.1 Write failing test: `drain_highlights_preserves_detail_field` — emit a `Highlight { detail: Some(...) }`, drain, assert the resulting `OrchestratorEvent.text` contains both summary and detail separated by `\n---\n`
- [ ] 1.2 Write failing test: `drain_highlights_without_detail_uses_legacy_format` — emit a `Highlight { detail: None }`, drain, assert text matches pre-change `"[{tag}] {summary}"` format
- [ ] 1.3 Modify `Orchestrator::drain_highlights` in `orchestrator.rs` to format text per D1 (conditional on `hl.detail.is_some()`)
- [ ] 1.4 Audit existing tests asserting on highlight text format (search for `"["` patterns in orchestrator tests); update expectations to the new detail-aware format
- [ ] 1.5 Verify: `cargo test --workspace` green, `cargo clippy --workspace --all-targets` clean

## 2. Phase 2 — EventKind extension + TaskOutcome bridge (D2, D6)

- [ ] 2.1 Write failing test: `event_kind_delivered_exempt_from_archival` — push a `Delivered` event, run compaction with overflow pressure, assert the `Delivered` event survives in active memory
- [ ] 2.2 Write failing test: `event_kind_tool_activity_retention_rule` — push 5 `ToolActivity { success: false }` events for one Task, run compaction, assert only the last 3 remain in active
- [ ] 2.3 Write failing test: `event_kind_tool_activity_success_keeps_first_per_tool` — push 3 `ToolActivity { success: true }` events for tool `ReadTool` and 2 for `EditTool`, run compaction, assert 1 ReadTool + 1 EditTool survive
- [ ] 2.4 Write failing test: `event_kind_serde_back_compat` — deserialise an archived row from before this change (no `Delivered`/`ToolActivity` variants), assert it parses with unknown variants falling back to `Other`
- [ ] 2.5 Add `EventKind::Delivered` and `EventKind::ToolActivity { success: bool }` variants to `memory/compact.rs` with `#[serde(default)]` fallback
- [ ] 2.6 Extend `OrchestratorCompaction::compact` in `memory/compact.rs` with the retention rules from D6 (Delivered never archived; ToolActivity failures keep last 3; ToolActivity successes keep first-per-tool-name)
- [ ] 2.7 Update `kind_tag()` to emit `"delivered"`, `"tool-activity-fail"`, `"tool-activity-success"` for the new variants
- [ ] 2.8 Write failing test: `poll_task_outcome_returns_completed_when_resolved` — spawn a Task whose join handle resolves to `Completed(msg)`, call `poll_task_outcome`, assert `Some(Completed(msg))` and a `Delivered` event in memory
- [ ] 2.9 Write failing test: `poll_task_outcome_returns_none_when_unresolved` — spawn a Task whose join handle is still pending, call `poll_task_outcome`, assert `None` and the join handle remains in the registry
- [ ] 2.10 Write failing test: `poll_task_outcome_idempotent_after_claim` — call `poll_task_outcome` twice on a resolved Task, assert the second call returns `None` and no duplicate `Delivered` event
- [ ] 2.11 Implement `Orchestrator::poll_task_outcome(task_id) -> Result<Option<TaskOutcome>>` using `tokio::task::UnboundedReceiver`-style non-blocking join handle polling (or `JoinHandle::is_finished()` + `try_join`)
- [ ] 2.12 Wire `poll_task_outcome` into `RealOrchestrator`'s turn loop: for every Task whose status is terminal but whose outcome hasn't been recorded, call `poll_task_outcome` before the Orchestrator's LLM call
- [ ] 2.13 Verify: `cargo test --workspace` green (including new tests), `cargo clippy --workspace --all-targets` clean

## 3. Phase 3 — TaskOutput sampling policy (D3, D7)

- [ ] 3.1 Write failing test: `promote_output_tool_failure_always` — feed `ToolResult { success: false, .. }`, assert `Some(PromotedOutput)` returned
- [ ] 3.2 Write failing test: `promote_output_first_tool_call_per_name` — feed `ToolCall { name: "ReadTool" }` twice, assert first returns `Some`, second returns `None` (de-dup via `seen_tools`)
- [ ] 3.3 Write failing test: `promote_output_text_delta_never` — feed `TextDelta("...")`, assert `None`
- [ ] 3.4 Write failing test: `promote_output_thinking_delta_never` — feed `ThinkingDelta("...")`, assert `None`
- [ ] 3.5 Write failing test: `promote_output_highlight_and_status_ignored_here` — feed `Highlight(...)` and `StatusChanged { .. }`, assert `None` (these have their own channels)
- [ ] 3.6 Write failing test: `promoted_output_cap_triggers_archival` — push 9 promotable outputs for one Task with cap=8, assert the oldest `ToolActivity` event is archived and the new one takes its place
- [ ] 3.7 Write failing test: `cap_zero_disables_promotion` — set `max_promoted_outputs_per_task = 0`, feed promotable outputs, assert none enter memory
- [ ] 3.8 Implement `promote_output(out: &TaskOutput, seen_tools: &mut HashSet<String>) -> Option<PromotedOutput>` as a pure function (no Orchestrator state)
- [ ] 3.9 Extend `TaskEntry` with `seen_tools: HashSet<String>` and `promoted_count: usize` fields (default empty)
- [ ] 3.10 Modify `Orchestrator::drain_outputs` to call `promote_output` per item and push resulting `OrchestratorEvent`s into memory (in addition to returning the raw stream to the caller)
- [ ] 3.11 Add `max_promoted_outputs_per_task` to config schema in `config.rs` with default 8, range 0..=32
- [ ] 3.12 Wire the config value into `Orchestrator::new` and pass through to `drain_outputs` / the cap enforcement path
- [ ] 3.13 Verify: `cargo test --workspace` green, `cargo clippy --workspace --all-targets` clean, config loading test asserts default and override

## 4. Phase 4 — Sub-task result enrichment (D4)

- [ ] 4.1 Write failing test: `subtask_result_default_fields_backward_compatible` — deserialise a `SubTaskResult` JSON lacking the new fields, assert `final_message == None`, `highlights == vec![]`
- [ ] 4.2 Write failing test: `subtask_result_enriched_on_delivered` — child Task reaches Delivered with `Completed(msg)`, 2 highlights, 3 artifacts; assert `SubTaskResult` carries all three new fields populated
- [ ] 4.3 Write failing test: `subtask_result_minimal_on_stuck` — child reaches Stuck; assert `SubTaskNotification::Failed` is constructed with reason string and no `SubTaskResult`
- [ ] 4.4 Write failing test: `parent_receives_multiline_injection_block` — drain the parent's notification queue, assert the injected `Message::user(...)` text contains `final_message:`, `highlights:`, `artifacts:` sections when non-empty
- [ ] 4.5 Write failing test: `parent_receives_compact_block_when_fields_empty` — child Delivered with no highlights and no artifacts; assert the injected block omits the empty sections
- [ ] 4.6 Extend `SubTaskResult` struct in `sub_agent/dispatch.rs` with `final_message: Option<String>`, `highlights: Vec<Highlight>`, `artifacts: Vec<PathBuf>`; add `#[serde(default)]` on each new field
- [ ] 4.7 Modify `Orchestrator::check_subtask_completions` to populate the new fields: pull `final_message` from the awaited `TaskOutcome` (via `poll_task_outcome`), copy `highlights` from `TaskEntry.highlights`, extract `artifacts` from `Workspace::ChangeSet`
- [ ] 4.8 Update the `inject_text` formatting in `check_subtask_completions` to emit the multi-line structured block per D4
- [ ] 4.9 Implement artifact extraction: snapshot `Workspace::ChangeSet` at child dispatch time, diff against current state at child completion, attribute the delta path list to the child
- [ ] 4.10 Add fallback: if artifact extraction fails (e.g. workspace strategy doesn't expose ChangeSet), `artifacts` falls back to `Vec::new()` (no regression from current behavior)
- [ ] 4.11 Verify: `cargo test --workspace --features sub-agents` green, `cargo clippy --workspace --all-targets --features sub-agents` clean

## 5. Phase 5 — Context-aware routing (D5)

- [ ] 5.1 Write failing test: `route_context_default_does_not_change_keyword_behavior` — call `route(msg, &RouteContext::default())` for all existing routing test cases, assert identical results to pre-change
- [ ] 5.2 Write failing test: `route_followup_after_recent_dispatch_routes_foreground` — `RouteContext { last_dispatched_at: Some(30s ago), .. }` + message "那顺便也把测试加上" (< 100 chars, starts with follow-up word), assert `Foreground`
- [ ] 5.3 Write failing test: `route_followup_expired_dispatch_keeps_keyword_behavior` — same message but `last_dispatched_at: Some(5min ago)`, assert keyword-decided `Background` (time window expired)
- [ ] 5.4 Write failing test: `route_recent_failure_overlap_routes_background` — `RouteContext { recent_outcomes: vec![stuck with summary "auth login bug"] }` + message "再修一下 login 那块" (bigram overlap > 0.3), assert `Background` with inherited focus
- [ ] 5.5 Write failing test: `route_no_overlap_with_failure_keeps_keyword_behavior` — stuck outcome summary "auth login bug" + message "帮我写个文档" (no bigram overlap), assert keyword-decided result
- [ ] 5.6 Write failing test: `route_long_followup_does_not_trigger_rule_1` — message > 100 chars starting with follow-up word, assert keyword-decided result (length guard)
- [ ] 5.7 Add `RouteContext` and `TaskOutcomeDigest` structs to `orchestrator/routing.rs`
- [ ] 5.8 Modify `route(message, ctx)` signature: remove underscore on `context`, accept `&RouteContext`
- [ ] 5.9 Implement the three tie-breaking rules from D5 inside `route`, applied only when keyword matching is ambiguous (both flags true, or both false)
- [ ] 5.10 Update the single call site in `crates/opca-cli/src/real.rs` to construct `RouteContext` from Orchestrator state (recent outcomes from registry, last dispatch timestamp, pending count)
- [ ] 5.11 Verify: `cargo test --workspace` green (all pre-existing routing tests unchanged + new tie-break tests pass), `cargo clippy --workspace --all-targets` clean

## 6. Documentation & final verification

- [ ] 6.1 Update `docs/architecture.md`: add a subsection under Orchestrator describing the four result-propagation pipes and the sampling policy; reference the design.md decisions
- [ ] 6.2 Update `docs/configuration.md`: document `max_promoted_outputs_per_task` under the `[orchestrator]` section with default, range, and behavior at 0
- [ ] 6.3 Update `AGENTS.md` module map if any new files were added (none expected — all changes are in existing modules)
- [ ] 6.4 Run full verification suite: `cargo fmt --all -- --check` (no-op), `cargo clippy --workspace --all-targets --features sub-agents` (clean), `cargo test --workspace --features sub-agents` (green)
- [ ] 6.5 Run `openspec validate wire-task-result-propagation --strict` and resolve any reported inconsistencies between proposal / design / specs / tasks
- [ ] 6.6 Run `openspec status --change "wire-task-result-propagation"` and confirm `isComplete: true`
