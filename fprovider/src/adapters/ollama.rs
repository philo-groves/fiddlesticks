//! Ollama provider implemented over OpenAI-compatible transport.

use std::sync::Arc;

use async_stream::try_stream;
use futures_util::StreamExt;
use reqwest::Client;
use serde::Deserialize;

use crate::adapters::openai::{
    OpenAiAuth, OpenAiHttpTransport, OpenAiMessage, OpenAiRequest, OpenAiStreamChunk, OpenAiTool,
    OpenAiTransport,
};
use crate::{
    BoxedEventStream, Message, ModelProvider, ModelRequest, ModelResponse, ProviderError,
    ProviderFuture, ProviderId, Role, SecretString, StreamEvent,
};

pub const OLLAMA_BASE_URL: &str = "http://localhost:11434/v1";
pub const OLLAMA_HOST_URL: &str = "http://localhost:11434";

#[derive(Clone)]
pub struct OllamaProvider {
    transport: Arc<dyn OpenAiTransport>,
    fallback_model: String,
}

impl OllamaProvider {
    pub fn new(transport: Arc<dyn OpenAiTransport>) -> Self {
        Self {
            transport,
            fallback_model: "llama3.2".to_string(),
        }
    }

    pub fn with_fallback_model(mut self, model: impl Into<String>) -> Self {
        self.fallback_model = model.into();
        self
    }

    pub fn default_http_transport(client: Client) -> OpenAiHttpTransport {
        OpenAiHttpTransport::new(client).with_base_url(OLLAMA_BASE_URL)
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

    fn auth_placeholder() -> OpenAiAuth {
        OpenAiAuth::ApiKey(SecretString::new("ollama-local"))
    }
}

impl ModelProvider for OllamaProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Ollama
    }

    fn complete<'a>(
        &'a self,
        request: ModelRequest,
    ) -> ProviderFuture<'a, Result<ModelResponse, ProviderError>> {
        Box::pin(async move {
            request.validate()?;
            let ollama_request = self.build_request(request, false);
            let response = self
                .transport
                .complete(ollama_request, Self::auth_placeholder())
                .await?;

            let mut mapped = response.into_model_response();
            mapped.provider = ProviderId::Ollama;
            Ok(mapped)
        })
    }

    fn stream<'a>(
        &'a self,
        request: ModelRequest,
    ) -> ProviderFuture<'a, Result<BoxedEventStream<'a>, ProviderError>> {
        Box::pin(async move {
            request.validate()?;
            let ollama_request = self.build_request(request, true);
            let mut chunks = self
                .transport
                .stream(ollama_request, Self::auth_placeholder())
                .await?;

            let stream = try_stream! {
                while let Some(chunk) = chunks.next().await {
                    yield map_stream_chunk(chunk?);
                }
            };

            Ok(Box::pin(stream) as BoxedEventStream<'a>)
        })
    }
}

pub async fn list_ollama_models() -> Result<Vec<String>, ProviderError> {
    list_ollama_models_with_base_url(OLLAMA_HOST_URL).await
}

pub async fn list_ollama_models_with_base_url(
    base_url: impl Into<String>,
) -> Result<Vec<String>, ProviderError> {
    let base_url = base_url.into();
    let endpoint = format!("{}/api/tags", base_url.trim_end_matches('/'));

    let response = Client::new().get(endpoint).send().await.map_err(|err| {
        if err.is_timeout() {
            ProviderError::timeout(err.to_string())
        } else {
            ProviderError::transport(err.to_string())
        }
    })?;

    if !response.status().is_success() {
        let code = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(ProviderError::transport(format!(
            "http {code}: {}",
            truncate(&body, 4096)
        )));
    }

    let parsed = response
        .json::<OllamaTagsResponse>()
        .await
        .map_err(|err| ProviderError::transport(err.to_string()))?;

    let mut ids = parsed
        .models
        .into_iter()
        .map(|m| m.name)
        .collect::<Vec<_>>();
    ids.sort();
    Ok(ids)
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
            mapped.provider = ProviderId::Ollama;
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
struct OllamaTagsResponse {
    #[serde(default)]
    models: Vec<OllamaModelTag>,
}

#[derive(Debug, Deserialize)]
struct OllamaModelTag {
    name: String,
}
