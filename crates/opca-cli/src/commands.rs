use crate::Reply;
use opca_core::orchestrator::route;

#[derive(Debug, Clone, PartialEq)]
pub enum SlashCommand {
    Accept {
        task_id: String,
    },
    Reject {
        task_id: String,
        feedback: Option<String>,
    },
    /// `/answer <task-id> <choice>` — answers a clarification request
    /// from a `Waiting` Task.
    Answer {
        task_id: String,
        choice: String,
    },
    Tasks,
    Status {
        task_id: Option<String>,
    },
    Help,
    Quit,
    /// `/continue <prompt>` starts a continuation chain.
    ///
    /// `/continue status [chain-id]` shows the status of one or all chains.
    Continue {
        action: ContinueAction,
    },
    /// `/stop-continuation <chain-id>` or `/stop-continuation all`.
    StopContinuation {
        target: StopTarget,
    },
    /// `/subtasks [parent-task-id]` — lists sub-tasks of a parent task.
    /// Only available when the `sub-agents` feature is enabled.
    #[cfg(feature = "sub-agents")]
    Subtasks {
        parent_task_id: Option<String>,
    },
}

/// What a `/continue` invocation should do.
#[derive(Debug, Clone, PartialEq)]
pub enum ContinueAction {
    /// Start a new chain with the given prompt and optional budget overrides.
    Start {
        prompt: String,
        max_iterations: Option<u32>,
        budget: Option<f64>,
    },
    /// Report status of one chain (`Some`) or every active chain (`None`).
    Status { chain_id: Option<String> },
}

/// Target of a `/stop-continuation` invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopTarget {
    /// Stop every active chain.
    All,
    /// Stop a single chain by its ID.
    One(String),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SlashError {
    #[error("unknown command: /{0}")]
    Unknown(String),
    #[error("missing task id for /{0}")]
    MissingTaskId(&'static str),
    #[error("malformed command: {0}")]
    Malformed(String),
    #[error("missing prompt for /{0}")]
    MissingPrompt(&'static str),
    #[error("missing chain id for /{0}")]
    MissingChainId(&'static str),
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
            "answer" => Self::Answer {
                task_id: require_task_id("answer", rest)?,
                choice: parse_rest_after_task_id(rest)?,
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
            "continue" | "continuation" => Self::Continue {
                action: parse_continue(rest)?,
            },
            "stop-continuation" | "stop-continue" => Self::StopContinuation {
                target: parse_stop_continuation(rest)?,
            },
            #[cfg(feature = "sub-agents")]
            "subtasks" | "sub-tasks" => Self::Subtasks {
                parent_task_id: if rest.is_empty() {
                    None
                } else {
                    Some(rest.to_string())
                },
            },
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

fn parse_rest_after_task_id(rest: &str) -> Result<String, SlashError> {
    let mut tokens = rest.split_whitespace();
    tokens.next().ok_or(SlashError::MissingTaskId("answer"))?;
    let remainder = tokens.collect::<Vec<_>>().join(" ");
    let cleaned = remainder
        .trim_matches(|c| c == '"' || c == '\'')
        .to_string();
    if cleaned.is_empty() {
        Err(SlashError::MissingPrompt("answer"))
    } else {
        Ok(cleaned)
    }
}

fn parse_continue(rest: &str) -> Result<ContinueAction, SlashError> {
    if rest.is_empty() {
        return Err(SlashError::MissingPrompt("continue"));
    }
    if let Some(chain_id) = rest.strip_prefix("status") {
        let trimmed = chain_id.trim();
        return Ok(ContinueAction::Status {
            chain_id: if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            },
        });
    }

    let mut max_iterations: Option<u32> = None;
    let mut budget: Option<f64> = None;
    let mut prompt_parts: Vec<&str> = Vec::new();

    let mut tokens = rest.split_whitespace();
    while let Some(tok) = tokens.next() {
        match tok {
            "--max-iterations" | "-i" => {
                let raw = tokens.next().ok_or_else(|| {
                    SlashError::Malformed("--max-iterations needs a value".into())
                })?;
                max_iterations = Some(raw.parse::<u32>().map_err(|_| {
                    SlashError::Malformed(format!("invalid --max-iterations value '{raw}'"))
                })?);
            }
            "--budget" | "-b" => {
                let raw = tokens
                    .next()
                    .ok_or_else(|| SlashError::Malformed("--budget needs a value".into()))?;
                budget = Some(raw.parse::<f64>().map_err(|_| {
                    SlashError::Malformed(format!("invalid --budget value '{raw}'"))
                })?);
            }
            other => prompt_parts.push(other),
        }
    }

    if prompt_parts.is_empty() {
        return Err(SlashError::MissingPrompt("continue"));
    }
    let prompt = prompt_parts.join(" ");
    Ok(ContinueAction::Start {
        prompt,
        max_iterations,
        budget,
    })
}

fn parse_stop_continuation(rest: &str) -> Result<StopTarget, SlashError> {
    let target = rest
        .split_whitespace()
        .next()
        .ok_or(SlashError::MissingChainId("stop-continuation"))?;
    Ok(if target.eq_ignore_ascii_case("all") {
        StopTarget::All
    } else {
        StopTarget::One(target.to_string())
    })
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
  /answer <task-id> <choice>   Answer a clarification request from a Waiting task
  /tasks                       List all active tasks
  /status [task-id]            Show task status (omit id for overview)
  /continue <prompt>           Start a continuation chain (auto-iterate until Audit confirms)
  /continue status [chain-id]  Show continuation chain status
  /continue [-i N] [-b USD] <prompt>  Start with budget overrides
  /stop-continuation <chain-id>  Terminate one continuation chain
  /stop-continuation all         Terminate every active continuation chain
  /subtasks [parent-id]        List sub-tasks of a parent task (feature: sub-agents)
  /help, /?                    Show this help
  /quit, /exit                 Exit the REPL

Tips:
  - Background tasks run silently. You will see a 🔔 notification on completion.
  - Ask naturally: \"how is task-0 going?\" or \"what's running?\"
  - Pending reviews appear in the prompt area without interrupting you.";
