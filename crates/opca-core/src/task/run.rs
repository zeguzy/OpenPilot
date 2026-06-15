use std::collections::HashMap;
use std::path::PathBuf;

use tokio_stream::StreamExt;

use crate::focus::Severity;
use crate::lifecycle::TaskStatus;
use crate::provider::{Message, ProviderEvent, ToolCall};
use crate::tools::dispatch::dispatch_batch;

use super::channels::{SteeringMessage, TaskOutput};
use super::evidence_gate::{ErrorKind, EvidenceGate};
use super::task::{Task, TaskOutcome};

const MAX_TURNS: u64 = 50;

/// The four phases of a Task's lifecycle.
///
/// See `design.md` §D2 for the hybrid enforcement rationale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Intent Gate — classify the request.
    Zero,
    /// Codebase Assessment — sample files, classify project state.
    One,
    /// Implementation — make changes, run tools.
    Two,
    /// Completion — Evidence Gate runs, then Delivered or back to Two.
    Three,
}

impl Phase {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Zero => "0",
            Self::One => "1",
            Self::Two => "2",
            Self::Three => "3",
        }
    }
}

/// Classification of project codebase discipline (Phase 1 output).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssessmentState {
    Disciplined,
    Transitional,
    Legacy,
    Greenfield,
}

/// Result of the Phase 1 codebase assessment.
#[derive(Debug, Clone)]
pub struct Assessment {
    pub state: AssessmentState,
    pub sampled_files: Vec<PathBuf>,
    pub notes: String,
}

/// A single todo item for multi-step Task tracking (G7 — type defined
/// now, wired later).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodoItem {
    pub content: String,
    pub status: String,
    pub priority: String,
}

/// Signature for grouping consecutive failures (3-strike rule, G8).
///
/// Two failures with the same signature (same file, same error kind,
/// same normalised message hash) count as the same issue.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct IssueSignature {
    pub file: PathBuf,
    pub kind: ErrorKind,
    pub msg_hash: u64,
}

/// Mutable state carried through the Task run loop.
///
/// Tracks the current phase, codebase assessment, evidence gate, and
/// 3-strike counters. Lives on [`Task`] as `run_state`.
#[derive(Debug)]
pub struct RunState {
    pub current_phase: Phase,
    pub codebase_assessment: Option<Assessment>,
    pub three_strike_counters: HashMap<IssueSignature, u8>,
    pub evidence_gate: Option<EvidenceGate>,
    pub todo_list: Vec<TodoItem>,
    turns_in_phase: u64,
}

impl Default for RunState {
    fn default() -> Self {
        Self::new()
    }
}

impl RunState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            current_phase: Phase::Zero,
            codebase_assessment: None,
            three_strike_counters: HashMap::new(),
            evidence_gate: None,
            todo_list: Vec::new(),
            turns_in_phase: 0,
        }
    }

    pub fn set_evidence_gate(&mut self, gate: EvidenceGate) {
        self.evidence_gate = Some(gate);
    }

    /// Records evidence-gate failures into the 3-strike counters.
    ///
    /// Returns `Some(reason)` when any counter reaches
    /// [`THREE_STRIKE_LIMIT`], signalling the Task should transition to
    /// Stuck. Returns `None` otherwise (under the threshold).
    #[must_use]
    pub fn record_evidence_failure(
        &mut self,
        errors: &[super::evidence_gate::ErrorEntry],
    ) -> Option<String> {
        let mut max_count: u8 = 0;
        let mut worst_msg: Option<String> = None;

        for entry in errors {
            let sig = IssueSignature {
                file: entry.file.clone().unwrap_or_default(),
                kind: entry.kind,
                msg_hash: normalize_error_msg(&entry.message),
            };
            let counter = self.three_strike_counters.entry(sig).or_insert(0);
            *counter += 1;
            if *counter > max_count {
                max_count = *counter;
                worst_msg = Some(entry.message.chars().take(120).collect());
            }
        }

        if max_count >= THREE_STRIKE_LIMIT {
            worst_msg.map(|m| format!("Same issue hit {max_count} times: {m}"))
        } else {
            None
        }
    }

    fn transition_to(&mut self, phase: Phase) {
        if self.current_phase != phase {
            self.current_phase = phase;
            self.turns_in_phase = 0;
        }
    }

    const fn tick_phase_turn(&mut self) {
        self.turns_in_phase += 1;
    }
}

