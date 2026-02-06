#![cfg(feature = "provider-openai")]

use std::sync::{Arc, Mutex};

use fprovider::adapters::openai::{
    OpenAiAuth, OpenAiProvider, OpenAiRequest, OpenAiResponse, OpenAiStreamChunk,
    OpenAiTransport,
};
use fprovider::{
    Message, ModelProvider, ModelRequest, ProviderError, ProviderFuture, ProviderId, Role,
    StopReason, ToolDefinition, ToolResult, SecureCredentialManager,
};

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
                message: fprovider::adapters::openai::OpenAiAssistantMessage {
                    content: "hello world".to_string(),
                    tool_calls: vec![fprovider::adapters::openai::OpenAiToolCall {
                        id: "call_1".to_string(),
                        name: "lookup".to_string(),
                        arguments: "{\"id\":1}".to_string(),
                    }],
                },
                finish_reason: fprovider::adapters::openai::OpenAiFinishReason::ToolCalls,
                usage: fprovider::adapters::openai::OpenAiUsage {
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

#[tokio::test]
async fn complete_maps_openai_response_to_provider_response() {
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

    let response = provider.complete(request).await.expect("completion should succeed");
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

#[tokio::test]
async fn stream_prefers_browser_session_when_api_key_missing() {
    let credentials = Arc::new(SecureCredentialManager::new());
    credentials
        .set_openai_browser_session("session-xyz", None)
        .expect("session should set");

    let transport = Arc::new(FakeTransport::default());
    let provider = OpenAiProvider::new(credentials, transport.clone());
    let request = ModelRequest::new("gpt-4o-mini", vec![Message::new(Role::User, "hi")]);

    let _stream = provider.stream(request).await.expect("stream should succeed");

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

#[tokio::test]
async fn missing_openai_credentials_returns_auth_error() {
    let credentials = Arc::new(SecureCredentialManager::new());
    let transport = Arc::new(FakeTransport::default());
    let provider = OpenAiProvider::new(credentials, transport);
    let request = ModelRequest::new("gpt-4o-mini", vec![Message::new(Role::User, "hi")]);

    let error = provider.complete(request).await.expect_err("missing creds should fail");
    assert_eq!(error.kind, fprovider::ProviderErrorKind::Authentication);
    assert_eq!(error.message, "no OpenAI credentials configured");
}
