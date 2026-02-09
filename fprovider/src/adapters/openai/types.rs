//! OpenAI adapter types and provider-agnostic conversion logic.

use std::fmt::Formatter;

use crate::{
    Message, ModelResponse, OutputItem, ProviderId, Role, SecretString, StopReason, StreamEvent,
    TokenUsage, ToolCall, ToolDefinition, ToolResult,
};

#[derive(Debug, Clone, PartialEq)]
pub struct OpenAiRequest {
    pub model: String,
    pub messages: Vec<OpenAiMessage>,
    pub tools: Vec<OpenAiTool>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub stream: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenAiMessage {
    pub role: OpenAiRole,
    pub content: String,
    pub tool_call_id: Option<String>,
}

impl OpenAiMessage {
    pub(crate) fn tool_result(tool_result: ToolResult) -> Self {
        Self {
            role: OpenAiRole::Tool,
            content: tool_result.output,
            tool_call_id: Some(tool_result.tool_call_id),
        }
    }
}

impl From<Message> for OpenAiMessage {
    fn from(value: Message) -> Self {
        Self {
            role: value.role.into(),
            content: value.content,
            tool_call_id: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenAiRole {
    System,
    User,
    Assistant,
    Tool,
}

impl OpenAiRole {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Tool => "tool",
        }
    }
}

impl From<Role> for OpenAiRole {
    fn from(value: Role) -> Self {
        match value {
            Role::System => Self::System,
            Role::User => Self::User,
            Role::Assistant => Self::Assistant,
            Role::Tool => Self::Tool,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenAiTool {
    pub name: String,
    pub description: String,
    pub input_schema: String,
}

impl From<ToolDefinition> for OpenAiTool {
    fn from(value: ToolDefinition) -> Self {
        Self {
            name: value.name,
            description: value.description,
            input_schema: value.input_schema,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenAiResponse {
    pub model: String,
    pub message: OpenAiAssistantMessage,
    pub finish_reason: OpenAiFinishReason,
    pub usage: OpenAiUsage,
}

impl OpenAiResponse {
    pub(crate) fn into_model_response(self) -> ModelResponse {
        let mut output = Vec::new();
        if !self.message.content.is_empty() {
            output.push(OutputItem::Message(Message::new(
                Role::Assistant,
                self.message.content,
            )));
        }

        output.extend(
            self.message
                .tool_calls
                .into_iter()
                .map(|tool_call| OutputItem::ToolCall(ToolCall::from(tool_call))),
        );

        ModelResponse {
            provider: ProviderId::OpenAi,
            model: self.model,
            output,
            stop_reason: self.finish_reason.into(),
            usage: self.usage.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenAiAssistantMessage {
    pub content: String,
    pub tool_calls: Vec<OpenAiToolCall>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenAiToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

impl From<OpenAiToolCall> for ToolCall {
    fn from(value: OpenAiToolCall) -> Self {
        Self {
            id: value.id,
            name: value.name,
            arguments: value.arguments,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenAiFinishReason {
    Stop,
    Length,
    ToolCalls,
    Cancelled,
    Other,
}

impl From<OpenAiFinishReason> for StopReason {
    fn from(value: OpenAiFinishReason) -> Self {
        match value {
            OpenAiFinishReason::Stop => Self::EndTurn,
            OpenAiFinishReason::Length => Self::MaxTokens,
            OpenAiFinishReason::ToolCalls => Self::ToolUse,
            OpenAiFinishReason::Cancelled => Self::Cancelled,
            OpenAiFinishReason::Other => Self::Other,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenAiUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

impl From<OpenAiUsage> for TokenUsage {
    fn from(value: OpenAiUsage) -> Self {
        Self {
            input_tokens: value.prompt_tokens,
            output_tokens: value.completion_tokens,
            total_tokens: value.total_tokens,
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum OpenAiAuth {
    ApiKey(SecretString),
    BrowserSession(SecretString),
}

impl std::fmt::Debug for OpenAiAuth {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ApiKey(_) => f.write_str("OpenAiAuth::ApiKey([REDACTED])"),
            Self::BrowserSession(_) => f.write_str("OpenAiAuth::BrowserSession([REDACTED])"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenAiStreamChunk {
    TextDelta(String),
    ToolCallDelta(OpenAiToolCall),
    MessageComplete(OpenAiAssistantMessage),
    ResponseComplete(OpenAiResponse),
}

impl From<OpenAiStreamChunk> for StreamEvent {
    fn from(value: OpenAiStreamChunk) -> Self {
        match value {
            OpenAiStreamChunk::TextDelta(delta) => Self::TextDelta(delta),
            OpenAiStreamChunk::ToolCallDelta(tool_call) => Self::ToolCallDelta(tool_call.into()),
            OpenAiStreamChunk::MessageComplete(message) => {
                Self::MessageComplete(Message::new(Role::Assistant, message.content))
            }
            OpenAiStreamChunk::ResponseComplete(response) => {
                Self::ResponseComplete(response.into_model_response())
            }
        }
    }
}
