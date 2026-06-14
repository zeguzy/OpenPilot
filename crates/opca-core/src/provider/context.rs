use std::borrow::Cow;

use super::message::Message;
use super::tool::ToolDef;

#[derive(Debug, Clone, Default)]
pub struct ContextBuilder {
    system_prompt: Option<String>,
    cached_messages: Vec<Message>,
    cached_tools: Vec<ToolDef>,
}

impl ContextBuilder {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_system_prompt(&mut self, prompt: String) {
        self.system_prompt = Some(prompt);
    }

    pub fn append_message(&mut self, msg: Message) {
        self.cached_messages.push(msg);
    }

    pub fn set_tools(&mut self, tools: Vec<ToolDef>) {
        self.cached_tools = tools;
    }

    #[must_use]
    pub fn build(&self) -> ContextRef<'_> {
        ContextRef {
            system_prompt: self.system_prompt.as_deref(),
            messages: Cow::Borrowed(&self.cached_messages),
            tools: Cow::Borrowed(&self.cached_tools),
        }
    }
}

pub struct ContextRef<'a> {
    pub system_prompt: Option<&'a str>,
    pub messages: Cow<'a, [Message]>,
    pub tools: Cow<'a, [ToolDef]>,
}

impl ContextRef<'_> {
    #[must_use]
    pub fn into_owned(self) -> ContextOwned {
        ContextOwned {
            system_prompt: self.system_prompt.map(str::to_owned),
            messages: self.messages.into_owned(),
            tools: self.tools.into_owned(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ContextOwned {
    pub system_prompt: Option<String>,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDef>,
}
