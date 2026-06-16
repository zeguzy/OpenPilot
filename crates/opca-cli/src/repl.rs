use std::borrow::Cow;
use std::sync::Arc;

#[cfg(feature = "sub-agents")]
use std::fmt::Write;

use opca_core::lifecycle::TaskStatus;
use reedline::{DefaultPrompt, Prompt, PromptEditMode, PromptHistorySearch, Reedline, Signal};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;

use crate::commands::{ContinueAction, HELP_TEXT, SlashCommand, StopTarget};
use crate::mock::format_task_line;
use crate::{Notification, OrchestratorApi, Reply, TaskInfo};

pub trait Output: Send + Sync {
    fn print_line(&self, msg: &str);
    fn print(&self, msg: &str);
}

pub struct StdOutput;

impl Output for StdOutput {
    fn print_line(&self, msg: &str) {
        println!("{msg}");
    }

    fn print(&self, msg: &str) {
        print!("{msg}");
    }
}

pub struct BufferOutput(std::sync::Mutex<Vec<String>>);

impl BufferOutput {
    #[must_use]
    pub const fn new() -> Self {
        Self(std::sync::Mutex::new(Vec::new()))
    }

    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        self.0.lock().expect("poisoned").clone()
    }

    pub fn clear(&self) {
        self.0.lock().expect("poisoned").clear();
    }

    #[must_use]
    pub fn joined(&self) -> String {
        self.0.lock().expect("poisoned").join("\n")
    }
}

impl Default for BufferOutput {
    fn default() -> Self {
        Self::new()
    }
}

impl Output for BufferOutput {
    fn print_line(&self, msg: &str) {
        self.0.lock().expect("poisoned").push(msg.to_string());
    }

    fn print(&self, msg: &str) {
        let mut buf = self.0.lock().expect("poisoned");
        if let Some(last) = buf.last_mut() {
            last.push_str(msg);
        } else {
            buf.push(msg.to_string());
        }
    }
}

pub struct Repl {
    orchestrator: Arc<dyn OrchestratorApi>,
    output: Arc<dyn Output>,
    pending_review_indicator: bool,
}

impl Repl {
    #[must_use]
    pub fn new(orchestrator: Arc<dyn OrchestratorApi>, output: Arc<dyn Output>) -> Self {
        Self {
            orchestrator,
            output,
            pending_review_indicator: true,
        }
    }

    #[must_use]
    pub const fn with_review_indicator(mut self, enabled: bool) -> Self {
        self.pending_review_indicator = enabled;
        self
    }

