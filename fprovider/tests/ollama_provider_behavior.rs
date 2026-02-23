#![cfg(feature = "provider-ollama")]

use std::sync::{Arc, Mutex};

use fprovider::adapters::ollama::OllamaProvider;
use fprovider::adapters::openai::{
    OpenAiAssistantMessage, OpenAiAuth, OpenAiChunkStream, OpenAiFinishReason, OpenAiRequest,
    OpenAiResponse, OpenAiStreamChunk, OpenAiToolCall, OpenAiTransport, OpenAiUsage,
};
use fprovider::{
    Message, ModelProvider, ModelRequest, ProviderError, ProviderFuture, ProviderId, Role,
    StopReason, StreamEvent,
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct CapturedAuth(String);

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
                OpenAiAuth::ApiKey(value) => CapturedAuth(value.expose().to_string()),
                OpenAiAuth::BrowserSession(value) => CapturedAuth(value.expose().to_string()),
            });

            Ok(OpenAiResponse {
                model: "llama3.2".to_string(),
                message: OpenAiAssistantMessage {
                    content: "ollama-ok".to_string(),
                    tool_calls: vec![OpenAiToolCall {
                        id: "call_1".to_string(),
                        name: "lookup".to_string(),
                        arguments: "{}".to_string(),
                    }],
                },
                finish_reason: OpenAiFinishReason::ToolCalls,
                usage: OpenAiUsage {
                    prompt_tokens: 2,
                    completion_tokens: 3,
                    total_tokens: 5,
                },
            })
        })
    }

    fn stream<'a>(
        &'a self,
        request: OpenAiRequest,
        auth: OpenAiAuth,
    ) -> ProviderFuture<'a, Result<OpenAiChunkStream<'a>, ProviderError>> {
        Box::pin(async move {
            *self.captured_request.lock().expect("request lock") = Some(request);
            *self.captured_auth.lock().expect("auth lock") = Some(match auth {
                OpenAiAuth::ApiKey(value) => CapturedAuth(value.expose().to_string()),
                OpenAiAuth::BrowserSession(value) => CapturedAuth(value.expose().to_string()),
            });

            let output = futures_util::stream::iter(
                vec![
                    OpenAiStreamChunk::TextDelta("hello".to_string()),
                    OpenAiStreamChunk::ResponseComplete(OpenAiResponse {
                        model: "llama3.2".to_string(),
                        message: OpenAiAssistantMessage {
                            content: "hello".to_string(),
                            tool_calls: Vec::new(),
                        },
                        finish_reason: OpenAiFinishReason::Stop,
                        usage: OpenAiUsage {
                            prompt_tokens: 1,
                            completion_tokens: 1,
                            total_tokens: 2,
                        },
                    }),
                ]
                .into_iter()
                .map(Ok),
            );

            Ok(Box::pin(output) as OpenAiChunkStream<'a>)
        })
    }
}

#[tokio::test]
async fn complete_maps_to_ollama_provider_id_and_uses_placeholder_auth() {
    let transport = Arc::new(FakeTransport::default());
    let provider = OllamaProvider::new(transport.clone());
    let request = ModelRequest::new("llama3.2", vec![Message::new(Role::User, "hi")]);

    let response = provider
        .complete(request)
        .await
        .expect("complete should succeed");
    assert_eq!(response.provider, ProviderId::Ollama);
    assert_eq!(response.model, "llama3.2");
    assert_eq!(response.stop_reason, StopReason::ToolUse);

    let auth = transport
        .captured_auth
        .lock()
        .expect("auth lock")
        .clone()
        .expect("auth should be captured");
    assert_eq!(auth, CapturedAuth("ollama-local".to_string()));
}

#[tokio::test]
async fn stream_maps_response_complete_to_ollama_provider_id() {
    let transport = Arc::new(FakeTransport::default());
    let provider = OllamaProvider::new(transport);
    let request = ModelRequest::new("llama3.2", vec![Message::new(Role::User, "stream")]);

    let mut stream = provider
        .stream(request)
        .await
        .expect("stream should succeed");
    let mut saw_provider = None;
    while let Some(item) = futures_util::StreamExt::next(&mut stream).await {
        let event = item.expect("stream event should be ok");
        if let StreamEvent::ResponseComplete(response) = event {
            saw_provider = Some(response.provider);
        }
    }

    assert_eq!(saw_provider, Some(ProviderId::Ollama));
}
