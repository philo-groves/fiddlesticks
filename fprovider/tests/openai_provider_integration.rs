#![cfg(feature = "provider-openai")]

use std::sync::{Arc, Mutex};

use fprovider::adapters::openai::{
    OpenAiAuth, OpenAiChunkStream, OpenAiProvider, OpenAiRequest, OpenAiResponse, OpenAiTransport,
};
use fprovider::{
    Message, ModelProvider, ModelRequest, ProviderError, ProviderFuture, ProviderId, Role,
    SecretString, SecureCredentialManager,
};

#[derive(Debug, Default)]
struct IntegrationFakeTransport {
    seen_auth: Mutex<Option<OpenAiAuth>>,
}

impl OpenAiTransport for IntegrationFakeTransport {
    fn complete<'a>(
        &'a self,
        _request: OpenAiRequest,
        auth: OpenAiAuth,
    ) -> ProviderFuture<'a, Result<OpenAiResponse, ProviderError>> {
        Box::pin(async move {
            *self.seen_auth.lock().expect("auth lock") = Some(auth);
            Ok(OpenAiResponse {
                model: "gpt-4o-mini".to_string(),
                message: fprovider::adapters::openai::OpenAiAssistantMessage {
                    content: "integration-ok".to_string(),
                    tool_calls: Vec::new(),
                },
                finish_reason: fprovider::adapters::openai::OpenAiFinishReason::Stop,
                usage: fprovider::adapters::openai::OpenAiUsage {
                    prompt_tokens: 1,
                    completion_tokens: 1,
                    total_tokens: 2,
                },
            })
        })
    }

    fn stream<'a>(
        &'a self,
        _request: OpenAiRequest,
        _auth: OpenAiAuth,
    ) -> ProviderFuture<'a, Result<OpenAiChunkStream<'a>, ProviderError>> {
        Box::pin(async {
            let output = futures_util::stream::iter(std::iter::empty::<Result<_, _>>());
            Ok(Box::pin(output) as OpenAiChunkStream<'a>)
        })
    }
}

#[tokio::test]
async fn openai_provider_uses_openai_credentials_and_maps_completion() {
    let credentials = Arc::new(SecureCredentialManager::new());
    credentials
        .set_openai_api_key("sk-integration-123")
        .expect("key should set");

    let transport = Arc::new(IntegrationFakeTransport::default());
    let provider = OpenAiProvider::new(credentials, transport.clone());

    let request = ModelRequest::new("gpt-4o-mini", vec![Message::new(Role::User, "hello")]);
    let response = provider
        .complete(request)
        .await
        .expect("complete should succeed");

    assert_eq!(response.provider, ProviderId::OpenAi);
    assert_eq!(response.model, "gpt-4o-mini");
    assert_eq!(response.usage.total_tokens, 2);

    let seen_auth = transport
        .seen_auth
        .lock()
        .expect("auth lock")
        .clone()
        .expect("auth should be captured");

    assert_eq!(
        seen_auth,
        OpenAiAuth::ApiKey(SecretString::new("sk-integration-123"))
    );
}
