use std::collections::VecDeque;

use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

use crate::focus::{FocusUpdate, Highlight};
use crate::lifecycle::Heartbeat;
use crate::lifecycle::TaskStatus;
use crate::provider::Message;

#[derive(Debug, Clone)]
pub enum TaskOutput {
    TextDelta(String),
    ThinkingDelta(String),
    ToolCall {
        name: String,
        args: String,
    },
    ToolResult {
        name: String,
        success: bool,
        summary: String,
    },
    Highlight(Highlight),
    StatusChanged {
        status: TaskStatus,
        progress: f64,
        summary: String,
    },
    Done,
}

pub type OutputTx = UnboundedSender<TaskOutput>;
pub type OutputRx = UnboundedReceiver<TaskOutput>;

#[derive(Debug, Clone, PartialEq)]
pub enum SteeringMessage {
    Inject(Message),
    UpdateFocus(FocusUpdate),
    Cancel,
}

pub type SteeringTx = UnboundedSender<SteeringMessage>;
pub type SteeringRx = UnboundedReceiver<SteeringMessage>;
pub type HeartbeatTx = UnboundedSender<Heartbeat>;
pub type HeartbeatRx = UnboundedReceiver<Heartbeat>;
pub type HighlightTx = UnboundedSender<Highlight>;
pub type HighlightRx = UnboundedReceiver<Highlight>;

#[derive(Debug, Clone, PartialEq)]
pub enum FollowupMessage {
    User(Message),
}

#[derive(Debug, Default)]
pub struct FollowupQueue {
    inner: VecDeque<FollowupMessage>,
}

impl FollowupQueue {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, msg: impl Into<FollowupMessage>) {
        self.inner.push_back(msg.into());
    }

    pub fn drain(&mut self) -> Vec<FollowupMessage> {
        self.inner.drain(..).collect()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.len()
    }
}

impl From<Message> for FollowupMessage {
    fn from(msg: Message) -> Self {
        Self::User(msg)
    }
}

#[derive(Debug)]
pub struct TaskHandle {
    pub steering_tx: SteeringTx,
    pub heartbeat_rx: HeartbeatRx,
    pub highlight_rx: HighlightRx,
    pub output_rx: OutputRx,
}

pub struct ChannelSet {
    pub steering_rx: SteeringRx,
    pub heartbeat_tx: HeartbeatTx,
    pub highlight_tx: HighlightTx,
    pub output_tx: OutputTx,
}

#[must_use]
pub fn create_channels() -> (TaskHandle, ChannelSet) {
    let (steering_tx, steering_rx) = mpsc::unbounded_channel();
    let (heartbeat_tx, heartbeat_rx) = mpsc::unbounded_channel();
    let (highlight_tx, highlight_rx) = mpsc::unbounded_channel();
    let (output_tx, output_rx) = mpsc::unbounded_channel();
    let handle = TaskHandle {
        steering_tx,
        heartbeat_rx,
        highlight_rx,
        output_rx,
    };
    let set = ChannelSet {
        steering_rx,
        heartbeat_tx,
        highlight_tx,
        output_tx,
    };
    (handle, set)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::Message;

    #[test]
    fn followup_queue_push_drain_roundtrip() {
        let mut q = FollowupQueue::new();
        assert!(q.is_empty());
        q.push(FollowupMessage::User(Message::user("a")));
        q.push(Message::user("b"));
        assert_eq!(q.len(), 2);
        let drained = q.drain();
        assert_eq!(drained.len(), 2);
        assert!(q.is_empty());
    }

    #[test]
    fn followup_drain_clears_queue() {
        let mut q = FollowupQueue::new();
        q.push(Message::user("x"));
        q.push(Message::user("y"));
        let first = q.drain();
        assert_eq!(first.len(), 2);
        let second = q.drain();
        assert!(second.is_empty());
    }

    #[test]
    fn steering_message_inject_carries_message() {
        let m = SteeringMessage::Inject(Message::user("hi"));
        assert!(matches!(m, SteeringMessage::Inject(_)));
    }

    #[test]
    fn steering_message_cancel_equality() {
        assert_eq!(SteeringMessage::Cancel, SteeringMessage::Cancel);
    }

    #[tokio::test]
    async fn create_channels_returns_paired_endpoints() {
        let (handle, mut set) = create_channels();
        handle.steering_tx.send(SteeringMessage::Cancel).unwrap();
        let received = set.steering_rx.recv().await.unwrap();
        assert_eq!(received, SteeringMessage::Cancel);
    }
}
