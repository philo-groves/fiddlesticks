use std::collections::BTreeMap;
use std::fmt::Formatter;
use std::sync::Arc;
use std::time::SystemTime;

use futures_util::StreamExt;
use reqwest::{Client, Response, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    BoxedEventStream, BrowserLoginSession, Message, ModelProvider, ModelRequest, ModelResponse,
    OutputItem, ProviderError, ProviderFuture, ProviderId, Role, SecureCredentialManager,
    StopReason, StreamEvent, TokenUsage, ToolCall, ToolDefinition, ToolResult, VecEventStream,
};

impl SecureCredentialManager {
    pub fn set_openai_api_key(&self, api_key: impl Into<String>) -> Result<(), ProviderError> {
        let api_key = api_key.into();
        if !api_key.starts_with("sk-") {
            return Err(ProviderError::authentication(
                "OpenAI API key must start with 'sk-'",
            ));
        }

        self.set_api_key(ProviderId::OpenAi, api_key)
    }

    pub fn set_openai_browser_session(
        &self,
        session_token: impl Into<String>,
        expires_at: Option<SystemTime>,
    ) -> Result<(), ProviderError> {
        self.set_browser_session(ProviderId::OpenAi, session_token, expires_at)
    }
}

#[derive(Clone)]
pub struct OpenAiProvider {
    credentials: Arc<SecureCredentialManager>,
    transport: Arc<dyn OpenAiTransport>,
    fallback_model: String,
}

impl OpenAiProvider {
    pub fn new(
        credentials: Arc<SecureCredentialManager>,
        transport: Arc<dyn OpenAiTransport>,
    ) -> Self {
        Self {
            credentials,
            transport,
            fallback_model: "gpt-4o-mini".to_string(),
        }
    }

    pub fn with_fallback_model(mut self, model: impl Into<String>) -> Self {
        self.fallback_model = model.into();
        self
    }

    fn build_openai_request(&self, request: ModelRequest, stream: bool) -> OpenAiRequest {
        let model = if request.model.trim().is_empty() {
            self.fallback_model.clone()
        } else {
            request.model
        };

        let mut messages = request
            .messages
            .into_iter()
            .map(OpenAiMessage::from)
            .collect::<Vec<_>>();

        for tool_result in request.tool_results {
            messages.push(OpenAiMessage::tool_result(tool_result));
        }

        let tools = request.tools.into_iter().map(OpenAiTool::from).collect::<Vec<_>>();

        OpenAiRequest {
            model,
            messages,
            tools,
            temperature: request.temperature,
            max_tokens: request.max_tokens,
            stream,
        }
    }

    fn resolve_auth(&self) -> Result<OpenAiAuth, ProviderError> {
        if let Some(api_key) = self
            .credentials
            .with_api_key(ProviderId::OpenAi, |value| value.to_string())?
        {
            return Ok(OpenAiAuth::ApiKey(api_key));
        }

        if let Some(session) = self
            .credentials
            .with_browser_session(ProviderId::OpenAi, clone_session)?
        {
            if let Some(expires_at) = session.expires_at {
                if expires_at <= SystemTime::now() {
                    return Err(ProviderError::authentication(
                        "OpenAI browser session has expired",
                    ));
                }
            }

            return Ok(OpenAiAuth::BrowserSession(
                session.session_token.expose().to_string(),
            ));
        }

        Err(ProviderError::authentication(
            "no OpenAI credentials configured",
        ))
    }

    fn convert_response(response: OpenAiResponse) -> ModelResponse {
        let mut output = Vec::new();
        if !response.message.content.is_empty() {
            output.push(OutputItem::Message(Message::new(
                Role::Assistant,
                response.message.content,
            )));
        }

        output.extend(
            response
                .message
                .tool_calls
                .into_iter()
                .map(|tool_call| OutputItem::ToolCall(ToolCall::from(tool_call))),
        );

        ModelResponse {
            provider: ProviderId::OpenAi,
            model: response.model,
            output,
            stop_reason: response.finish_reason.into(),
            usage: response.usage.into(),
        }
    }
}

