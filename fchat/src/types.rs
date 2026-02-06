//! Chat session, turn, and chat event types.

use std::pin::Pin;

use fprovider::{ProviderId, StopReason, TokenUsage, ToolCall};
use futures_core::Stream;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatSession {
    pub id: String,
    pub provider: ProviderId,
    pub model: String,
    pub system_prompt: Option<String>,
}

impl ChatSession {
    pub fn new(id: impl Into<String>, provider: ProviderId, model: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            provider,
            model: model.into(),
            system_prompt: None,
        }
    }

    pub fn with_system_prompt(mut self, system_prompt: impl Into<String>) -> Self {
        self.system_prompt = Some(system_prompt.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChatTurnRequest {
    pub session: ChatSession,
    pub user_input: String,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub stream: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ChatTurnOptions {
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub stream: bool,
}

impl ChatTurnRequest {
    pub fn new(session: ChatSession, user_input: impl Into<String>) -> Self {
        Self {
            session,
            user_input: user_input.into(),
            temperature: None,
            max_tokens: None,
            stream: false,
        }
    }

    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    pub fn enable_streaming(mut self) -> Self {
        self.stream = true;
        self
    }

    pub fn with_options(mut self, options: ChatTurnOptions) -> Self {
        self.temperature = options.temperature;
        self.max_tokens = options.max_tokens;
        self.stream = options.stream;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatTurnResult {
    pub session_id: String,
    pub assistant_message: String,
    pub tool_calls: Vec<ToolCall>,
    pub stop_reason: StopReason,
    pub usage: TokenUsage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatEvent {
    TextDelta(String),
    ToolCallDelta(ToolCall),
    AssistantMessageComplete(String),
    TurnComplete(ChatTurnResult),
}

pub type ChatEventStream<'a> =
    Pin<Box<dyn Stream<Item = Result<ChatEvent, crate::ChatError>> + Send + 'a>>;
