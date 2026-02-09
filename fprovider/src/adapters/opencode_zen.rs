//! OpenCode Zen provider implemented via OpenAI-compatible transport.

use std::sync::Arc;

use async_stream::try_stream;
use futures_util::StreamExt;
use reqwest::{Client, StatusCode};
use serde::Deserialize;

use crate::adapters::openai::{
    OpenAiAuth, OpenAiHttpTransport, OpenAiMessage, OpenAiRequest, OpenAiStreamChunk, OpenAiTool,
    OpenAiTransport,
};
use crate::{
    BoxedEventStream, Message, ModelProvider, ModelRequest, ModelResponse, ProviderError,
    ProviderFuture, ProviderId, Role, SecureCredentialManager, StreamEvent,
};

pub const OPENCODE_ZEN_BASE_URL: &str = "https://opencode.ai/zen/v1";
pub const OPENCODE_ZEN_MODELS_URL: &str = "https://opencode.ai/zen/v1/models";

#[derive(Clone)]
pub struct OpenCodeZenProvider {
    credentials: Arc<SecureCredentialManager>,
    transport: Arc<dyn OpenAiTransport>,
    fallback_model: String,
}

impl OpenCodeZenProvider {
    pub fn new(
        credentials: Arc<SecureCredentialManager>,
        transport: Arc<dyn OpenAiTransport>,
    ) -> Self {
        Self {
            credentials,
            transport,
            fallback_model: "kimi-k2.5".to_string(),
        }
    }

    pub fn with_fallback_model(mut self, model: impl Into<String>) -> Self {
        self.fallback_model = model.into();
        self
    }

    pub fn default_http_transport(client: Client) -> OpenAiHttpTransport {
        OpenAiHttpTransport::new(client).with_base_url(OPENCODE_ZEN_BASE_URL)
    }

    pub async fn list_models(&self) -> Result<Vec<String>, ProviderError> {
        let key = resolve_zen_api_key(&self.credentials)?;
        list_zen_models_with_api_key(key).await
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
            temperature: request.options.temperature,
            max_tokens: request.options.max_tokens,
            stream,
        }
    }
}

impl ModelProvider for OpenCodeZenProvider {
    fn id(&self) -> ProviderId {
        ProviderId::OpenCodeZen
    }

    fn complete<'a>(
        &'a self,
        request: ModelRequest,
    ) -> ProviderFuture<'a, Result<ModelResponse, ProviderError>> {
        Box::pin(async move {
            request.validate()?;
            let auth = OpenAiAuth::ApiKey(resolve_zen_api_key(&self.credentials)?);
            let zen_request = self.build_request(request, false);
            let response = self.transport.complete(zen_request, auth).await?;

            let mut mapped = response.into_model_response();
            mapped.provider = ProviderId::OpenCodeZen;
            Ok(mapped)
        })
    }

    fn stream<'a>(
        &'a self,
        request: ModelRequest,
    ) -> ProviderFuture<'a, Result<BoxedEventStream<'a>, ProviderError>> {
        Box::pin(async move {
            request.validate()?;
            let auth = OpenAiAuth::ApiKey(resolve_zen_api_key(&self.credentials)?);
            let zen_request = self.build_request(request, true);
            let mut chunks = self.transport.stream(zen_request, auth).await?;

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
    pub fn set_opencode_zen_api_key(
        &self,
        api_key: impl Into<String>,
    ) -> Result<(), ProviderError> {
        self.set_api_key(ProviderId::OpenCodeZen, api_key)
    }
}

pub async fn list_zen_models_with_api_key(
    api_key: impl Into<String>,
) -> Result<Vec<String>, ProviderError> {
    let key = api_key.into();
    let trimmed = key.trim();
    if trimmed.is_empty() {
        return Err(ProviderError::authentication(
            "OpenCode Zen API key must not be empty",
        ));
    }

    let response = Client::new()
        .get(OPENCODE_ZEN_MODELS_URL)
        .bearer_auth(trimmed)
        .send()
        .await
        .map_err(|err| {
            if err.is_timeout() {
                ProviderError::timeout(err.to_string())
            } else {
                ProviderError::transport(err.to_string())
            }
        })?;

    if response.status() == StatusCode::UNAUTHORIZED || response.status() == StatusCode::FORBIDDEN {
        return Err(ProviderError::authentication(
            "OpenCode Zen API key is invalid or expired",
        ));
    }

    if !response.status().is_success() {
        let code = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(ProviderError::transport(format!(
            "http {code}: {}",
            truncate(&body, 4096)
        )));
    }

    let parsed = response
        .json::<ZenModelsResponse>()
        .await
        .map_err(|err| ProviderError::transport(err.to_string()))?;

    let mut ids = parsed.data.into_iter().map(|m| m.id).collect::<Vec<_>>();
    ids.sort();
    Ok(ids)
}

fn resolve_zen_api_key(credentials: &SecureCredentialManager) -> Result<String, ProviderError> {
    credentials
        .with_api_key(ProviderId::OpenCodeZen, |value| value.to_string())?
        .ok_or_else(|| ProviderError::authentication("no OpenCode Zen credentials configured"))
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
            mapped.provider = ProviderId::OpenCodeZen;
            StreamEvent::ResponseComplete(mapped)
        }
    }
}

fn truncate(input: &str, max: usize) -> String {
    if input.len() <= max {
        return input.to_string();
    }
    let mut output = input[..max].to_string();
    output.push_str("...");
    output
}

#[derive(Debug, Deserialize)]
struct ZenModelsResponse {
    data: Vec<ZenModel>,
}

#[derive(Debug, Deserialize)]
struct ZenModel {
    id: String,
}
