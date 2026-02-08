//! Anthropic provider implemented over OpenAI-compatible transport.

use std::sync::Arc;

use async_stream::try_stream;
use futures_util::StreamExt;
use reqwest::Client;

use crate::adapters::openai::{
    OpenAiAuth, OpenAiHttpTransport, OpenAiMessage, OpenAiRequest, OpenAiStreamChunk, OpenAiTool,
    OpenAiTransport,
};
use crate::{
    BoxedEventStream, Message, ModelProvider, ModelRequest, ModelResponse, ProviderError,
    ProviderFuture, ProviderId, Role, SecureCredentialManager, StreamEvent,
};

pub const ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com/v1";

#[derive(Clone)]
pub struct AnthropicProvider {
    credentials: Arc<SecureCredentialManager>,
    transport: Arc<dyn OpenAiTransport>,
    fallback_model: String,
}

impl AnthropicProvider {
    pub fn new(
        credentials: Arc<SecureCredentialManager>,
        transport: Arc<dyn OpenAiTransport>,
    ) -> Self {
        Self {
            credentials,
            transport,
            fallback_model: "claude-3-5-sonnet-latest".to_string(),
        }
    }

    pub fn with_fallback_model(mut self, model: impl Into<String>) -> Self {
        self.fallback_model = model.into();
        self
    }

    pub fn default_http_transport(client: Client) -> OpenAiHttpTransport {
        OpenAiHttpTransport::new(client).with_base_url(ANTHROPIC_BASE_URL)
    }

    fn build_request(&self, request: ModelRequest, stream: bool) -> OpenAiRequest {
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

        let tools = request
            .tools
            .into_iter()
            .map(OpenAiTool::from)
            .collect::<Vec<_>>();

        OpenAiRequest {
            model,
            messages,
            tools,
            temperature: request.temperature,
            max_tokens: request.max_tokens,
            stream,
        }
    }
}

impl ModelProvider for AnthropicProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Anthropic
    }

    fn complete<'a>(
        &'a self,
        request: ModelRequest,
    ) -> ProviderFuture<'a, Result<ModelResponse, ProviderError>> {
        Box::pin(async move {
            request.validate()?;
            let auth = OpenAiAuth::ApiKey(resolve_anthropic_api_key(&self.credentials)?);
            let anthropic_request = self.build_request(request, false);
            let response = self.transport.complete(anthropic_request, auth).await?;

            let mut mapped = response.into_model_response();
            mapped.provider = ProviderId::Anthropic;
            Ok(mapped)
        })
    }

    fn stream<'a>(
        &'a self,
        request: ModelRequest,
    ) -> ProviderFuture<'a, Result<BoxedEventStream<'a>, ProviderError>> {
        Box::pin(async move {
            request.validate()?;
            let auth = OpenAiAuth::ApiKey(resolve_anthropic_api_key(&self.credentials)?);
            let anthropic_request = self.build_request(request, true);
            let mut chunks = self.transport.stream(anthropic_request, auth).await?;

            let stream = try_stream! {
                while let Some(chunk) = chunks.next().await {
                    yield map_stream_chunk(chunk?);
                }
            };

            Ok(Box::pin(stream) as BoxedEventStream<'a>)
        })
    }
}

impl SecureCredentialManager {
    pub fn set_anthropic_api_key(&self, api_key: impl Into<String>) -> Result<(), ProviderError> {
        let api_key = api_key.into();
        if !api_key.starts_with("sk-ant-") {
            return Err(ProviderError::authentication(
                "Anthropic API key must start with 'sk-ant-'",
            ));
        }

        self.set_api_key(ProviderId::Anthropic, api_key)
    }
}

fn resolve_anthropic_api_key(
    credentials: &SecureCredentialManager,
) -> Result<String, ProviderError> {
    credentials
        .with_api_key(ProviderId::Anthropic, |value| value.to_string())?
        .ok_or_else(|| ProviderError::authentication("no Anthropic credentials configured"))
}

fn map_stream_chunk(chunk: OpenAiStreamChunk) -> StreamEvent {
    match chunk {
        OpenAiStreamChunk::TextDelta(delta) => StreamEvent::TextDelta(delta),
        OpenAiStreamChunk::ToolCallDelta(tool_call) => StreamEvent::ToolCallDelta(tool_call.into()),
        OpenAiStreamChunk::MessageComplete(message) => {
            StreamEvent::MessageComplete(Message::new(Role::Assistant, message.content))
        }
        OpenAiStreamChunk::ResponseComplete(response) => {
            let mut mapped = response.into_model_response();
            mapped.provider = ProviderId::Anthropic;
            StreamEvent::ResponseComplete(mapped)
        }
    }
}
