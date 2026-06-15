use std::sync::Arc;

use crate::{Notification, OrchestratorApi, Reply};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppMode {
    Orchestrator,
    Task { task_id: String },
}

#[derive(Debug, Clone)]
pub enum ChatItem {
    UserMessage(String),
    AssistantText(String),
    StreamingAssistant(String),
    TaskPanel {
        task_id: String,
        description: String,
        collapsed: bool,
        events: Vec<String>,
    },
    SystemMessage(String),
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEvent {
    Delta(String),
    Done,
    Dispatch(String),
    Error(String),
}

pub struct App {
    pub mode: AppMode,
    pub chat_items: Vec<ChatItem>,
    pub model_name: String,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub should_quit: bool,
    pub is_working: bool,
    pub pending_messages: std::collections::VecDeque<String>,
    pub working_start: Option<std::time::Instant>,
    pub spinner_frame: usize,
    pub orchestrator: Arc<dyn OrchestratorApi>,
    pub stream_rx: tokio::sync::mpsc::UnboundedReceiver<StreamEvent>,
    pub stream_tx: tokio::sync::mpsc::UnboundedSender<StreamEvent>,
    /// Chat scroll offset in rendered lines. 0 = stick to bottom (latest).
    /// Positive = scrolled up N lines from the bottom.
    pub scroll_offset: usize,
}

impl App {
    #[must_use]
    pub fn new(orchestrator: Arc<dyn OrchestratorApi>, model_name: String) -> Self {
        let (stream_tx, stream_rx) = tokio::sync::mpsc::unbounded_channel();
        Self {
            mode: AppMode::Orchestrator,
            chat_items: vec![ChatItem::SystemMessage(
                "opca — background-first code agent. Type /help for commands.".to_string(),
            )],
            model_name,
            prompt_tokens: 0,
            completion_tokens: 0,
            should_quit: false,
            is_working: false,
            pending_messages: std::collections::VecDeque::new(),
            working_start: None,
            spinner_frame: 0,
            orchestrator,
            stream_rx,
            stream_tx,
            scroll_offset: 0,
        }
    }

    pub fn start_working(&mut self) {
        self.is_working = true;
        self.working_start = Some(std::time::Instant::now());
    }

    pub const fn stop_working(&mut self) {
        self.is_working = false;
        self.working_start = None;
    }

    pub const fn scroll_up(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset.saturating_add(lines);
    }

    pub const fn scroll_down(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(lines);
    }

    pub const fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
    }

    pub const fn is_at_bottom(&self) -> bool {
        self.scroll_offset == 0
    }

    #[must_use]
    pub fn elapsed_secs(&self) -> u64 {
        self.working_start.map_or(0, |t| t.elapsed().as_secs())
    }

    #[must_use]
    pub const fn spinner_char(&self) -> &str {
        const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        FRAMES[self.spinner_frame % FRAMES.len()]
    }

    pub fn handle_message(&mut self, msg: &str) {
        if msg.starts_with('/') {
            self.handle_slash_command(msg);
            return;
        }
        match &self.mode {
            AppMode::Orchestrator => {
                self.chat_items.push(ChatItem::UserMessage(msg.to_string()));
                let reply = self.orchestrator.handle_message(msg);
                match reply {
                    Reply::Foreground(_) => {
                        self.start_working();
                        self.chat_items
                            .push(ChatItem::StreamingAssistant(String::new()));
                    }
                    Reply::Dispatched {
                        task_id,
                        description,
                    } => {
                        self.chat_items.push(ChatItem::TaskPanel {
                            task_id,
                            description,
                            collapsed: true,
                            events: Vec::new(),
                        });
                    }
                    Reply::Acknowledged(text) => {
                        self.chat_items.push(ChatItem::SystemMessage(text));
                    }
                    Reply::Error(text) => {
                        self.chat_items.push(ChatItem::Error(text));
                    }
                    Reply::Nothing => {}
                }
            }
            AppMode::Task { task_id } => {
                self.chat_items.push(ChatItem::UserMessage(format!(
                    "[steering -> {task_id}] {msg}"
                )));
            }
        }
    }

    pub fn poll_stream(&mut self) {
        while let Ok(ev) = self.stream_rx.try_recv() {
            match ev {
                StreamEvent::Delta(delta) => {
                    if let Some(ChatItem::StreamingAssistant(text)) = self.chat_items.last_mut() {
                        text.push_str(&delta);
                    }
                }
                StreamEvent::Done => {
                    if let Some(ChatItem::StreamingAssistant(text)) = self.chat_items.last_mut() {
                        let owned = std::mem::take(text);
                        *self.chat_items.last_mut().unwrap() = ChatItem::AssistantText(owned);
                    }
                    self.stop_working();
                }
                StreamEvent::Dispatch(description) => {
                    let task_id = self.orchestrator.dispatch(&description);
                    if task_id.starts_with("dispatch-error") {
                        if let Some(last) = self.chat_items.last_mut() {
                            *last = ChatItem::Error(task_id);
                        }
                    } else {
                        if let Some(last) = self.chat_items.last_mut() {
                            *last = ChatItem::SystemMessage("dispatched to background".to_string());
                        }
                        self.chat_items.push(ChatItem::TaskPanel {
                            task_id,
                            description,
                            collapsed: true,
                            events: Vec::new(),
                        });
                    }
                    self.stop_working();
                }
                StreamEvent::Error(err) => {
                    if let Some(last) = self.chat_items.last_mut() {
                        *last = ChatItem::Error(err);
                    }
                    self.stop_working();
                }
            }
        }
    }

