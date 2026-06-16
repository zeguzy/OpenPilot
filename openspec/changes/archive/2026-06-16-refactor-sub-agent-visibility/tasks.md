## 1. Phase 1: Orchestrator dispatch + Registry

- [x] 1.1 Add `subtask_request_queue: Arc<Mutex<Vec<SubTaskRequest>>>` and `subtask_notifications: HashMap<String, SubTaskNotificationQueue>` to Orchestrator. Initialize in `new()`.
- [x] 1.2 Add `drain_subtask_requests(&mut self)` method to Orchestrator: iterate queue, for each request call `dispatch_task(description, focus, [], Some(parent_id))`, create notification queue keyed by parent_id, store child id in pending set.
- [x] 1.3 Add `check_subtask_completions(&mut self)` method: for each parent with pending subtasks, check if child reached terminal state (Delivered/Stuck/Error). If so, construct notification message, inject into parent steering via `inject_message`, push `SubTaskNotification` to queue.
- [x] 1.4 Wire `drain_subtask_requests()` and `check_subtask_completions()` into the poll loop (real.rs `poll_loop` or Orchestrator's drain methods).
- [x] 1.5 Add `pending_subtasks: HashSet<String>` to TaskEntry to track which child IDs a parent is waiting for.

## 2. Phase 2: ToolContext + DispatchSubtaskTool fix

- [x] 2.1 Add `task_id: Option<String>` field to `ToolContext`. Update `dispatch_task` in Orchestrator to set it when creating the ToolContext.
- [x] 2.2 Fix `DispatchSubtaskTool::execute()`: read `ctx.task_id` to populate `SubTaskRequest.parent_id` (currently always empty string).
- [x] 2.3 Update `DispatchSubtaskTool::execute()` return message to include ticket ID format: `"Sub-task dispatched (ticket: subtask-N). You will be notified when it completes."`.

## 3. Phase 3: Run loop async refactor

- [x] 3.1 Remove `drain_and_run_subtasks()` synchronous execution from run.rs.
- [x] 3.2 Add `drain_subtask_notifications(&mut self)` to run.rs: drain notification queue (held by Task), inject each as `Message::user("[Sub-task result] ...")` into active, track completed count.
- [x] 3.3 Add Waiting-for-subtask logic: after `run_turn`, if Task has pending subtasks (check via stored pending set) and turn produced no tool calls, transition to Waiting with heartbeat "waiting for N subtask(s)".
- [x] 3.4 Add Waiting timeout: track `waiting_since: Option<Instant>` for subtask waiting (reuse existing clarification waiting_since if present, or add separate field). On timeout (default 5 min), inject "[Sub-task timeout]" message and transition back to OnIt.
- [x] 3.5 Task struct: add `subtask_notification_queue: Option<SubTaskNotificationQueue>` and `pending_subtask_count: usize` fields (behind `#[cfg(feature = "sub-agents")]`).

## 4. Phase 4: Continuation awareness

- [x] 4.1 In `check_continuations` (real.rs), skip evaluation for tasks whose heartbeat status is Waiting AND have pending subtasks. These tasks are intentionally waiting, not "delivered and ready for continuation evaluation".

## 5. Phase 5: Testing

- [x] 5.1 Unit test: DispatchSubtaskTool enqueues request with correct parent_id from ToolContext.
- [x] 5.2 Unit test: Orchestrator drain_subtask_requests creates child Task with parent_task_id set.
- [x] 5.3 Unit test: check_subtask_completions injects notification when child reaches Delivered.
- [x] 5.4 Integration test: parent dispatch → child run (ScriptedProvider) → notification → parent wakes from Waiting → result in active messages.
- [x] 5.5 Integration test: parent dispatch → child Stuck → parent wakes with error message.
- [x] 5.6 E2E test: 2 children dispatched in one batch → parent Waiting → first child completes → parent still Waiting → second child completes → parent resumes.
- [x] 5.7 Test: Waiting timeout fires after configured duration, parent resumes with timeout message.

## 6. Phase 6: Cleanup

- [x] 6.1 Remove dead code: old `drain_and_run_subtasks()`, unused `block_in_place` import, old subtask sync test.
- [x] 6.2 Verify `cargo build --workspace --features sub-agents` and `cargo test --workspace --features sub-agents` pass.
- [x] 6.3 Verify `cargo clippy --workspace --all-targets --features sub-agents` is clean.
