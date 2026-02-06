//! Provider-agnostic request, response, and message model types.

use std::fmt::{Display, Formatter};

use fcommon::MetadataMap;

use crate::{ProviderError, ProviderErrorKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProviderId {
    OpenCodeZen,
    OpenAi,
    Claude,
}

impl Display for ProviderId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let id = match self {
            Self::OpenCodeZen => "opencode-zen",
            Self::OpenAi => "openai",
            Self::Claude => "claude",
        };

        f.write_str(id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

impl Message {
    pub fn new(role: Role, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolResult {
    pub tool_call_id: String,
    pub output: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputItem {
    Message(Message),
    ToolCall(ToolCall),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    EndTurn,
    MaxTokens,
    ToolUse,
    Cancelled,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TokenUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelResponse {
    pub provider: ProviderId,
    pub model: String,
    pub output: Vec<OutputItem>,
    pub stop_reason: StopReason,
    pub usage: TokenUsage,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub tools: Vec<ToolDefinition>,
    pub tool_results: Vec<ToolResult>,
    pub metadata: MetadataMap,
    pub stream: bool,
}

impl ModelRequest {
    pub fn builder(model: impl Into<String>) -> ModelRequestBuilder {
        ModelRequestBuilder::new(model)
    }

    pub fn new(model: impl Into<String>, messages: Vec<Message>) -> Self {
        Self {
            model: model.into(),
            messages,
            temperature: None,
            max_tokens: None,
            tools: Vec::new(),
            tool_results: Vec::new(),
            metadata: MetadataMap::new(),
            stream: false,
        }
    }

    pub fn new_validated(
        model: impl Into<String>,
        messages: Vec<Message>,
    ) -> Result<Self, ProviderError> {
        let request = Self::new(model, messages);
        request.validate()?;
        Ok(request)
    }

    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    pub fn with_tools(mut self, tools: Vec<ToolDefinition>) -> Self {
        self.tools = tools;
        self
    }

    pub fn with_tool_results(mut self, tool_results: Vec<ToolResult>) -> Self {
        self.tool_results = tool_results;
        self
    }

    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    pub fn enable_streaming(mut self) -> Self {
        self.stream = true;
        self
    }

    pub fn validate(&self) -> Result<(), ProviderError> {
        if self.model.trim().is_empty() {
            return Err(ProviderError::invalid_request("model must not be empty"));
        }

        if self.messages.is_empty() {
            return Err(ProviderError::invalid_request(
                "at least one message is required",
            ));
        }

        if let Some(max_tokens) = self.max_tokens {
            if max_tokens == 0 {
                return Err(ProviderError::invalid_request(
                    "max_tokens must be greater than zero",
                ));
            }
        }

        if let Some(temperature) = self.temperature {
            if !(0.0..=2.0).contains(&temperature) {
                return Err(ProviderError::new(
                    ProviderErrorKind::InvalidRequest,
                    "temperature must be in the inclusive range 0.0..=2.0",
                    false,
                ));
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelRequestBuilder {
    model: String,
    messages: Vec<Message>,
    temperature: Option<f32>,
    max_tokens: Option<u32>,
    tools: Vec<ToolDefinition>,
    tool_results: Vec<ToolResult>,
    metadata: MetadataMap,
    stream: bool,
}

impl ModelRequestBuilder {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            messages: Vec::new(),
            temperature: None,
            max_tokens: None,
            tools: Vec::new(),
            tool_results: Vec::new(),
            metadata: MetadataMap::new(),
            stream: false,
        }
    }

    pub fn message(mut self, message: Message) -> Self {
        self.messages.push(message);
        self
    }

    pub fn messages(mut self, messages: Vec<Message>) -> Self {
        self.messages.extend(messages);
        self
    }

    pub fn temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    pub fn max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    pub fn tools(mut self, tools: Vec<ToolDefinition>) -> Self {
        self.tools = tools;
        self
    }

    pub fn tool_results(mut self, tool_results: Vec<ToolResult>) -> Self {
        self.tool_results = tool_results;
        self
    }

    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    pub fn streaming(mut self, stream: bool) -> Self {
        self.stream = stream;
        self
    }

    pub fn enable_streaming(self) -> Self {
        self.streaming(true)
    }

    pub fn build(self) -> Result<ModelRequest, ProviderError> {
        let request = ModelRequest {
            model: self.model,
            messages: self.messages,
            temperature: self.temperature,
            max_tokens: self.max_tokens,
            tools: self.tools,
            tool_results: self.tool_results,
            metadata: self.metadata,
            stream: self.stream,
        };

        request.validate()?;
        Ok(request)
    }
}