/// Normalises an error message by stripping line numbers and absolute
/// paths, then returns an FNV-1a hash.
///
/// Two messages that differ only by line number produce the same hash,
/// so `src/lib.rs:42` and `src/lib.rs:67` map to the same value.
#[must_use]
pub fn normalize_error_msg(msg: &str) -> u64 {
    let normalized = strip_line_numbers(msg);
    fnv1a(&normalized)
}

fn strip_line_numbers(msg: &str) -> String {
    let mut result = String::with_capacity(msg.len());
    let mut chars = msg.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == ':' {
            let mut lookahead = chars.clone();
            if lookahead.peek().is_some_and(char::is_ascii_digit) {
                while lookahead.peek().is_some_and(char::is_ascii_digit) {
                    lookahead.next();
                }
                if lookahead.peek() == Some(&':') {
                    lookahead.next();
                    while lookahead.peek().is_some_and(char::is_ascii_digit) {
                        lookahead.next();
                    }
                }
                chars = lookahead;
                result.push(':');
                result.push_str("<n>");
            } else {
                result.push(':');
            }
        } else {
            result.push(ch);
        }
    }
    result.replace("/Users/", "~/").replace("/home/", "~/")
}

fn fnv1a(s: &str) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0000_0100_0000_01b3;
    let mut hash = FNV_OFFSET_BASIS;
    for byte in s.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SteeringOutcome {
    Continue,
    Cancelled,
}

impl Task {
    pub async fn run(&mut self, initial_input: &str) -> TaskOutcome {
        if let Err(e) = self.begin_lifecycle() {
            return TaskOutcome::Error(e);
        }
        self.active.push(Message::user(initial_input));

        if let Some(gate) = &mut self.run_state.evidence_gate {
            if let Err(e) = gate.capture_baseline(self.workspace.path()) {
                tracing::warn!(task_id = %self.id, error = %e, "evidence gate baseline capture failed; gate disabled");
                self.run_state.evidence_gate = None;
            }
        }

        loop {
            self.turn_count += 1;
            if self.turn_count > MAX_TURNS {
                return TaskOutcome::Error(format!("exceeded max turns ({MAX_TURNS})"));
            }

            if self.process_steering() == SteeringOutcome::Cancelled {
                let _ = self
                    .lifecycle
                    .transition(TaskStatus::Axed, 0.0, "cancelled");
                return TaskOutcome::Cancelled;
            }

            self.drain_followups();

            match self.run_turn().await {
                Ok(TurnVerdict::Continue) => {}
                Ok(TurnVerdict::Completed(msg)) => {
                    let _ = self
                        .lifecycle
                        .transition(TaskStatus::Delivered, 1.0, "delivered");
                    self.push_output(TaskOutput::StatusChanged {
                        status: TaskStatus::Delivered,
                        progress: 1.0,
                        summary: "delivered".to_string(),
                    });
                    self.push_output(TaskOutput::Done);
                    return TaskOutcome::Completed(msg);
                }
                Ok(TurnVerdict::Stuck(reason)) => {
                    let _ = self.lifecycle.transition(TaskStatus::Stuck, 0.0, &reason);
                    self.push_output(TaskOutput::StatusChanged {
                        status: TaskStatus::Stuck,
                        progress: 0.0,
                        summary: reason.clone(),
                    });
                    self.push_output(TaskOutput::Done);
                    return TaskOutcome::Error(reason);
                }
                Err(e) => {
                    let _ = self.lifecycle.transition(TaskStatus::Stuck, 0.0, "error");
                    self.push_output(TaskOutput::Done);
                    return TaskOutcome::Error(e);
                }
            }
        }
    }

