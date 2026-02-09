#![cfg(feature = "provider-opencode-zen")]

use std::sync::{Arc, Mutex};

use fprovider::adapters::openai::{
    OpenAiAssistantMessage, OpenAiAuth, OpenAiChunkStream, OpenAiFinishReason, OpenAiRequest,
    OpenAiResponse, OpenAiStreamChunk, OpenAiToolCall, OpenAiTransport, OpenAiUsage,
};
use fprovider::adapters::opencode_zen::OpenCodeZenProvider;
use fprovider::{
    Message, ModelProvider, ModelRequest, ProviderError, ProviderFuture, ProviderId, Role,
    SecureCredentialManager, StopReason, StreamEvent,
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
                model: "kimi-k2.5".to_string(),
                message: OpenAiAssistantMessage {
                    content: "zen-ok".to_string(),
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
                        model: "kimi-k2.5".to_string(),
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
async fn complete_uses_zen_credentials_and_maps_provider_id() {
    let credentials = Arc::new(SecureCredentialManager::new());
    credentials
        .set_opencode_zen_api_key("zen-key-123")
        .expect("key should set");

    let transport = Arc::new(FakeTransport::default());
    let provider = OpenCodeZenProvider::new(credentials, transport.clone());
    let request = ModelRequest::new("kimi-k2.5", vec![Message::new(Role::User, "hi")]);

    let response = provider
        .complete(request)
        .await
        .expect("complete should succeed");
    assert_eq!(response.provider, ProviderId::OpenCodeZen);
    assert_eq!(response.model, "kimi-k2.5");
    assert_eq!(response.stop_reason, StopReason::ToolUse);

    let auth = transport
        .captured_auth
        .lock()
        .expect("auth lock")
        .clone()
        .expect("auth should be captured");
    assert_eq!(auth, CapturedAuth("zen-key-123".to_string()));
}

#[tokio::test]
async fn stream_maps_response_complete_to_zen_provider_id() {
    let credentials = Arc::new(SecureCredentialManager::new());
    credentials
        .set_opencode_zen_api_key("zen-key-xyz")
        .expect("key should set");

    let transport = Arc::new(FakeTransport::default());
    let provider = OpenCodeZenProvider::new(credentials, transport);
    let request = ModelRequest::new("kimi-k2.5", vec![Message::new(Role::User, "stream")]);

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

    assert_eq!(saw_provider, Some(ProviderId::OpenCodeZen));
}

#[tokio::test]
async fn missing_zen_credentials_returns_authentication_error() {
    let credentials = Arc::new(SecureCredentialManager::new());
    let transport = Arc::new(FakeTransport::default());
    let provider = OpenCodeZenProvider::new(credentials, transport);
    let request = ModelRequest::new("kimi-k2.5", vec![Message::new(Role::User, "hi")]);

    let error = provider
        .complete(request)
        .await
        .expect_err("missing zen credentials should fail");
    assert_eq!(error.kind, fprovider::ProviderErrorKind::Authentication);
    assert_eq!(error.message, "no OpenCode Zen credentials configured");
}
