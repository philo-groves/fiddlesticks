//! OpenAI provider implementation over transport and shared models.

use std::sync::Arc;

use async_stream::try_stream;
use futures_util::StreamExt;

use crate::{
    BoxedEventStream, ModelProvider, ModelRequest, ModelResponse, ProviderError, ProviderFuture,
    ProviderId, SecureCredentialManager, StreamEvent,
};

use super::auth::resolve_openai_auth;
use super::transport::OpenAiTransport;
use super::types::{OpenAiMessage, OpenAiRequest, OpenAiTool};

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

    pub(crate) fn build_openai_request(&self, request: ModelRequest, stream: bool) -> OpenAiRequest {
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
            let auth = resolve_openai_auth(&self.credentials)?;
            let openai_request = self.build_openai_request(request, false);
            let response = self.transport.complete(openai_request, auth).await?;
            Ok(response.into_model_response())
        })
    }

    fn stream<'a>(
        &'a self,
        request: ModelRequest,
    ) -> ProviderFuture<'a, Result<BoxedEventStream<'a>, ProviderError>> {
        Box::pin(async move {
            request.validate()?;
            let auth = resolve_openai_auth(&self.credentials)?;
            let openai_request = self.build_openai_request(request, true);
            let mut chunks = self.transport.stream(openai_request, auth).await?;

            let stream = try_stream! {
                while let Some(chunk) = chunks.next().await {
                    yield StreamEvent::from(chunk?);
                }
            };

            Ok(Box::pin(stream) as BoxedEventStream<'a>)
        })
    }
}
