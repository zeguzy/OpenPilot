use crate::Reply;
use opca_core::orchestrator::route;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlashCommand {
    Accept {
        task_id: String,
    },
    Reject {
        task_id: String,
        feedback: Option<String>,
    },
    Tasks,
    Status {
        task_id: Option<String>,
    },
    Help,
    Quit,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SlashError {
    #[error("unknown command: /{0}")]
    Unknown(String),
    #[error("missing task id for /{0}")]
    MissingTaskId(&'static str),
    #[error("malformed command: {0}")]
    Malformed(String),
}

impl SlashCommand {
    pub fn parse(input: &str) -> Result<Option<Self>, SlashError> {
        let trimmed = input.trim();
        if !trimmed.starts_with('/') {
            return Ok(None);
        }
        let body = &trimmed[1..];
        let mut parts = body.splitn(2, char::is_whitespace);
        let name = parts.next().unwrap_or_default();
        let rest = parts.next().unwrap_or_default().trim();
        let command = match name {
            "accept" => Self::Accept {
                task_id: require_task_id("accept", rest)?,
            },
            "reject" => Self::Reject {
                task_id: require_task_id("reject", rest)?,
                feedback: parse_feedback(rest),
            },
            "tasks" | "running" | "jobs" => Self::Tasks,
            "status" => Self::Status {
                task_id: if rest.is_empty() {
                    None
                } else {
                    Some(rest.to_string())
                },
            },
            "help" | "?" => Self::Help,
            "quit" | "exit" | "q" => Self::Quit,
            other => return Err(SlashError::Unknown(other.to_string())),
        };
        Ok(Some(command))
    }
}

fn require_task_id(cmd: &'static str, rest: &str) -> Result<String, SlashError> {
    rest.split_whitespace()
        .next()
        .map(str::to_string)
        .ok_or(SlashError::MissingTaskId(cmd))
}

fn parse_feedback(rest: &str) -> Option<String> {
    let mut tokens = rest.split_whitespace();
    tokens.next()?;
    let remainder = tokens.collect::<Vec<_>>().join(" ");
    let cleaned = remainder
        .trim_matches(|c| c == '"' || c == '\'')
        .to_string();
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

#[must_use]
pub fn route_message(message: &str) -> Reply {
    let _ = route(message, "");
    Reply::Nothing
}

pub const HELP_TEXT: &str = "\
Slash commands:
  /accept <task-id>            Accept (merge) a delivered task
  /reject <task-id> [\"msg\"]    Reject a task; with feedback, returns it to OnIt
  /tasks                       List all active tasks
  /status [task-id]            Show task status (omit id for overview)
  /help, /?                    Show this help
  /quit, /exit                 Exit the REPL

Tips:
  - Background tasks run silently. You will see a 🔔 notification on completion.
  - Ask naturally: \"how is task-0 going?\" or \"what's running?\"
  - Pending reviews appear in the prompt area without interrupting you.";