impl ModelProvider for OpenAiProvider {
    fn id(&self) -> ProviderId {
        ProviderId::OpenAi
    }

    fn complete<'a>(
        &'a self,
        request: ModelRequest,
    ) -> ProviderFuture<'a, Result<ModelResponse, ProviderError>> {
        Box::pin(async move {
            request.validate()?;
            let auth = self.resolve_auth()?;
            let openai_request = self.build_openai_request(request, false);
            let response = self.transport.complete(openai_request, auth).await?;
            Ok(Self::convert_response(response))
        })
    }

    fn stream<'a>(
        &'a self,
        request: ModelRequest,
    ) -> ProviderFuture<'a, Result<BoxedEventStream<'a>, ProviderError>> {
        Box::pin(async move {
            request.validate()?;
            let auth = self.resolve_auth()?;
            let openai_request = self.build_openai_request(request, true);
            let chunks = self.transport.stream(openai_request, auth).await?;

            let events = chunks
                .into_iter()
                .map(StreamEvent::from)
                .map(Ok)
                .collect::<Vec<_>>();

            let stream = VecEventStream::new(events);
            Ok(Box::pin(stream) as BoxedEventStream<'a>)
        })
    }
}

pub trait OpenAiTransport: Send + Sync + std::fmt::Debug {
    fn complete<'a>(
        &'a self,
        request: OpenAiRequest,
        auth: OpenAiAuth,
    ) -> ProviderFuture<'a, Result<OpenAiResponse, ProviderError>>;

    fn stream<'a>(
        &'a self,
        request: OpenAiRequest,
        auth: OpenAiAuth,
    ) -> ProviderFuture<'a, Result<Vec<OpenAiStreamChunk>, ProviderError>>;
}

#[derive(Debug, Clone)]
pub struct OpenAiHttpTransport {
    client: Client,
    base_url: String,
}

impl OpenAiHttpTransport {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            base_url: "https://api.openai.com/v1".to_string(),
        }
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    fn endpoint(&self, path: &str) -> String {
        format!("{}/{}", self.base_url.trim_end_matches('/'), path)
    }

    fn build_api_request(request: OpenAiRequest) -> Result<OpenAiApiRequest, ProviderError> {
        let mut messages = request
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
            messages: {
                messages.shrink_to_fit();
                messages
            },
            tools,
            temperature: request.temperature,
            max_tokens: request.max_tokens,
            stream: request.stream,
        })
    }

    fn apply_auth(
        &self,
        builder: reqwest::RequestBuilder,
        auth: &OpenAiAuth,
    ) -> reqwest::RequestBuilder {
        match auth {
            OpenAiAuth::ApiKey(key) => builder.bearer_auth(key),
            OpenAiAuth::BrowserSession(token) => {
                builder.header("Cookie", format!("__Secure-next-auth.session-token={token}"))
            }
        }
    }

    async fn parse_error(response: Response) -> ProviderError {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let message = extract_error_message(&body)
            .unwrap_or_else(|| format!("OpenAI request failed with status {status}"));

        match status {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                ProviderError::authentication(message)
            }
            StatusCode::TOO_MANY_REQUESTS => ProviderError::rate_limited(message),
            StatusCode::REQUEST_TIMEOUT | StatusCode::GATEWAY_TIMEOUT => ProviderError::timeout(message),
            StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY => {
                ProviderError::invalid_request(message)
            }
            StatusCode::SERVICE_UNAVAILABLE | StatusCode::BAD_GATEWAY => {
                ProviderError::unavailable(message)
            }
            _ => ProviderError::transport(message),
        }
    }

    fn parse_finish_reason(value: Option<&str>) -> OpenAiFinishReason {
        match value {
            Some("stop") => OpenAiFinishReason::Stop,
            Some("length") => OpenAiFinishReason::Length,
            Some("tool_calls") => OpenAiFinishReason::ToolCalls,
            Some("cancelled") => OpenAiFinishReason::Cancelled,
            _ => OpenAiFinishReason::Other,
        }
    }
}

