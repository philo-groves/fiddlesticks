mod credentials;
pub mod adapters;
mod error;
mod model;
pub mod prelude;
mod provider;
mod registry;
mod stream;

pub use credentials::{
    BrowserLoginSession, CredentialKind, ProviderCredential, SecretString, SecureCredentialManager,
};
pub use error::{ProviderError, ProviderErrorKind};
pub use model::{
    Message, ModelRequest, ModelRequestBuilder, ModelResponse, OutputItem, ProviderId, Role,
    StopReason, TokenUsage, ToolCall, ToolDefinition, ToolResult,
};
pub use provider::{ModelProvider, ProviderFuture};
pub use registry::ProviderRegistry;
pub use stream::{BoxedEventStream, ModelEventStream, StreamEvent, VecEventStream};

#[cfg(test)]
mod tests {
    use futures_core::Stream;
    use super::*;
    use std::future::Future;
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
    use std::time::{Duration, UNIX_EPOCH};

    #[derive(Debug)]
    struct FakeProvider;

    impl ModelProvider for FakeProvider {
        fn id(&self) -> ProviderId {
            ProviderId::OpenAi
        }

        fn complete<'a>(
            &'a self,
            request: ModelRequest,
        ) -> ProviderFuture<'a, Result<ModelResponse, ProviderError>> {
            Box::pin(async move {
                request.validate()?;
                Ok(ModelResponse {
                    provider: ProviderId::OpenAi,
                    model: request.model,
                    output: vec![OutputItem::Message(Message::new(
                        Role::Assistant,
                        "hello from provider",
                    ))],
                    stop_reason: StopReason::EndTurn,
                    usage: TokenUsage {
                        input_tokens: 5,
                        output_tokens: 4,
                        total_tokens: 9,
                    },
                })
            })
        }

        fn stream<'a>(
            &'a self,
            request: ModelRequest,
        ) -> ProviderFuture<'a, Result<BoxedEventStream<'a>, ProviderError>> {
            Box::pin(async move {
                request.validate()?;
                let stream = VecEventStream::new(vec![
                    Ok(StreamEvent::TextDelta("hello".to_string())),
                    Ok(StreamEvent::TextDelta(" world".to_string())),
                ]);
                let boxed: BoxedEventStream<'a> = Box::pin(stream);
                Ok::<BoxedEventStream<'a>, ProviderError>(boxed)
            })
        }
    }

    #[test]
    fn provider_id_display_is_stable() {
        assert_eq!(ProviderId::OpenCodeZen.to_string(), "opencode-zen");
        assert_eq!(ProviderId::OpenAi.to_string(), "openai");
        assert_eq!(ProviderId::Claude.to_string(), "claude");
    }

    #[test]
    fn model_request_validate_enforces_contract() {
        let empty_model = ModelRequest::new("   ", vec![Message::new(Role::User, "hi")]);
        let err = empty_model.validate().expect_err("empty model must fail");
        assert_eq!(err.kind, ProviderErrorKind::InvalidRequest);

        let empty_messages = ModelRequest::new("gpt", Vec::new());
        let err = empty_messages
            .validate()
            .expect_err("empty messages must fail");
        assert_eq!(err.kind, ProviderErrorKind::InvalidRequest);

        let bad_temperature = ModelRequest::new("gpt", vec![Message::new(Role::User, "hi")])
            .with_temperature(2.5);
        let err = bad_temperature
            .validate()
            .expect_err("temperature outside range must fail");
        assert_eq!(err.kind, ProviderErrorKind::InvalidRequest);

        let bad_max_tokens =
            ModelRequest::new("gpt", vec![Message::new(Role::User, "hi")]).with_max_tokens(0);
        let err = bad_max_tokens
            .validate()
            .expect_err("max_tokens=0 must fail");
        assert_eq!(err.kind, ProviderErrorKind::InvalidRequest);

        let valid = ModelRequest::new("gpt", vec![Message::new(Role::User, "hi")])
            .with_temperature(0.4)
            .with_max_tokens(128)
            .with_metadata("trace_id", "abc")
            .enable_streaming();
        assert!(valid.validate().is_ok());
        assert!(valid.stream);
        assert_eq!(valid.metadata.get("trace_id"), Some(&"abc".to_string()));
    }

    #[test]
    fn model_request_builder_validates_before_building() {
        let err = ModelRequest::builder("gpt-4o-mini")
            .temperature(3.0)
            .build()
            .expect_err("builder should fail without messages and with bad temperature");
        assert_eq!(err.kind, ProviderErrorKind::InvalidRequest);

        let request = ModelRequest::builder("gpt-4o-mini")
            .message(Message::new(Role::User, "hello"))
            .temperature(0.2)
            .max_tokens(200)
            .metadata("trace_id", "abc")
            .enable_streaming()
            .build()
            .expect("builder should produce valid request");

        assert_eq!(request.messages.len(), 1);
        assert_eq!(request.metadata.get("trace_id"), Some(&"abc".to_string()));
        assert!(request.stream);
    }

    #[test]
    fn provider_error_helper_builders_assign_expected_retryability() {
        let auth = ProviderError::authentication("bad key");
        assert!(!auth.retryable);
        assert_eq!(auth.kind, ProviderErrorKind::Authentication);

        let timeout = ProviderError::timeout("request timed out");
        assert!(timeout.retryable);
        assert_eq!(timeout.kind, ProviderErrorKind::Timeout);

        let rate_limited = ProviderError::rate_limited("try later");
        assert!(rate_limited.retryable);
        assert_eq!(rate_limited.kind, ProviderErrorKind::RateLimited);
    }

    #[test]
    fn vec_event_stream_yields_events_in_order() {
        let mut stream = Box::pin(VecEventStream::new(vec![
            Ok(StreamEvent::TextDelta("one".into())),
            Ok(StreamEvent::TextDelta("two".into())),
        ]));
        let waker = noop_waker();
        let mut cx = Context::from_waker(&waker);

        let first = stream.as_mut().poll_next(&mut cx);
        assert_eq!(
            first,
            Poll::Ready(Some(Ok(StreamEvent::TextDelta("one".into()))))
        );

        let second = stream.as_mut().poll_next(&mut cx);
        assert_eq!(
            second,
            Poll::Ready(Some(Ok(StreamEvent::TextDelta("two".into()))))
        );

        let end = stream.as_mut().poll_next(&mut cx);
        assert_eq!(end, Poll::Ready(None));
    }

    #[test]
    fn provider_registry_registers_and_returns_providers() {
        let mut registry = ProviderRegistry::new();
        assert!(registry.is_empty());

        registry.register(FakeProvider);
        assert_eq!(registry.len(), 1);
        assert!(registry.contains(ProviderId::OpenAi));

        let provider = registry
            .get(ProviderId::OpenAi)
            .expect("provider should exist");

        let request = ModelRequest::new("gpt-4o-mini", vec![Message::new(Role::User, "hi")]);
        let response = block_on(provider.complete(request)).expect("completion should work");

        assert_eq!(response.provider, ProviderId::OpenAi);
        assert_eq!(response.stop_reason, StopReason::EndTurn);

        let removed = registry.remove(ProviderId::OpenAi);
        assert!(removed.is_some());
        assert!(registry.is_empty());
    }

    #[test]
    fn model_provider_stream_returns_expected_events() {
        let provider = FakeProvider;
        let request = ModelRequest::new("gpt", vec![Message::new(Role::User, "stream please")]);
        let mut stream = block_on(provider.stream(request)).expect("stream should work");
        let waker = noop_waker();
        let mut cx = Context::from_waker(&waker);

        let first = stream.as_mut().poll_next(&mut cx);
        assert_eq!(
            first,
            Poll::Ready(Some(Ok(StreamEvent::TextDelta("hello".to_string()))))
        );

        let second = stream.as_mut().poll_next(&mut cx);
        assert_eq!(
            second,
            Poll::Ready(Some(Ok(StreamEvent::TextDelta(" world".to_string()))))
        );

        let done = stream.as_mut().poll_next(&mut cx);
        assert_eq!(done, Poll::Ready(None));
    }

    #[test]
    fn secret_string_debug_is_redacted() {
        let secret = SecretString::new("super-secret-value");
        assert_eq!(format!("{secret:?}"), "[REDACTED]");
        assert_eq!(secret.expose(), "super-secret-value");
    }

    #[test]
    fn secure_credential_manager_handles_provider_agnostic_credentials() {
        let manager = SecureCredentialManager::new();

        manager
            .set_api_key(ProviderId::Claude, "claude-key")
            .expect("api key set should work");
        assert!(manager
            .has_credentials(ProviderId::Claude)
            .expect("has_credentials should work"));

        let kind = manager
            .credential_kind(ProviderId::Claude)
            .expect("kind lookup should work");
        assert_eq!(kind, Some(CredentialKind::ApiKey));

        let captured = manager
            .with_api_key(ProviderId::Claude, |value| value.to_string())
            .expect("api key read should work");
        assert_eq!(captured, Some("claude-key".to_string()));

        let cleared = manager
            .clear(ProviderId::Claude)
            .expect("clear should work");
        assert!(cleared);
        assert!(!manager
            .has_credentials(ProviderId::Claude)
            .expect("has_credentials should work"));
    }

    #[test]
    fn secure_credential_manager_handles_browser_sessions() {
        let manager = SecureCredentialManager::new();
        let expires_at = UNIX_EPOCH + Duration::from_secs(1234);

        manager
            .set_browser_session(ProviderId::OpenCodeZen, "session-token", Some(expires_at))
            .expect("session set should work");

        let kind = manager
            .credential_kind(ProviderId::OpenCodeZen)
            .expect("kind lookup should work");
        assert_eq!(kind, Some(CredentialKind::BrowserSession));

        let captured = manager
            .with_browser_session(ProviderId::OpenCodeZen, |session| {
                (session.session_token.expose().to_string(), session.expires_at)
            })
            .expect("session read should work");
        assert_eq!(captured, Some(("session-token".to_string(), Some(expires_at))));
    }

    #[cfg(feature = "provider-openai")]
    #[test]
    fn openai_helpers_validate_and_store_credentials() {
        let manager = SecureCredentialManager::new();

        let err = manager
            .set_openai_api_key("not-valid")
            .expect_err("invalid key should fail");
        assert_eq!(err.kind, ProviderErrorKind::Authentication);

        manager
            .set_openai_api_key("sk-test-123")
            .expect("valid key should store");

        let key = manager
            .with_api_key(ProviderId::OpenAi, |value| value.to_string())
            .expect("openai key read should work");
        assert_eq!(key, Some("sk-test-123".to_string()));

        manager
            .set_openai_browser_session("browser-session", None)
            .expect("openai browser session should store");

        let kind = manager
            .credential_kind(ProviderId::OpenAi)
            .expect("openai kind should be available");
        assert_eq!(kind, Some(CredentialKind::BrowserSession));

        let no_api_key = manager
            .with_api_key(ProviderId::OpenAi, |value| value.to_string())
            .expect("api key lookup should work");
        assert_eq!(no_api_key, None);

        let session_token = manager
            .with_browser_session(ProviderId::OpenAi, |session| {
                session.session_token.expose().to_string()
            })
            .expect("session lookup should work");
        assert_eq!(session_token, Some("browser-session".to_string()));
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
}