    fn begin_lifecycle(&mut self) -> Result<(), String> {
        self.lifecycle
            .transition(TaskStatus::Waking, 0.0, "waking up")
            .map_err(|e| e.to_string())?;
        self.lifecycle
            .transition(TaskStatus::Pondering, 0.0, "pondering")
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    async fn run_turn(&mut self) -> Result<TurnVerdict, String> {
        let system_prompt = self.build_system_prompt();
        let tools = self.tools.definitions();

        let stream = {
            let messages = self.active.clone();
            let provider = self.provider.clone();
            let prompt = if system_prompt.is_empty() {
                None
            } else {
                Some(system_prompt.as_str())
            };
            provider
                .stream(&messages, &tools, prompt)
                .await
                .map_err(|e| e.to_string())?
        };

        let (text, tool_calls, err) = self.collect_stream(stream).await;
        if let Some(err_msg) = err {
            return Err(err_msg);
        }

        let assistant_msg = Message::assistant_with_tools(text.clone(), tool_calls.clone());
        self.active.push(assistant_msg);

        if self.lifecycle.current() == TaskStatus::Pondering {
            let label = if tool_calls.is_empty() {
                "producing response"
            } else {
                "executing tools"
            };
            let _ = self.lifecycle.transition(TaskStatus::OnIt, 0.1, label);
        }

        if !tool_calls.is_empty() {
            self.handle_tool_call_turn(tool_calls).await;
            return Ok(TurnVerdict::Continue);
        }

        // ── Text-only turn: phase advancement + evidence gate ────────
        self.advance_to_phase_three();

        // ── Evidence Gate (G3.6 + G8.4) ──────────────────────────────
        match self.check_evidence_gate() {
            GateOutcome::Proceed => {}
            GateOutcome::Retry => return Ok(TurnVerdict::Continue),
            GateOutcome::Stuck(reason) => return Ok(TurnVerdict::Stuck(reason)),
        }

        self.on_turn_complete(&text);

        if !self.followup.is_empty() {
            if self.lifecycle.current() == TaskStatus::OnIt {
                let _ =
                    self.lifecycle
                        .transition(TaskStatus::Pondering, 0.3, "processing follow-up");
            }
            return Ok(TurnVerdict::Continue);
        }

        Ok(TurnVerdict::Completed(Message::assistant(text)))
    }

    async fn handle_tool_call_turn(&mut self, tool_calls: Vec<ToolCall>) {
        if self.run_state.current_phase == Phase::Zero {
            self.transition_phase(Phase::One, "first tool call");
        }

        let has_assessment = tool_calls.iter().any(|tc| {
            tc.name == "report_highlight"
                && tc
                    .arguments
                    .get("tag")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|t| t == "assessment")
        });

        if has_assessment && self.run_state.current_phase == Phase::One {
            self.transition_phase(Phase::Two, "assessment emitted");
        }

        self.execute_tools(tool_calls).await;
        self.sync_todos();

        if self.run_state.current_phase == Phase::One {
            self.run_state.tick_phase_turn();
            if self.run_state.turns_in_phase >= 1 {
                self.transition_phase(Phase::Two, "auto-advance after assessment window");
            }
        }

        if self.check_clarification_requested() {
            return;
        }

        self.push_heartbeat(0.5, "executed tools, continuing");
    }

    fn advance_to_phase_three(&mut self) {
        match self.run_state.current_phase {
            Phase::Zero => {
                self.transition_phase(Phase::One, "text response, advancing");
                self.transition_phase(Phase::Two, "text response, advancing");
            }
            Phase::One => {
                self.transition_phase(Phase::Two, "text response, advancing");
            }
            Phase::Two | Phase::Three => {}
        }
        self.transition_phase(Phase::Three, "text-only response");
    }