    pub fn handle_line(&self, line: &str) -> HandleOutcome {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return HandleOutcome::Continue;
        }
        match SlashCommand::parse(trimmed) {
            Ok(Some(cmd)) => self.run_slash(cmd),
            Ok(None) => self.run_message(trimmed),
            Err(e) => {
                self.output.print_line(&format!("error: {e}"));
                HandleOutcome::Continue
            }
        }
    }

    fn run_slash(&self, cmd: SlashCommand) -> HandleOutcome {
        match cmd {
            SlashCommand::Accept { task_id } => match self.orchestrator.accept(&task_id) {
                Ok(()) => {
                    self.output.print_line(&format!("✓ merged task {task_id}"));
                }
                Err(e) => self.output.print_line(&format!("cannot accept: {e}")),
            },
            SlashCommand::Reject { task_id, feedback } => {
                match self.orchestrator.reject(&task_id, feedback.as_deref()) {
                    Ok(()) => {
                        if feedback.is_some() {
                            self.output.print_line(&format!(
                                "↻ returned task {task_id} to OnIt with feedback"
                            ));
                        } else {
                            self.output
                                .print_line(&format!("✗ discarded task {task_id}"));
                        }
                    }
                    Err(e) => self.output.print_line(&format!("cannot reject: {e}")),
                }
            }
            SlashCommand::Answer { task_id, choice } => {
                match self.orchestrator.answer_task(&task_id, &choice) {
                    Ok(()) => {
                        self.output
                            .print_line(&format!("↻ answered task {task_id}: {choice}"));
                    }
                    Err(e) => self.output.print_line(&format!("cannot answer: {e}")),
                }
            }
            SlashCommand::Tasks => {
                let tasks = self.orchestrator.list_tasks();
                self.output.print_line(&render_task_list(&tasks));
            }
            SlashCommand::Status { task_id } => {
                if let Some(id) = task_id {
                    match self.orchestrator.task_status(&id) {
                        Some(info) => self.output.print_line(&render_single_status(&info)),
                        None => self.output.print_line(&format!("no task named '{id}'")),
                    }
                } else {
                    let pending = self.orchestrator.pending_review_count();
                    if pending > 0 {
                        self.output.print_line(&format!(
                            "{pending} task{} pending review",
                            if pending == 1 { "" } else { "s" }
                        ));
                    } else {
                        self.output.print_line("no tasks pending review");
                    }
                }
            }
            SlashCommand::Help => {
                self.output.print_line(HELP_TEXT);
            }
            SlashCommand::Continue { action } => self.run_continue(action),
            SlashCommand::StopContinuation { target } => self.run_stop_continuation(target),
            SlashCommand::Quit => return HandleOutcome::Quit,
            #[cfg(feature = "sub-agents")]
            SlashCommand::Subtasks { parent_task_id } => {
                self.run_subtasks(parent_task_id);
            }
        }
        HandleOutcome::Continue
    }

    fn run_continue(&self, action: ContinueAction) {
        match action {
            ContinueAction::Start {
                prompt,
                max_iterations,
                budget,
            } => {
                let chain_id =
                    self.orchestrator
                        .start_continuation(&prompt, max_iterations, budget);
                self.output.print_line(&format!(
                    "🔗 continuation chain {chain_id} started — {prompt}"
                ));
            }
            ContinueAction::Status { chain_id } => {
                let report = self.orchestrator.continuation_status(chain_id.as_deref());
                self.output.print_line(&report);
            }
        }
    }

    fn run_stop_continuation(&self, target: StopTarget) {
        match target {
            StopTarget::All => match self.orchestrator.stop_continuation("all") {
                Ok(0) => self.output.print_line("no active continuation chains"),
                Ok(n) => self
                    .output
                    .print_line(&format!("✗ stopped {n} continuation chain(s)")),
                Err(e) => self.output.print_line(&format!("cannot stop: {e}")),
            },
            StopTarget::One(id) => match self.orchestrator.stop_continuation(&id) {
                Ok(0) => self.output.print_line(&format!("chain {id} is not active")),
                Ok(_) => self
                    .output
                    .print_line(&format!("✗ stopped continuation chain {id}")),
                Err(e) => self.output.print_line(&format!("cannot stop: {e}")),
            },
        }
    }

    #[cfg(feature = "sub-agents")]
    fn run_subtasks(&self, parent_task_id: Option<String>) {
        let subs = self.orchestrator.list_subtasks(parent_task_id.as_deref());
        if subs.is_empty() {
            self.output.print_line("No sub-tasks.");
        } else {
            self.output.print_line(&render_subtask_list(&subs));
        }
    }

    fn run_message(&self, message: &str) -> HandleOutcome {
        let reply = self.orchestrator.handle_message(message);
        match reply {
            Reply::Dispatched {
                task_id,
                description,
            } => {
                self.output
                    .print_line(&format!("🚀 dispatched {task_id} — {description}",));
            }
            Reply::Foreground(text) => {
                self.output.print_line(&text);
            }
            Reply::Acknowledged(text) => {
                self.output.print_line(&text);
            }
            Reply::Error(text) => {
                self.output.print_line(&format!("error: {text}"));
            }
            Reply::Nothing => {}
        }
        HandleOutcome::Continue
    }

    pub fn render_notification(&self, notif: &Notification) {
        match notif {
            Notification::Completed {
                task_id,
                description,
                files_modified,
                summary,
            } => {
                let files_note = if *files_modified > 0 {
                    format!(
                        " — {files_modified} file{} modified",
                        if *files_modified == 1 { "" } else { "s" }
                    )
                } else {
                    String::new()
                };
                let short = shorten(description, 40);
                self.output.print_line(&format!(
                    "\u{1F514} Task {task_id} \"{short}\" done{files_note}"
                ));
                if !summary.is_empty() {
                    let preview = shorten(summary, 200);
                    self.output.print_line(&format!("  → {preview}"));
                }
            }
            Notification::StatusChanged {
                task_id,
                status,
                summary,
            } => {
                let short = shorten(summary, 60);
                self.output
                    .print_line(&format!("{} {task_id} → {status}: {short}", status.emoji()));
            }
            Notification::Clarification {
                task_id,
                question,
                options,
                timeout_secs,
            } => {
                let mins = timeout_secs / 60;
                let opts = if options.is_empty() {
                    String::new()
                } else {
                    let items: Vec<String> = options
                        .iter()
                        .enumerate()
                        .map(|(i, opt)| format!("  {}. {opt}", i + 1))
                        .collect();
                    format!("\n{}", items.join("\n"))
                };
                self.output.print_line(&format!(
                    "\u{1fae5} Task {task_id} is waiting — {question}{opts}\n  Reply with: /answer {task_id} <your choice> (auto-proceeds in {mins}m)"
                ));
            }
        }
    }

    #[must_use]
    pub fn prompt_indicator(&self) -> String {
        let pending = self.orchestrator.pending_review_count();
        if self.pending_review_indicator && pending > 0 {
            format!("[{pending} pending review — /tasks to see]\n> ",)
        } else {
            "> ".to_string()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandleOutcome {
    Continue,
    Quit,
}

pub fn render_task_list(tasks: &[TaskInfo]) -> String {
    if tasks.is_empty() {
        return "No tasks.".to_string();
    }
    let mut out = String::from("Active Tasks:");
    for info in tasks {
        out.push_str("\n  ");
        out.push_str(&format_task_line(info));
    }
    out
}

pub fn render_single_status(info: &TaskInfo) -> String {
    let pct = (info.progress * 100.0).round() as u32;
    format!(
        "{} {} [{} {}%] — {}\n  description: {}\n  files modified: {}",
        info.status.emoji(),
        info.id,
        info.status,
        pct,
        info.summary,
        info.description,
        info.files_modified,
    )
}

#[cfg(feature = "sub-agents")]
pub fn render_subtask_list(subs: &[crate::SubTaskInfo]) -> String {
    if subs.is_empty() {
        return "No sub-tasks.".to_string();
    }
    let mut out = format!("Sub-tasks ({}):\n", subs.len());
    for s in subs {
        let pct = (s.progress * 100.0).round() as u32;
        let _ = write!(
            out,
            "  {} {} [{} {}%] — {}\n    description: {}\n",
            s.status.emoji(),
            s.id,
            s.status,
            pct,
            s.summary,
            s.description,
        );
    }
    out.trim_end().to_string()
}

fn shorten(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('\u{2026}');
        out
    }
}

pub fn spawn_input_loop(tx: UnboundedSender<String>) -> JoinHandle<()> {
    tokio::task::spawn_blocking(move || run_reedline_input(tx))
}

fn run_reedline_input(tx: UnboundedSender<String>) {
    let mut reedline = Reedline::create();
    let prompt = OpcaPrompt::default();
    loop {
        let signal = reedline.read_line(&prompt);
        match signal {
            Ok(Signal::Success(line)) => {
                let trimmed = line.trim().to_string();
                if trimmed.is_empty() {
                    continue;
                }
                if tx.send(trimmed).is_err() {
                    break;
                }
            }
            Ok(Signal::CtrlC) => {
                break;
            }
            Ok(Signal::CtrlD) => break,
            Err(_) => break,
        }
    }
}

#[derive(Clone, Default)]
struct OpcaPrompt {
    inner: DefaultPrompt,
}

impl Prompt for OpcaPrompt {
    fn render_prompt_left(&self) -> Cow<'_, str> {
        self.inner.render_prompt_left()
    }

    fn render_prompt_right(&self) -> Cow<'_, str> {
        self.inner.render_prompt_right()
    }

    fn render_prompt_indicator(&self, mode: PromptEditMode) -> Cow<'_, str> {
        self.inner.render_prompt_indicator(mode)
    }

    fn render_prompt_multiline_indicator(&self) -> Cow<'_, str> {
        self.inner.render_prompt_multiline_indicator()
    }

    fn render_prompt_history_search_indicator(&self, search: PromptHistorySearch) -> Cow<'_, str> {
        self.inner.render_prompt_history_search_indicator(search)
    }
}

