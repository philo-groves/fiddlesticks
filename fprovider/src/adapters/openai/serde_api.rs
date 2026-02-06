//! OpenAI HTTP payload serde models and conversion helpers.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ProviderError;

use super::types::{
    OpenAiAssistantMessage, OpenAiFinishReason, OpenAiMessage, OpenAiRequest, OpenAiResponse,
    OpenAiRole, OpenAiTool, OpenAiToolCall, OpenAiUsage,
};

pub(crate) fn build_api_request(request: OpenAiRequest) -> Result<OpenAiApiRequest, ProviderError> {
    let messages = request
        .messages
        .into_iter()
        .map(OpenAiApiMessage::try_from)
        .collect::<Result<Vec<_>, _>>()?;

    if messages.is_empty() {
        return Err(ProviderError::invalid_request(
            "OpenAI request requires at least one message",
        ));
    }

    let tools = if request.tools.is_empty() {
        None
    } else {
        Some(
            request
                .tools
                .into_iter()
                .map(OpenAiApiTool::try_from)
                .collect::<Result<Vec<_>, _>>()?,
        )
    };

    Ok(OpenAiApiRequest {
        model: request.model,
        messages,
        tools,
        temperature: request.temperature,
        max_tokens: request.max_tokens,
        stream: request.stream,
    })
}

pub(crate) fn parse_finish_reason(value: Option<&str>) -> OpenAiFinishReason {
    match value {
        Some("stop") => OpenAiFinishReason::Stop,
        Some("length") => OpenAiFinishReason::Length,
        Some("tool_calls") => OpenAiFinishReason::ToolCalls,
        Some("cancelled") => OpenAiFinishReason::Cancelled,
        _ => OpenAiFinishReason::Other,
    }
}

pub(crate) fn extract_error_message(body: &str) -> Option<String> {
    let parsed = serde_json::from_str::<OpenAiApiErrorEnvelope>(body).ok()?;
    Some(parsed.error.message)
}

#[derive(Debug, Deserialize)]
pub(crate) struct OpenAiApiErrorEnvelope {
    pub error: OpenAiApiError,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OpenAiApiError {
    pub message: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct OpenAiApiRequest {
    pub model: String,
    pub messages: Vec<OpenAiApiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<OpenAiApiTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    pub stream: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct OpenAiApiMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl TryFrom<OpenAiMessage> for OpenAiApiMessage {
    type Error = ProviderError;

    fn try_from(value: OpenAiMessage) -> Result<Self, Self::Error> {
        if value.content.trim().is_empty() && value.role != OpenAiRole::Assistant {
            return Err(ProviderError::invalid_request(
                "OpenAI message content must not be empty",
            ));
        }

        Ok(Self {
            role: value.role.as_str().to_string(),
            content: value.content,
            tool_call_id: value.tool_call_id,
        })
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct OpenAiApiTool {
    pub r#type: String,
    pub function: OpenAiApiFunction,
}

impl TryFrom<OpenAiTool> for OpenAiApiTool {
    type Error = ProviderError;

    fn try_from(value: OpenAiTool) -> Result<Self, Self::Error> {
        let parameters = serde_json::from_str::<Value>(&value.input_schema)
            .map_err(|_| ProviderError::invalid_request("OpenAI tool schema must be valid JSON"))?;

        Ok(Self {
            r#type: "function".to_string(),
            function: OpenAiApiFunction {
                name: value.name,
                description: value.description,
                parameters,
            },
        })
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct OpenAiApiFunction {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OpenAiApiResponse {
    pub model: String,
    pub choices: Vec<OpenAiApiChoice>,
    pub usage: Option<OpenAiApiUsage>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OpenAiApiChoice {
    pub message: OpenAiApiAssistantMessage,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OpenAiApiAssistantMessage {
    pub content: Option<String>,
    pub tool_calls: Option<Vec<OpenAiApiToolCall>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OpenAiApiToolCall {
    pub id: String,
    pub function: OpenAiApiToolFunction,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OpenAiApiToolFunction {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OpenAiApiUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

impl TryFrom<OpenAiApiResponse> for OpenAiResponse {
    type Error = ProviderError;

    fn try_from(value: OpenAiApiResponse) -> Result<Self, Self::Error> {
        let choice =
            value.choices.into_iter().next().ok_or_else(|| {
                ProviderError::transport("OpenAI response did not include choices")
            })?;

        let tool_calls = choice
            .message
            .tool_calls
            .unwrap_or_default()
            .into_iter()
            .map(|call| OpenAiToolCall {
                id: call.id,
                name: call.function.name,
                arguments: call.function.arguments,
            })
            .collect::<Vec<_>>();

        let usage = value.usage.unwrap_or(OpenAiApiUsage {
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
        });

        Ok(Self {
            model: value.model,
            message: OpenAiAssistantMessage {
                content: choice.message.content.unwrap_or_default(),
                tool_calls,
            },
            finish_reason: parse_finish_reason(choice.finish_reason.as_deref()),
            usage: OpenAiUsage {
                prompt_tokens: usage.prompt_tokens,
                completion_tokens: usage.completion_tokens,
                total_tokens: usage.total_tokens,
            },
        })
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct OpenAiApiStreamResponse {
    pub model: String,
    pub choices: Vec<OpenAiApiStreamChoice>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OpenAiApiStreamChoice {
    pub delta: OpenAiApiStreamDelta,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OpenAiApiStreamDelta {
    pub content: Option<String>,
    pub tool_calls: Option<Vec<OpenAiApiDeltaToolCall>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OpenAiApiDeltaToolCall {
    pub index: Option<u32>,
    pub id: Option<String>,
    pub function: Option<OpenAiApiDeltaToolFunction>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OpenAiApiDeltaToolFunction {
    pub name: Option<String>,
    pub arguments: Option<String>,
}