    fn check_evidence_gate(&mut self) -> GateOutcome {
        let Some(gate) = &self.run_state.evidence_gate else {
            return GateOutcome::Proceed;
        };
        match gate.verify(self.workspace.path()) {
            Ok(()) => {
                self.emit_highlight(
                    "evidence-gate",
                    Severity::Info,
                    "Evidence Gate passed",
                    "All evidence commands succeeded.",
                );
                GateOutcome::Proceed
            }
            Err(failure) => {
                let summary = failure.summary();
                self.emit_highlight(
                    "evidence-gate",
                    Severity::Warning,
                    "Evidence Gate failed",
                    &summary,
                );

                if let Some(stuck_reason) = self.process_three_strike(&failure) {
                    return GateOutcome::Stuck(stuck_reason);
                }

                self.active.push(Message::user(format!(
                    "Evidence Gate FAILED. Fix these errors and try again:\n{summary}"
                )));
                self.transition_phase(Phase::Two, "evidence gate failure");
                self.push_heartbeat(0.3, "evidence gate failed, returning to implementation");
                GateOutcome::Retry
            }
        }
    }

    fn process_three_strike(
        &mut self,
        failure: &super::evidence_gate::EvidenceFailure,
    ) -> Option<String> {
        self.run_state.record_evidence_failure(&failure.new_errors)
    }

    /// Checks whether `request_clarification` was called during the last
    /// tool batch. If so, transitions to `Waiting`, pushes a heartbeat
    /// with the question, and returns `true` to signal the caller should
    /// yield back to the run loop's steering poll.
    fn check_clarification_requested(&mut self) -> bool {
        let request = {
            let mut guard = self
                .clarification_store
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            guard.take()
        };
        let Some(req) = request else {
            return false;
        };

        let opts = if req.options.is_empty() {
            String::new()
        } else {
            format!(" (options: {})", req.options.join(", "))
        };
        let summary = format!("Waiting for clarification: {}{}", req.question, opts);
        let _ = self
            .lifecycle
            .transition(TaskStatus::Waiting, 0.0, &summary);
        self.push_heartbeat(0.0, &summary);
        true
    }

    async fn execute_tools(&mut self, tool_calls: Vec<ToolCall>) {
        let results = dispatch_batch(&self.tools, &tool_calls, &self.tool_ctx).await;
        for (call_id, result) in results {
            let tool_result = match result {
                Ok(r) => r,
                Err(e) => crate::provider::ToolResult {
                    content: e.to_string(),
                    is_error: true,
                },
            };
            let name = tool_calls
                .iter()
                .find(|tc| tc.id == call_id)
                .map(|tc| tc.name.clone())
                .unwrap_or_default();
            self.push_output(TaskOutput::ToolResult {
                name,
                success: !tool_result.is_error,
                summary: tool_result.content.chars().take(200).collect(),
            });
            self.active.push(Message::tool_result(call_id, tool_result));
        }
    }

    fn on_turn_complete(&self, text: &str) {
        let summary = if text.is_empty() {
            "turn complete"
        } else {
            text
        };
        self.push_heartbeat(0.8, summary);
    }

    fn transition_phase(&mut self, to: Phase, reason: &str) {
        let from = self.run_state.current_phase;
        if from == to {
            return;
        }
        self.run_state.transition_to(to);
        let summary = format!("Phase {}→{}: {}", from.label(), to.label(), reason);
        let detail = format!(
            "Transitioned from Phase {} to Phase {} ({reason})",
            from.label(),
            to.label()
        );
        let highlight = crate::focus::Highlight::new("phase-transition", Severity::Info, &summary)
            .with_detail(&detail);
        let _ = self.highlight_tx.send(highlight);
        self.push_heartbeat(0.4, &summary);
    }

    fn emit_highlight(&self, tag: &str, severity: Severity, summary: &str, detail: &str) {
        let hl = crate::focus::Highlight::new(tag, severity, summary).with_detail(detail);
        let _ = self.highlight_tx.send(hl);
    }

    fn process_steering(&mut self) -> SteeringOutcome {
        while let Ok(msg) = self.steering_rx.try_recv() {
            match msg {
                SteeringMessage::Cancel => return SteeringOutcome::Cancelled,
                SteeringMessage::Inject(m) => {
                    if self.lifecycle.current() == TaskStatus::Stuck {
                        self.run_state.three_strike_counters.clear();
                        let _ = self.lifecycle.transition(
                            TaskStatus::OnIt,
                            0.2,
                            "resumed after user injection",
                        );
                    } else if self.lifecycle.current() == TaskStatus::Waiting {
                        let _ = self.lifecycle.transition(
                            TaskStatus::OnIt,
                            0.2,
                            "resumed after clarification answer",
                        );
                    }
                    self.active.push(m);
                }
                SteeringMessage::UpdateFocus(update) => {
                    let _ = update.apply(&mut self.focus);
                }
            }
        }
        SteeringOutcome::Continue
    }