#[derive(Debug)]
pub struct ReplRuntime {
    pub input_tx: UnboundedSender<String>,
    pub input_handle: Option<JoinHandle<()>>,
    pub repl_handle: Option<JoinHandle<()>>,
    pub notif_handle: Option<JoinHandle<()>>,
}

impl ReplRuntime {
    pub fn run(
        orchestrator: Arc<dyn OrchestratorApi>,
        output: Arc<dyn Output>,
        enable_input_thread: bool,
    ) -> Self {
        let repl = Arc::new(Repl::new(orchestrator.clone(), output));
        let (input_tx, input_rx) = mpsc::unbounded_channel::<String>();
        let notif_rx = orchestrator.subscribe();

        let input_handle = if enable_input_thread {
            Some(spawn_input_loop(input_tx.clone()))
        } else {
            None
        };

        let repl_clone = repl;
        let repl_handle = tokio::spawn(repl_main_loop(repl_clone, input_rx, notif_rx));

        Self {
            input_tx,
            input_handle,
            repl_handle: Some(repl_handle),
            notif_handle: None,
        }
    }

    pub fn shutdown(self) {
        let _ = self.input_tx.send(String::new());
        if let Some(handle) = self.input_handle {
            handle.abort();
        }
    }
}

async fn repl_main_loop(
    repl: Arc<Repl>,
    mut input_rx: UnboundedReceiver<String>,
    mut notif_rx: UnboundedReceiver<Notification>,
) {
    loop {
        tokio::select! {
            Some(line) = input_rx.recv() => {
                if line.is_empty() {
                    continue;
                }
                let outcome = repl.handle_line(&line);
                if outcome == HandleOutcome::Quit {
                    break;
                }
            }
            Some(notif) = notif_rx.recv() => {
                repl.render_notification(&notif);
            }
            else => break,
        }
    }
}

pub async fn run_main_loop_for_test(
    repl: Arc<Repl>,
    input_rx: UnboundedReceiver<String>,
    notif_rx: UnboundedReceiver<Notification>,
) {
    repl_main_loop(repl, input_rx, notif_rx).await;
}

#[allow(dead_code)]
pub const fn dummy_status() -> TaskStatus {
    TaskStatus::Sleeping
}
