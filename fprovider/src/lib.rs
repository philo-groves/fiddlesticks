use std::collections::{HashMap, VecDeque};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

pub type ProviderFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProviderId {
    OpenCodeZen,
    OpenAi,
    Claude,
}

impl Display for ProviderId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let id = match self {
            Self::OpenCodeZen => "opencode-zen",
            Self::OpenAi => "openai",
            Self::Claude => "claude",
        };

        f.write_str(id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

impl Message {
    pub fn new(role: Role, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolResult {
    pub tool_call_id: String,
    pub output: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputItem {
    Message(Message),
    ToolCall(ToolCall),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    EndTurn,
    MaxTokens,
    ToolUse,
    Cancelled,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TokenUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelResponse {
    pub provider: ProviderId,
    pub model: String,
    pub output: Vec<OutputItem>,
    pub stop_reason: StopReason,
    pub usage: TokenUsage,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub tools: Vec<ToolDefinition>,
    pub tool_results: Vec<ToolResult>,
    pub metadata: HashMap<String, String>,
    pub stream: bool,
}

impl ModelRequest {
    pub fn new(model: impl Into<String>, messages: Vec<Message>) -> Self {
        Self {
            model: model.into(),
            messages,
            temperature: None,
            max_tokens: None,
            tools: Vec::new(),
            tool_results: Vec::new(),
            metadata: HashMap::new(),
            stream: false,
        }
    }

    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    pub fn with_tools(mut self, tools: Vec<ToolDefinition>) -> Self {
        self.tools = tools;
        self
    }

    pub fn with_tool_results(mut self, tool_results: Vec<ToolResult>) -> Self {
        self.tool_results = tool_results;
        self
    }

    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    pub fn enable_streaming(mut self) -> Self {
        self.stream = true;
        self
    }

    pub fn validate(&self) -> Result<(), ProviderError> {
        if self.model.trim().is_empty() {
            return Err(ProviderError::invalid_request("model must not be empty"));
        }

        if self.messages.is_empty() {
            return Err(ProviderError::invalid_request(
                "at least one message is required",
            ));
        }

        if let Some(max_tokens) = self.max_tokens {
            if max_tokens == 0 {
                return Err(ProviderError::invalid_request(
                    "max_tokens must be greater than zero",
                ));
            }
        }

        if let Some(temperature) = self.temperature {
            if !(0.0..=2.0).contains(&temperature) {
                return Err(ProviderError::invalid_request(
                    "temperature must be in the inclusive range 0.0..=2.0",
                ));
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEvent {
    TextDelta(String),
    ToolCallDelta(ToolCall),
    MessageComplete(Message),
    ResponseComplete(ModelResponse),
}

pub trait ModelEventStream {
    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<StreamEvent, ProviderError>>>;
}

pub type BoxedEventStream<'a> = Pin<Box<dyn ModelEventStream + Send + 'a>>;

#[derive(Debug)]
pub struct VecEventStream {
    events: VecDeque<Result<StreamEvent, ProviderError>>,
}

impl VecEventStream {
    pub fn new(events: Vec<Result<StreamEvent, ProviderError>>) -> Self {
        Self {
            events: events.into(),
        }
    }
}

impl ModelEventStream for VecEventStream {
    fn poll_next(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<StreamEvent, ProviderError>>> {
        Poll::Ready(self.events.pop_front())
    }
}

pub trait ModelProvider: Send + Sync {
    fn id(&self) -> ProviderId;

    fn complete<'a>(&'a self, request: ModelRequest)
        -> ProviderFuture<'a, Result<ModelResponse, ProviderError>>;

    fn stream<'a>(
        &'a self,
        request: ModelRequest,
    ) -> ProviderFuture<'a, Result<BoxedEventStream<'a>, ProviderError>>;
}

#[derive(Default)]
pub struct ProviderRegistry {
    providers: HashMap<ProviderId, Arc<dyn ModelProvider>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<P>(&mut self, provider: P)
    where
        P: ModelProvider + 'static,
    {
        self.providers.insert(provider.id(), Arc::new(provider));
    }

    pub fn get(&self, provider_id: ProviderId) -> Option<Arc<dyn ModelProvider>> {
        self.providers.get(&provider_id).map(Arc::clone)
    }

    pub fn remove(&mut self, provider_id: ProviderId) -> Option<Arc<dyn ModelProvider>> {
        self.providers.remove(&provider_id)
    }

    pub fn contains(&self, provider_id: ProviderId) -> bool {
        self.providers.contains_key(&provider_id)
    }

    pub fn len(&self) -> usize {
        self.providers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderErrorKind {
    Authentication,
    RateLimited,
    InvalidRequest,
    Timeout,
    Transport,
    Unavailable,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderError {
    pub kind: ProviderErrorKind,
    pub message: String,
    pub retryable: bool,
}

impl ProviderError {
    pub fn new(kind: ProviderErrorKind, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            kind,
            message: message.into(),
            retryable,
        }
    }

    pub fn authentication(message: impl Into<String>) -> Self {
        Self::new(ProviderErrorKind::Authentication, message, false)
    }

    pub fn rate_limited(message: impl Into<String>) -> Self {
        Self::new(ProviderErrorKind::RateLimited, message, true)
    }

    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(ProviderErrorKind::InvalidRequest, message, false)
    }

    pub fn timeout(message: impl Into<String>) -> Self {
        Self::new(ProviderErrorKind::Timeout, message, true)
    }

    pub fn transport(message: impl Into<String>) -> Self {
        Self::new(ProviderErrorKind::Transport, message, true)
    }

    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::new(ProviderErrorKind::Unavailable, message, true)
    }
}

impl Display for ProviderError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)
    }
}

impl Error for ProviderError {}

#[cfg(feature = "provider-opencode-zen")]
pub mod opencode_zen;

#[cfg(feature = "provider-openai")]
pub mod openai;

#[cfg(feature = "provider-claude")]
pub mod claude;

#[cfg(test)]
mod tests {
    use super::*;
    use std::task::{RawWaker, RawWakerVTable, Waker};

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