impl OpenAiTransport for OpenAiHttpTransport {
    fn complete<'a>(
        &'a self,
        request: OpenAiRequest,
        auth: OpenAiAuth,
    ) -> ProviderFuture<'a, Result<OpenAiResponse, ProviderError>> {
        Box::pin(async move {
            let api_request = Self::build_api_request(request)?;
            let url = self.endpoint("chat/completions");
            let builder = self.client.post(url).json(&api_request);
            let response = self.apply_auth(builder, &auth).send().await.map_err(|err| {
                if err.is_timeout() {
                    ProviderError::timeout(err.to_string())
                } else {
                    ProviderError::transport(err.to_string())
                }
            })?;

            if !response.status().is_success() {
                return Err(Self::parse_error(response).await);
            }

            let parsed: OpenAiApiResponse = response
                .json()
                .await
                .map_err(|err| ProviderError::transport(err.to_string()))?;

            OpenAiResponse::try_from(parsed)
        })
    }

    fn stream<'a>(
        &'a self,
        mut request: OpenAiRequest,
        auth: OpenAiAuth,
    ) -> ProviderFuture<'a, Result<Vec<OpenAiStreamChunk>, ProviderError>> {
        Box::pin(async move {
            request.stream = true;
            let model_for_fallback = request.model.clone();
            let api_request = Self::build_api_request(request)?;
            let url = self.endpoint("chat/completions");
            let builder = self.client.post(url).json(&api_request);
            let response = self.apply_auth(builder, &auth).send().await.map_err(|err| {
                if err.is_timeout() {
                    ProviderError::timeout(err.to_string())
                } else {
                    ProviderError::transport(err.to_string())
                }
            })?;

            if !response.status().is_success() {
                return Err(Self::parse_error(response).await);
            }

            let mut chunks = response.bytes_stream();
            let mut sse_buffer = String::new();
            let mut events = Vec::new();
            let mut finished = false;
            let mut content = String::new();
            let mut tool_calls: BTreeMap<u32, OpenAiToolCall> = BTreeMap::new();
            let mut model = None::<String>;
            let mut finish_reason = OpenAiFinishReason::Other;

            while let Some(item) = chunks.next().await {
                let bytes = item.map_err(|err| ProviderError::transport(err.to_string()))?;
                let text = std::str::from_utf8(&bytes)
                    .map_err(|err| ProviderError::transport(err.to_string()))?;
                sse_buffer.push_str(text);

                while let Some(newline_index) = sse_buffer.find('\n') {
                    let line = sse_buffer.drain(..=newline_index).collect::<String>();
                    let line = line.trim();

                    if !line.starts_with("data:") {
                        continue;
                    }

                    let payload = line.trim_start_matches("data:").trim();
                    if payload == "[DONE]" {
                        finished = true;
                        break;
                    }

                    let parsed: OpenAiApiStreamResponse = serde_json::from_str(payload)
                        .map_err(|err| ProviderError::transport(err.to_string()))?;

                    if model.is_none() {
                        model = Some(parsed.model.clone());
                    }

                    if let Some(choice) = parsed.choices.first() {
                        if let Some(delta_content) = &choice.delta.content {
                            if !delta_content.is_empty() {
                                content.push_str(delta_content);
                                events.push(OpenAiStreamChunk::TextDelta(delta_content.clone()));
                            }
                        }

                        if let Some(delta_tool_calls) = &choice.delta.tool_calls {
                            for delta_call in delta_tool_calls {
                                let index = delta_call.index.unwrap_or(0);
                                let entry = tool_calls.entry(index).or_insert_with(|| OpenAiToolCall {
                                    id: delta_call
                                        .id
                                        .clone()
                                        .unwrap_or_else(|| format!("tool_call_{index}")),
                                    name: String::new(),
                                    arguments: String::new(),
                                });

                                if let Some(id) = &delta_call.id {
                                    entry.id = id.clone();
                                }

                                if let Some(function) = &delta_call.function {
                                    if let Some(name) = &function.name {
                                        entry.name = name.clone();
                                    }

                                    if let Some(arguments) = &function.arguments {
                                        entry.arguments.push_str(arguments);
                                    }
                                }

                                events.push(OpenAiStreamChunk::ToolCallDelta(entry.clone()));
                            }
                        }

                        if choice.finish_reason.is_some() {
                            finish_reason = Self::parse_finish_reason(choice.finish_reason.as_deref());
                        }
                    }
                }

                if finished {
                    break;
                }
            }

            let final_message = OpenAiAssistantMessage {
                content,
                tool_calls: tool_calls.into_values().collect(),
            };

            events.push(OpenAiStreamChunk::MessageComplete(final_message.clone()));
            events.push(OpenAiStreamChunk::ResponseComplete(OpenAiResponse {
                model: model.unwrap_or(model_for_fallback),
                message: final_message,
                finish_reason,
                usage: OpenAiUsage {
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    total_tokens: 0,
                },
            }));

            Ok(events)
        })
    }
}

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
    fn tool_result(tool_result: ToolResult) -> Self {
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
    fn as_str(self) -> &'static str {
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
    ApiKey(String),
    BrowserSession(String),
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
                Self::ResponseComplete(OpenAiProvider::convert_response(response))
            }
        }
    }
}