    async fn collect_stream(
        &self,
        mut stream: crate::provider::ProviderStream,
    ) -> (String, Vec<ToolCall>, Option<String>) {
        let mut text = String::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut in_flight: HashMap<String, (String, String)> = HashMap::new();
        let mut err: Option<String> = None;

        while let Some(event) = stream.next().await {
            match event {
                Ok(ProviderEvent::TextDelta(delta)) => {
                    text.push_str(&delta);
                    self.push_output(TaskOutput::TextDelta(delta));
                }
                Ok(ProviderEvent::ToolCallStart { id, name }) => {
                    in_flight.insert(id, (name, String::new()));
                }
                Ok(ProviderEvent::ToolCallArgs { id, args }) => {
                    if let Some(entry) = in_flight.get_mut(&id) {
                        entry.1.push_str(&args);
                    }
                }
                Ok(ProviderEvent::ToolCallEnd { id }) => {
                    if let Some((name, raw_args)) = in_flight.remove(&id) {
                        let arguments = serde_json::from_str(&raw_args)
                            .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new()));
                        tool_calls.push(ToolCall {
                            id: id.clone(),
                            name: name.clone(),
                            arguments: arguments.clone(),
                        });
                        self.push_output(TaskOutput::ToolCall {
                            name,
                            args: arguments.to_string(),
                        });
                    }
                }
                Ok(ProviderEvent::Usage { .. }) => {}
                Ok(ProviderEvent::Done { .. }) => break,
                Ok(ProviderEvent::Error(message)) => {
                    err = Some(message);
                    break;
                }
                Err(e) => {
                    err = Some(e.to_string());
                    break;
                }
            }
        }
        (text, tool_calls, err)
    }
}

enum TurnVerdict {
    Continue,
    Completed(Message),
    Stuck(String),
}

/// Threshold at which repeated identical failures trigger a Stuck transition.
const THREE_STRIKE_LIMIT: u8 = 3;

enum GateOutcome {
    Proceed,
    Retry,
    Stuck(String),
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::focus::{FocusContract, FocusUpdate};
    use crate::task::evidence_gate::ErrorKind;

    use super::{IssueSignature, Phase, RunState, SteeringOutcome, normalize_error_msg};

    #[test]
    fn steering_outcome_variants_are_distinct() {
        assert_ne!(SteeringOutcome::Continue, SteeringOutcome::Cancelled);
    }

    #[test]
    fn focus_update_apply_to_contract() {
        let mut focus = FocusContract::empty();
        let update = FocusUpdate::new().with_add(vec!["security".to_string()]);
        update.apply(&mut focus).unwrap();
        assert!(focus.contains("security"));
    }

    // ── G6.9 Phase transition tests ──────────────────────────────────

    #[test]
    fn run_state_starts_at_phase_zero() {
        let state = RunState::new();
        assert_eq!(state.current_phase, Phase::Zero);
    }

    #[test]
    fn phase_zero_to_one_on_transition() {
        let mut state = RunState::new();
        state.transition_to(Phase::One);
        assert_eq!(state.current_phase, Phase::One);
        assert_eq!(state.turns_in_phase, 0);
    }

    #[test]
    fn phase_one_to_two_on_transition() {
        let mut state = RunState::new();
        state.transition_to(Phase::One);
        state.transition_to(Phase::Two);
        assert_eq!(state.current_phase, Phase::Two);
    }

    #[test]
    fn phase_two_to_three_on_transition() {
        let mut state = RunState::new();
        state.transition_to(Phase::Two);
        state.transition_to(Phase::Three);
        assert_eq!(state.current_phase, Phase::Three);
    }

