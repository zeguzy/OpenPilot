use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskStatus {
    Sleeping,
    Waking,
    Pondering,
    OnIt,
    Waiting,
    Reviewing,
    Delivered,
    Stuck,
    Axed,
    Archived,
}

impl TaskStatus {
    #[must_use]
    pub const fn emoji(self) -> &'static str {
        match self {
            Self::Sleeping => "\u{1F4A4}",
            Self::Waking => "\u{1F305}",
            Self::Pondering => "\u{1F914}",
            Self::OnIt => "\u{1F528}",
            Self::Waiting => "\u{1FAE5}",
            Self::Reviewing => "\u{1F50D}",
            Self::Delivered => "\u{2705}",
            Self::Stuck => "\u{1F635}",
            Self::Axed => "\u{2702}\u{FE0F}",
            Self::Archived => "\u{1F4E6}",
        }
    }

    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Archived)
    }
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::Sleeping => "sleeping",
            Self::Waking => "waking",
            Self::Pondering => "pondering",
            Self::OnIt => "on-it",
            Self::Waiting => "waiting",
            Self::Reviewing => "reviewing",
            Self::Delivered => "delivered",
            Self::Stuck => "stuck",
            Self::Axed => "axed",
            Self::Archived => "archived",
        };
        write!(f, "{name}")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TransitionError {
    #[error("invalid transition: {from} \u{2192} {to}")]
    InvalidTransition { from: TaskStatus, to: TaskStatus },
}

#[must_use]
pub const fn is_valid_transition(from: TaskStatus, to: TaskStatus) -> bool {
    use TaskStatus::{
        Archived, Axed, Delivered, OnIt, Pondering, Reviewing, Sleeping, Stuck, Waiting, Waking,
    };
    matches!(
        (from, to),
        (Sleeping, Waking | Axed)
            | (Waking | OnIt, Pondering)
            | (
                Waking | Pondering | OnIt | Waiting | Delivered | Reviewing | Stuck,
                Axed
            )
            | (Pondering | Waiting | Reviewing | Stuck, OnIt)
            | (OnIt, Waiting | Delivered | Stuck)
            | (Delivered, Reviewing | Archived)
            | (Reviewing | Axed, Archived)
    )
}

pub const fn transition(from: TaskStatus, to: TaskStatus) -> Result<TaskStatus, TransitionError> {
    if is_valid_transition(from, to) {
        Ok(to)
    } else {
        Err(TransitionError::InvalidTransition { from, to })
    }
}

pub const ALL_STATUSES: [TaskStatus; 10] = [
    TaskStatus::Sleeping,
    TaskStatus::Waking,
    TaskStatus::Pondering,
    TaskStatus::OnIt,
    TaskStatus::Waiting,
    TaskStatus::Reviewing,
    TaskStatus::Delivered,
    TaskStatus::Stuck,
    TaskStatus::Axed,
    TaskStatus::Archived,
];
