use std::future::Future;
use std::pin::Pin;

use super::message::Message;
use super::tool::ToolDef;

pub trait Provider: Send + Sync {
    fn stream(
        &self,
        messages: &[Message],
        tools: &[ToolDef],
        system_prompt: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<ProviderStream>> + Send>>;
}

pub type ProviderStream =
    Pin<Box<dyn tokio_stream::Stream<Item = anyhow::Result<ProviderEvent>> + Send>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderEvent {
    TextDelta(String),
    ToolCallStart {
        id: String,
        name: String,
    },
    ToolCallArgs {
        id: String,
        args: String,
    },
    ToolCallEnd {
        id: String,
    },
    Usage {
        prompt_tokens: u64,
        completion_tokens: u64,
    },
    Done {
        stop_reason: StopReason,
    },
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    StopSequence,
}

impl ProviderEvent {
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::Done { .. } | Self::Error(..))
    }
}