    #[test]
    fn phase_three_to_two_on_transition() {
        let mut state = RunState::new();
        state.transition_to(Phase::Two);
        state.transition_to(Phase::Three);
        state.transition_to(Phase::Two);
        assert_eq!(state.current_phase, Phase::Two);
    }

    #[test]
    fn phase_transition_resets_turn_counter() {
        let mut state = RunState::new();
        state.tick_phase_turn();
        state.tick_phase_turn();
        assert_eq!(state.turns_in_phase, 2);
        state.transition_to(Phase::One);
        assert_eq!(state.turns_in_phase, 0);
    }

    #[test]
    fn phase_one_auto_advances_after_one_turn() {
        let mut state = RunState::new();
        state.transition_to(Phase::One);
        assert_eq!(state.current_phase, Phase::One);
        assert_eq!(state.turns_in_phase, 0);

        state.tick_phase_turn();
        assert!(state.turns_in_phase >= 1);
        state.transition_to(Phase::Two);
        assert_eq!(state.current_phase, Phase::Two);
    }

    // ── normalize_error_msg tests ────────────────────────────────────

    #[test]
    fn normalize_strips_line_numbers() {
        let h1 = normalize_error_msg("error at src/lib.rs:42:13");
        let h2 = normalize_error_msg("error at src/lib.rs:67:5");
        assert_eq!(h1, h2, "should be equal after stripping line numbers");
    }

    #[test]
    fn normalize_is_deterministic() {
        let h1 = normalize_error_msg("same error message");
        let h2 = normalize_error_msg("same error message");
        assert_eq!(h1, h2);
    }

    #[test]
    fn normalize_differs_for_different_messages() {
        let h1 = normalize_error_msg("type mismatch");
        let h2 = normalize_error_msg("borrow error");
        assert_ne!(h1, h2);
    }

    #[test]
    fn normalize_strips_absolute_paths() {
        let h1 = normalize_error_msg("/Users/foo/proj/src/lib.rs:10");
        let h2 = normalize_error_msg("/home/foo/proj/src/lib.rs:42");
        assert_eq!(
            h1, h2,
            "same relative path with different OS prefixes and line numbers should match"
        );
    }

    // ── IssueSignature tests ─────────────────────────────────────────

    #[test]
    fn issue_signature_equality() {
        let a = IssueSignature {
            file: PathBuf::from("src/lib.rs"),
            kind: ErrorKind::CompileError,
            msg_hash: 42,
        };
        let b = IssueSignature {
            file: PathBuf::from("src/lib.rs"),
            kind: ErrorKind::CompileError,
            msg_hash: 42,
        };
        assert_eq!(a, b);
    }

    #[test]
    fn issue_signature_inequality_on_kind() {
        let a = IssueSignature {
            file: PathBuf::from("src/lib.rs"),
            kind: ErrorKind::CompileError,
            msg_hash: 42,
        };
        let b = IssueSignature {
            file: PathBuf::from("src/lib.rs"),
            kind: ErrorKind::TestFailure,
            msg_hash: 42,
        };
        assert_ne!(a, b);
    }

    #[test]
    fn issue_signature_used_in_hashmap() {
        use std::collections::HashMap;
        let mut counters: HashMap<IssueSignature, u8> = HashMap::new();
        let sig = IssueSignature {
            file: PathBuf::from("src/lib.rs"),
            kind: ErrorKind::CompileError,
            msg_hash: 99,
        };
        counters.insert(sig.clone(), 1);
        assert_eq!(counters.get(&sig), Some(&1));
    }

    // ── G8.6 3-strike rule tests ─────────────────────────────────────

    fn make_error_entry(
        file: &str,
        kind: ErrorKind,
        msg: &str,
    ) -> crate::task::evidence_gate::ErrorEntry {
        crate::task::evidence_gate::ErrorEntry {
            file: Some(PathBuf::from(file)),
            line: None,
            kind,
            message: msg.to_string(),
        }
    }