fn clone_session(session: &BrowserLoginSession) -> BrowserLoginSession {
    BrowserLoginSession::new(session.session_token.expose(), session.expires_at)
}

fn extract_error_message(body: &str) -> Option<String> {
    let parsed = serde_json::from_str::<OpenAiApiErrorEnvelope>(body).ok()?;
    Some(parsed.error.message)
}

#[derive(Debug, Deserialize)]
struct OpenAiApiErrorEnvelope {
    error: OpenAiApiError,
}

#[derive(Debug, Deserialize)]
struct OpenAiApiError {
    message: String,
}

#[derive(Debug, Serialize)]
struct OpenAiApiRequest {
    model: String,
    messages: Vec<OpenAiApiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAiApiTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    stream: bool,
}

#[derive(Debug, Serialize)]
struct OpenAiApiMessage {
    role: String,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
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
struct OpenAiApiTool {
    r#type: String,
    function: OpenAiApiFunction,
}

impl TryFrom<OpenAiTool> for OpenAiApiTool {
    type Error = ProviderError;

    fn try_from(value: OpenAiTool) -> Result<Self, Self::Error> {
        let parameters = serde_json::from_str::<Value>(&value.input_schema).map_err(|_| {
            ProviderError::invalid_request("OpenAI tool schema must be valid JSON")
        })?;

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
struct OpenAiApiFunction {
    name: String,
    description: String,
    parameters: Value,
}

#[derive(Debug, Deserialize)]
struct OpenAiApiResponse {
    model: String,
    choices: Vec<OpenAiApiChoice>,
    usage: Option<OpenAiApiUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenAiApiChoice {
    message: OpenAiApiAssistantMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiApiAssistantMessage {
    content: Option<String>,
    tool_calls: Option<Vec<OpenAiApiToolCall>>,
}

#[derive(Debug, Deserialize)]
struct OpenAiApiToolCall {
    id: String,
    function: OpenAiApiToolFunction,
}

#[derive(Debug, Deserialize)]
struct OpenAiApiToolFunction {
    name: String,
    arguments: String,
}

#[derive(Debug, Deserialize)]
struct OpenAiApiUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

impl TryFrom<OpenAiApiResponse> for OpenAiResponse {
    type Error = ProviderError;

    fn try_from(value: OpenAiApiResponse) -> Result<Self, Self::Error> {
        let choice = value
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| ProviderError::transport("OpenAI response did not include choices"))?;

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
            finish_reason: OpenAiHttpTransport::parse_finish_reason(choice.finish_reason.as_deref()),
            usage: OpenAiUsage {
                prompt_tokens: usage.prompt_tokens,
                completion_tokens: usage.completion_tokens,
                total_tokens: usage.total_tokens,
            },
        })
    }
}

#[derive(Debug, Deserialize)]
struct OpenAiApiStreamResponse {
    model: String,
    choices: Vec<OpenAiApiStreamChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenAiApiStreamChoice {
    delta: OpenAiApiStreamDelta,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiApiStreamDelta {
    content: Option<String>,
    tool_calls: Option<Vec<OpenAiApiDeltaToolCall>>,
}

#[derive(Debug, Deserialize)]
struct OpenAiApiDeltaToolCall {
    index: Option<u32>,
    id: Option<String>,
    function: Option<OpenAiApiDeltaToolFunction>,
}

#[derive(Debug, Deserialize)]
struct OpenAiApiDeltaToolFunction {
    name: Option<String>,
    arguments: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;
    use std::sync::Mutex;
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct CapturedAuth(&'static str, String);

    #[derive(Debug, Default)]
    struct FakeTransport {
        captured_auth: Mutex<Option<CapturedAuth>>,
        captured_request: Mutex<Option<OpenAiRequest>>,
    }

    impl OpenAiTransport for FakeTransport {
        fn complete<'a>(
            &'a self,
            request: OpenAiRequest,
            auth: OpenAiAuth,
        ) -> ProviderFuture<'a, Result<OpenAiResponse, ProviderError>> {
            Box::pin(async move {
                *self.captured_request.lock().expect("request lock") = Some(request);
                *self.captured_auth.lock().expect("auth lock") = Some(match auth {
                    OpenAiAuth::ApiKey(value) => CapturedAuth("api_key", value),
                    OpenAiAuth::BrowserSession(value) => CapturedAuth("browser_session", value),
                });

                Ok(OpenAiResponse {
                    model: "gpt-4o-mini".to_string(),
                    message: OpenAiAssistantMessage {
                        content: "hello world".to_string(),
                        tool_calls: vec![OpenAiToolCall {
                            id: "call_1".to_string(),
                            name: "lookup".to_string(),
                            arguments: "{\"id\":1}".to_string(),
                        }],
                    },
                    finish_reason: OpenAiFinishReason::ToolCalls,
                    usage: OpenAiUsage {
                        prompt_tokens: 7,
                        completion_tokens: 3,
                        total_tokens: 10,
                    },
                })
            })
        }

        fn stream<'a>(
            &'a self,
            request: OpenAiRequest,
            auth: OpenAiAuth,
        ) -> ProviderFuture<'a, Result<Vec<OpenAiStreamChunk>, ProviderError>> {
            Box::pin(async move {
                *self.captured_request.lock().expect("request lock") = Some(request);
                *self.captured_auth.lock().expect("auth lock") = Some(match auth {
                    OpenAiAuth::ApiKey(value) => CapturedAuth("api_key", value),
                    OpenAiAuth::BrowserSession(value) => CapturedAuth("browser_session", value),
                });

                Ok(vec![
                    OpenAiStreamChunk::TextDelta("hello".to_string()),
                    OpenAiStreamChunk::TextDelta(" world".to_string()),
                ])
            })
        }
    }