    pub fn handle_notification(&mut self, notif: &Notification) {
        match notif {
            Notification::Completed {
                task_id,
                description,
                files_modified,
            } => {
                self.update_task_panel(task_id, &format!("done — {files_modified} files"));
                self.chat_items.push(ChatItem::SystemMessage(format!(
                    "🔔 Task {task_id} \"{description}\" done"
                )));
            }
            Notification::StatusChanged {
                task_id,
                status,
                summary,
            } => {
                self.update_task_panel(task_id, &format!("{status} — {summary}"));
            }
            Notification::Clarification {
                task_id, question, ..
            } => {
                self.update_task_panel(task_id, "waiting for clarification");
                self.chat_items.push(ChatItem::SystemMessage(format!(
                    "\u{1fae5} Task {task_id} asks: {question}"
                )));
            }
        }
    }

    fn update_task_panel(&mut self, task_id: &str, msg: &str) {
        for item in &mut self.chat_items {
            if let ChatItem::TaskPanel {
                task_id: tid,
                events,
                ..
            } = item
            {
                if tid == task_id {
                    events.push(msg.to_string());
                }
            }
        }
    }

    fn handle_slash_command(&mut self, cmd: &str) {
        let parts: Vec<&str> = cmd[1..].splitn(2, char::is_whitespace).collect();
        let name = parts[0];
        let arg = parts.get(1).copied().unwrap_or("");
        match name {
            "quit" | "exit" | "q" => self.should_quit = true,
            "help" | "?" => {
                self.chat_items.push(ChatItem::SystemMessage(
                    "Commands: /task <id> /back /expand <id> /collapse <id> /model <name> /clear /cost /tasks /quit"
                        .to_string(),
                ));
            }
            "tasks" => {
                let tasks = self.orchestrator.list_tasks();
                if tasks.is_empty() {
                    self.chat_items
                        .push(ChatItem::SystemMessage("No active tasks.".to_string()));
                } else {
                    let mut lines = vec!["Active Tasks:".to_string()];
                    for t in tasks {
                        lines.push(format!(
                            "  {} [{:.0}%] — {}",
                            t.id,
                            t.progress * 100.0,
                            t.summary
                        ));
                    }
                    self.chat_items
                        .push(ChatItem::SystemMessage(lines.join("\n")));
                }
            }
            "task" => {
                if arg.is_empty() {
                    self.chat_items
                        .push(ChatItem::Error("Usage: /task <id>".to_string()));
                } else {
                    self.mode = AppMode::Task {
                        task_id: arg.to_string(),
                    };
                    self.chat_items.push(ChatItem::SystemMessage(format!(
                        "Switched to Task {arg} mode. /back to return."
                    )));
                }
            }
            "back" => {
                self.mode = AppMode::Orchestrator;
                self.chat_items.push(ChatItem::SystemMessage(
                    "Back to Orchestrator mode.".to_string(),
                ));
            }
            "expand" => {
                if let Some(ChatItem::TaskPanel { collapsed, .. }) = self
                    .chat_items
                    .iter_mut()
                    .find(|i| matches!(i, ChatItem::TaskPanel { task_id, .. } if task_id == arg))
                {
                    *collapsed = false;
                }
            }
            "collapse" => {
                if let Some(ChatItem::TaskPanel { collapsed, .. }) = self
                    .chat_items
                    .iter_mut()
                    .find(|i| matches!(i, ChatItem::TaskPanel { task_id, .. } if task_id == arg))
                {
                    *collapsed = true;
                }
            }
            "clear" => {
                self.chat_items.clear();
                self.chat_items
                    .push(ChatItem::SystemMessage("Conversation cleared.".to_string()));
            }
            "cost" => {
                self.chat_items.push(ChatItem::SystemMessage(format!(
                    "Tokens: up:{} down:{} | Est. cost: ${:.4}",
                    self.prompt_tokens,
                    self.completion_tokens,
                    self.estimated_cost()
                )));
            }
            "model" => {
                self.model_name = arg.to_string();
                self.chat_items
                    .push(ChatItem::SystemMessage(format!("Model set to {arg}")));
            }
            _ => {
                self.chat_items
                    .push(ChatItem::Error(format!("Unknown command: /{name}")));
            }
        }
    }

    pub const fn add_tokens(&mut self, prompt: u64, completion: u64) {
        self.prompt_tokens += prompt;
        self.completion_tokens += completion;
    }

    #[must_use]
    pub fn estimated_cost(&self) -> f64 {
        (self.prompt_tokens as f64 / 1000.0)
            .mul_add(0.003, (self.completion_tokens as f64 / 1000.0) * 0.015)
    }
}
