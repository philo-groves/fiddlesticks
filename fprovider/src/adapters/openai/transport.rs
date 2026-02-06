//! OpenAI transport trait and reqwest-based HTTP implementation.

use std::collections::BTreeMap;
use std::pin::Pin;

use async_stream::try_stream;
use futures_core::Stream;
use futures_util::StreamExt;
use reqwest::{Client, Response, StatusCode};

use crate::{ProviderError, ProviderFuture};

use super::serde_api::{
    OpenAiApiStreamResponse, build_api_request, extract_error_message, parse_finish_reason,
};
use super::types::{
    OpenAiAssistantMessage, OpenAiAuth, OpenAiFinishReason, OpenAiRequest, OpenAiResponse,
    OpenAiStreamChunk, OpenAiToolCall, OpenAiUsage,
};

pub type OpenAiChunkStream<'a> = Pin<Box<dyn Stream<Item = Result<OpenAiStreamChunk, ProviderError>> + Send + 'a>>;

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
    ) -> ProviderFuture<'a, Result<OpenAiChunkStream<'a>, ProviderError>>;
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
            StatusCode::REQUEST_TIMEOUT | StatusCode::GATEWAY_TIMEOUT => {
                ProviderError::timeout(message)
            }
            StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY => {
                ProviderError::invalid_request(message)
            }
            StatusCode::SERVICE_UNAVAILABLE | StatusCode::BAD_GATEWAY => {
                ProviderError::unavailable(message)
            }
            _ => ProviderError::transport(message),
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
            let api_request = build_api_request(request)?;
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

            let parsed: super::serde_api::OpenAiApiResponse = response
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
    ) -> ProviderFuture<'a, Result<OpenAiChunkStream<'a>, ProviderError>> {
        Box::pin(async move {
            request.stream = true;
            let model_for_fallback = request.model.clone();
            let api_request = build_api_request(request)?;
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

            let stream = try_stream! {
                let mut chunks = response.bytes_stream();
                let mut sse_buffer = String::new();
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
                                    yield OpenAiStreamChunk::TextDelta(delta_content.clone());
                                }
                            }

                            if let Some(delta_tool_calls) = &choice.delta.tool_calls {
                                for delta_call in delta_tool_calls {
                                    let index = delta_call.index.unwrap_or(0);
                                    let entry =
                                        tool_calls.entry(index).or_insert_with(|| OpenAiToolCall {
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

                                    yield OpenAiStreamChunk::ToolCallDelta(entry.clone());
                                }
                            }

                            if choice.finish_reason.is_some() {
                                finish_reason = parse_finish_reason(choice.finish_reason.as_deref());
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

                yield OpenAiStreamChunk::MessageComplete(final_message.clone());
                yield OpenAiStreamChunk::ResponseComplete(OpenAiResponse {
                    model: model.unwrap_or(model_for_fallback),
                    message: final_message,
                    finish_reason,
                    usage: OpenAiUsage {
                        prompt_tokens: 0,
                        completion_tokens: 0,
                        total_tokens: 0,
                    },
                });
            };

            Ok(Box::pin(stream) as OpenAiChunkStream<'a>)
        })
    }
}