    #[test]
    fn three_strikes_same_issue_returns_stuck_reason() {
        let mut state = RunState::new();
        let entry = make_error_entry("src/lib.rs", ErrorKind::CompileError, "type mismatch");

        assert!(
            state
                .record_evidence_failure(std::slice::from_ref(&entry))
                .is_none()
        );
        assert!(
            state
                .record_evidence_failure(std::slice::from_ref(&entry))
                .is_none()
        );
        let result = state.record_evidence_failure(std::slice::from_ref(&entry));
        assert!(result.is_some(), "third strike should return Stuck reason");
        let reason = result.unwrap();
        assert!(reason.contains("3 times"));
        assert!(reason.contains("type mismatch"));
    }

    #[test]
    fn two_strikes_do_not_trigger_stuck() {
        let mut state = RunState::new();
        let entry = make_error_entry("src/lib.rs", ErrorKind::CompileError, "borow error");

        assert!(
            state
                .record_evidence_failure(std::slice::from_ref(&entry))
                .is_none()
        );
        assert!(
            state
                .record_evidence_failure(std::slice::from_ref(&entry))
                .is_none()
        );
    }

    #[test]
    fn different_signatures_use_different_counters() {
        let mut state = RunState::new();
        let entry_a = make_error_entry("src/a.rs", ErrorKind::CompileError, "error A");
        let entry_b = make_error_entry("src/b.rs", ErrorKind::CompileError, "error B");

        assert!(
            state
                .record_evidence_failure(std::slice::from_ref(&entry_a))
                .is_none()
        );
        assert!(
            state
                .record_evidence_failure(std::slice::from_ref(&entry_b))
                .is_none()
        );
        assert!(
            state
                .record_evidence_failure(std::slice::from_ref(&entry_a))
                .is_none()
        );
        assert!(
            state
                .record_evidence_failure(std::slice::from_ref(&entry_b))
                .is_none()
        );
        assert_eq!(state.three_strike_counters.len(), 2);
    }

    #[test]
    fn line_number_shifts_do_not_create_new_counter() {
        let mut state = RunState::new();
        let e1 = make_error_entry(
            "src/lib.rs",
            ErrorKind::CompileError,
            "error at src/lib.rs:10",
        );
        let e2 = make_error_entry(
            "src/lib.rs",
            ErrorKind::CompileError,
            "error at src/lib.rs:42",
        );

        assert!(
            state
                .record_evidence_failure(std::slice::from_ref(&e1))
                .is_none()
        );
        assert!(
            state
                .record_evidence_failure(std::slice::from_ref(&e2))
                .is_none()
        );
        let result = state.record_evidence_failure(std::slice::from_ref(&e2));
        assert!(
            result.is_some(),
            "line-number shift should count as same issue"
        );
        assert_eq!(state.three_strike_counters.len(), 1);
    }

    #[test]
    fn clearing_counters_resets_strike_clock() {
        let mut state = RunState::new();
        let entry = make_error_entry("src/lib.rs", ErrorKind::CompileError, "oops");

        let _ = state.record_evidence_failure(std::slice::from_ref(&entry));
        let _ = state.record_evidence_failure(std::slice::from_ref(&entry));
        state.three_strike_counters.clear();
        assert!(
            state
                .record_evidence_failure(std::slice::from_ref(&entry))
                .is_none()
        );
        assert!(
            state
                .record_evidence_failure(std::slice::from_ref(&entry))
                .is_none()
        );
    }

    #[test]
    fn empty_errors_returns_none() {
        let mut state = RunState::new();
        assert!(state.record_evidence_failure(&[]).is_none());
        assert!(state.three_strike_counters.is_empty());
    }

    #[test]
    fn multiple_errors_in_one_failure_increment_independently() {
        let mut state = RunState::new();
        let entries = vec![
            make_error_entry("src/a.rs", ErrorKind::CompileError, "error A"),
            make_error_entry("src/b.rs", ErrorKind::TestFailure, "test B"),
        ];

        assert!(state.record_evidence_failure(&entries).is_none());
        assert_eq!(state.three_strike_counters.len(), 2);

        let result = state.record_evidence_failure(&entries.clone());
        assert!(result.is_none());

        let result = state.record_evidence_failure(&entries);
        assert!(result.is_some());
    }
}
