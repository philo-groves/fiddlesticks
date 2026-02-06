#![cfg(feature = "provider-openai")]

use std::future::Future;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

use fprovider::adapters::openai::{
    OpenAiAuth, OpenAiProvider, OpenAiRequest, OpenAiResponse, OpenAiStreamChunk,
    OpenAiTransport,
};
use fprovider::{
    Message, ModelProvider, ModelRequest, ProviderError, ProviderFuture, ProviderId, Role,
    SecureCredentialManager,
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
    ) -> ProviderFuture<'a, Result<Vec<OpenAiStreamChunk>, ProviderError>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}

#[test]
fn openai_provider_uses_openai_credentials_and_maps_completion() {
    let credentials = Arc::new(SecureCredentialManager::new());
    credentials
        .set_openai_api_key("sk-integration-123")
        .expect("key should set");

    let transport = Arc::new(IntegrationFakeTransport::default());
    let provider = OpenAiProvider::new(credentials, transport.clone());

    let request = ModelRequest::new("gpt-4o-mini", vec![Message::new(Role::User, "hello")]);
    let response = block_on(provider.complete(request)).expect("complete should succeed");

    assert_eq!(response.provider, ProviderId::OpenAi);
    assert_eq!(response.model, "gpt-4o-mini");
    assert_eq!(response.usage.total_tokens, 2);

    let seen_auth = transport
        .seen_auth
        .lock()
        .expect("auth lock")
        .clone()
        .expect("auth should be captured");

    assert_eq!(seen_auth, OpenAiAuth::ApiKey("sk-integration-123".to_string()));
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