    #[test]
    fn complete_maps_openai_response_to_provider_response() {
        let credentials = Arc::new(SecureCredentialManager::new());
        credentials
            .set_openai_api_key("sk-live-123")
            .expect("key should set");

        let transport = Arc::new(FakeTransport::default());
        let provider = OpenAiProvider::new(credentials, transport.clone());
        let request = ModelRequest::new("gpt-4o", vec![Message::new(Role::User, "hi")])
            .with_tools(vec![ToolDefinition {
                name: "lookup".to_string(),
                description: "Look up ID".to_string(),
                input_schema: "{\"type\":\"object\"}".to_string(),
            }])
            .with_tool_results(vec![ToolResult {
                tool_call_id: "call_0".to_string(),
                output: "{\"ok\":true}".to_string(),
            }]);

        let response = block_on(provider.complete(request)).expect("completion should succeed");
        assert_eq!(response.provider, ProviderId::OpenAi);
        assert_eq!(response.stop_reason, StopReason::ToolUse);
        assert_eq!(response.usage.total_tokens, 10);
        assert_eq!(response.output.len(), 2);

        let auth = transport
            .captured_auth
            .lock()
            .expect("auth lock")
            .clone()
            .expect("auth should be captured");
        assert_eq!(auth, CapturedAuth("api_key", "sk-live-123".to_string()));

        let captured_request = transport
            .captured_request
            .lock()
            .expect("request lock")
            .clone()
            .expect("request should be captured");
        assert_eq!(captured_request.model, "gpt-4o");
        assert_eq!(captured_request.messages.len(), 2);
        assert!(!captured_request.stream);
    }

