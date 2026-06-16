use super::tool::{ToolCall, ToolResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MessageRole {
    User,
    Assistant,
    System,
    Tool,
}

/// A single piece of content within a [`Message`].
///
/// Messages are multi-part: text, thinking traces, tool calls, and tool
/// results are all represented as distinct parts. This mirrors how modern
/// LLM APIs (`Anthropic`, `OpenAI`) structure conversation turns.
#[derive(Debug, Clone, PartialEq)]
pub enum MessagePart {
    /// Visible text content.
    Text(String),
    /// Model reasoning / chain-of-thought (extended thinking).
    Thinking(String),
    /// A tool call requested by the assistant.
    ToolCall(ToolCall),
    /// The result of a tool call, paired with its call id.
    ToolResult {
        tool_call_id: String,
        result: ToolResult,
    },
}

/// A conversational message, composed of one or more [`MessagePart`]s.
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub role: MessageRole,
    pub parts: Vec<MessagePart>,
}

impl Message {
    #[must_use]
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::User,
            parts: vec![MessagePart::Text(content.into())],
        }
    }

    #[must_use]
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Assistant,
            parts: vec![MessagePart::Text(content.into())],
        }
    }

    #[must_use]
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::System,
            parts: vec![MessagePart::Text(content.into())],
        }
    }

    #[must_use]
    pub fn tool_result(tool_call_id: impl Into<String>, result: ToolResult) -> Self {
        Self {
            role: MessageRole::Tool,
            parts: vec![MessagePart::ToolResult {
                tool_call_id: tool_call_id.into(),
                result,
            }],
        }
    }

    #[must_use]
    pub fn assistant_with_tools(content: impl Into<String>, tool_calls: Vec<ToolCall>) -> Self {
        let mut parts = vec![MessagePart::Text(content.into())];
        parts.extend(tool_calls.into_iter().map(MessagePart::ToolCall));
        Self {
            role: MessageRole::Assistant,
            parts,
        }
    }
}

impl Message {
    /// Returns the first `Text` part's content, or an empty string if no
    /// text part exists. This is the primary accessor replacing the old
    /// `.content` field.
    #[must_use]
    pub fn text(&self) -> &str {
        for p in &self.parts {
            if let MessagePart::Text(t) = p {
                return t;
            }
        }
        ""
    }

    /// Concatenates all `Text` parts into a single owned string.
    #[must_use]
    pub fn all_text(&self) -> String {
        self.parts
            .iter()
            .filter_map(|p| match p {
                MessagePart::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect()
    }

    /// Returns the first `Thinking` part's content, if any.
    #[must_use]
    pub fn thinking(&self) -> Option<&str> {
        self.parts.iter().find_map(|p| match p {
            MessagePart::Thinking(t) => Some(t.as_str()),
            _ => None,
        })
    }

    /// Returns references to all `ToolCall` parts.
    #[must_use]
    pub fn tool_calls(&self) -> Vec<&ToolCall> {
        self.parts
            .iter()
            .filter_map(|p| match p {
                MessagePart::ToolCall(tc) => Some(tc),
                _ => None,
            })
            .collect()
    }

    /// Returns `(tool_call_id, result)` if this message contains a
    /// `ToolResult` part.
    #[must_use]
    pub fn tool_result_info(&self) -> Option<(&str, &ToolResult)> {
        self.parts.iter().find_map(|p| match p {
            MessagePart::ToolResult {
                tool_call_id,
                result,
            } => Some((tool_call_id.as_str(), result)),
            _ => None,
        })
    }

    /// Returns `true` if any part is a `ToolCall`.
    #[must_use]
    pub fn has_tool_calls(&self) -> bool {
        self.parts
            .iter()
            .any(|p| matches!(p, MessagePart::ToolCall(_)))
    }

    /// Returns `true` if any part is a `Thinking` block.
    #[must_use]
    pub fn has_thinking(&self) -> bool {
        self.parts
            .iter()
            .any(|p| matches!(p, MessagePart::Thinking(_)))
    }
}

impl Message {
    /// Append a `Text` part.
    pub fn push_text(&mut self, text: impl Into<String>) {
        self.parts.push(MessagePart::Text(text.into()));
    }

    /// Append a `Thinking` part.
    pub fn push_thinking(&mut self, thinking: impl Into<String>) {
        self.parts.push(MessagePart::Thinking(thinking.into()));
    }

    /// Append a `ToolCall` part.
    pub fn push_tool_call(&mut self, call: ToolCall) {
        self.parts.push(MessagePart::ToolCall(call));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_message_has_text_part() {
        let m = Message::user("hello");
        assert_eq!(m.role, MessageRole::User);
        assert_eq!(m.text(), "hello");
        assert!(!m.has_tool_calls());
        assert!(!m.has_thinking());
    }

    #[test]
    fn assistant_message_has_text_part() {
        let m = Message::assistant("hi");
        assert_eq!(m.role, MessageRole::Assistant);
        assert_eq!(m.text(), "hi");
    }

    #[test]
    fn system_message_has_text_part() {
        let m = Message::system("rules");
        assert_eq!(m.role, MessageRole::System);
        assert_eq!(m.text(), "rules");
    }

    #[test]
    fn tool_result_message_has_result_info() {
        let m = Message::tool_result(
            "call_1",
            ToolResult {
                content: "ok".to_string(),
                is_error: false,
            },
        );
        assert_eq!(m.role, MessageRole::Tool);
        let (id, result) = m.tool_result_info().expect("tool result info");
        assert_eq!(id, "call_1");
        assert!(!result.is_error);
    }

    #[test]
    fn assistant_with_tools_has_calls() {
        let m = Message::assistant_with_tools(
            "thinking...",
            vec![ToolCall {
                id: "c1".to_string(),
                name: "read".to_string(),
                arguments: serde_json::json!({}),
            }],
        );
        assert_eq!(m.text(), "thinking...");
        assert!(m.has_tool_calls());
        assert_eq!(m.tool_calls().len(), 1);
        assert_eq!(m.tool_calls()[0].name, "read");
    }

    #[test]
    fn text_returns_empty_when_no_text_part() {
        let m = Message::tool_result(
            "c1",
            ToolResult {
                content: "r".to_string(),
                is_error: false,
            },
        );
        assert_eq!(m.text(), "");
    }

    #[test]
    fn all_text_concatenates_multiple_text_parts() {
        let mut m = Message::user("first");
        m.push_text("second");
        assert_eq!(m.all_text(), "firstsecond");
    }

    #[test]
    fn thinking_accessor_works() {
        let mut m = Message::assistant("answer");
        m.push_thinking("let me reason...");
        assert!(m.has_thinking());
        assert_eq!(m.thinking(), Some("let me reason..."));
    }

    #[test]
    fn push_methods_add_parts() {
        let mut m = Message::assistant("base");
        m.push_text("more text");
        m.push_thinking("reasoning");
        m.push_tool_call(ToolCall {
            id: "tc1".to_string(),
            name: "write".to_string(),
            arguments: serde_json::json!({}),
        });
        assert_eq!(m.parts.len(), 4);
        assert_eq!(m.text(), "base");
        assert!(m.has_thinking());
        assert!(m.has_tool_calls());
    }
}