    #[test]
    fn stream_prefers_browser_session_when_api_key_missing() {
        let credentials = Arc::new(SecureCredentialManager::new());
        credentials
            .set_openai_browser_session("session-xyz", None)
            .expect("session should set");

        let transport = Arc::new(FakeTransport::default());
        let provider = OpenAiProvider::new(credentials, transport.clone());
        let request = ModelRequest::new("gpt-4o-mini", vec![Message::new(Role::User, "hi")]);

        let mut stream = block_on(provider.stream(request)).expect("stream should succeed");
        let waker = noop_waker();
        let mut cx = Context::from_waker(&waker);

        let first = stream.as_mut().poll_next(&mut cx);
        assert_eq!(
            first,
            Poll::Ready(Some(Ok(StreamEvent::TextDelta("hello".to_string()))))
        );

        let auth = transport
            .captured_auth
            .lock()
            .expect("auth lock")
            .clone()
            .expect("auth should be captured");
        assert_eq!(auth, CapturedAuth("browser_session", "session-xyz".to_string()));

        let captured_request = transport
            .captured_request
            .lock()
            .expect("request lock")
            .clone()
            .expect("request should be captured");
        assert!(captured_request.stream);
    }

    #[test]
    fn missing_openai_credentials_returns_auth_error() {
        let credentials = Arc::new(SecureCredentialManager::new());
        let transport = Arc::new(FakeTransport::default());
        let provider = OpenAiProvider::new(credentials, transport);
        let request = ModelRequest::new("gpt-4o-mini", vec![Message::new(Role::User, "hi")]);

        let error = block_on(provider.complete(request)).expect_err("missing creds should fail");
        assert_eq!(error.kind, crate::ProviderErrorKind::Authentication);
        assert_eq!(error.message, "no OpenAI credentials configured");
    }

    fn block_on<F: Future>(future: F) -> F::Output {
        let mut future = std::pin::pin!(future);
        let waker = noop_waker();
        let mut cx = Context::from_waker(&waker);

        loop {
            match future.as_mut().poll(&mut cx) {
                Poll::Ready(value) => return value,
                Poll::Pending => std::thread::yield_now(),
            }
        }
    }

    fn noop_waker() -> Waker {
        unsafe fn clone(_: *const ()) -> RawWaker {
            RawWaker::new(std::ptr::null(), &VTABLE)
        }

        unsafe fn wake(_: *const ()) {}

        unsafe fn wake_by_ref(_: *const ()) {}

        unsafe fn drop(_: *const ()) {}

        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop);

        let raw_waker = RawWaker::new(std::ptr::null(), &VTABLE);
        unsafe { Waker::from_raw(raw_waker) }
    }
}
