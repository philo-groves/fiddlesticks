//! Chat service slices for non-streaming and streaming turn orchestration.
//!
//! ```rust
//! use fchat::ChatPolicy;
//! use std::time::Duration;
//!
//! let policy = ChatPolicy {
//!     max_tool_round_trips: 2,
//!     default_temperature: Some(0.2),
//!     default_max_tokens: Some(256),
//!     provider_retry_policy: fprovider::RetryPolicy {
//!         max_attempts: 2,
//!         initial_backoff: Duration::from_millis(10),
//!         max_backoff: Duration::from_millis(20),
//!         backoff_multiplier: 2.0,
//!     },
//! };
//!
//! assert_eq!(policy.max_tool_round_trips, 2);
//! ```

use std::collections::BTreeMap;
use std::sync::Arc;

use async_stream::try_stream;
use fprovider::{
    Message, ModelProvider, ModelRequest, NoopOperationHooks, OutputItem, ProviderOperationHooks,
    RetryPolicy, Role, StopReason, StreamEvent, TokenUsage, ToolCall, ToolResult,
    execute_with_retry,
};
use ftooling::{ToolExecutionContext, ToolRuntime};
use futures_timer::Delay;
use futures_util::StreamExt;

use crate::{
    ChatError, ChatErrorPhase, ChatEvent, ChatEventStream, ChatTurnRequest, ChatTurnResult,
    ConversationStore, InMemoryConversationStore,
};

#[derive(Debug, Clone, PartialEq)]
pub struct ChatPolicy {
    pub max_tool_round_trips: usize,
    pub default_temperature: Option<f32>,
    pub default_max_tokens: Option<u32>,
    pub provider_retry_policy: RetryPolicy,
}

impl Default for ChatPolicy {
    fn default() -> Self {
        Self {
            max_tool_round_trips: 4,
            default_temperature: None,
            default_max_tokens: None,
            provider_retry_policy: RetryPolicy::default(),
        }
    }
}

pub struct ChatServiceBuilder {
    provider: Arc<dyn ModelProvider>,
    store: Arc<dyn ConversationStore>,
    tool_runtime: Option<Arc<dyn ToolRuntime>>,
    provider_hooks: Arc<dyn ProviderOperationHooks>,
    policy: ChatPolicy,
}

impl ChatServiceBuilder {
    pub fn new(provider: Arc<dyn ModelProvider>) -> Self {
        Self {
            provider,
            store: Arc::new(InMemoryConversationStore::new()),
            tool_runtime: None,
            provider_hooks: Arc::new(NoopOperationHooks),
            policy: ChatPolicy::default(),
        }
    }

    pub fn store(mut self, store: Arc<dyn ConversationStore>) -> Self {
        self.store = store;
        self
    }

    pub fn tool_runtime(mut self, tool_runtime: Arc<dyn ToolRuntime>) -> Self {
        self.tool_runtime = Some(tool_runtime);
        self
    }

    pub fn provider_operation_hooks(mut self, hooks: Arc<dyn ProviderOperationHooks>) -> Self {
        self.provider_hooks = hooks;
        self
    }

    pub fn policy(mut self, policy: ChatPolicy) -> Self {
        self.policy = policy;
        self
    }

    pub fn max_tool_round_trips(mut self, max_tool_round_trips: usize) -> Self {
        self.policy.max_tool_round_trips = max_tool_round_trips;
        self
    }

    pub fn default_temperature(mut self, temperature: Option<f32>) -> Self {
        self.policy.default_temperature = temperature;
        self
    }

    pub fn default_max_tokens(mut self, max_tokens: Option<u32>) -> Self {
        self.policy.default_max_tokens = max_tokens;
        self
    }

    pub fn provider_retry_policy(mut self, provider_retry_policy: RetryPolicy) -> Self {
        self.policy.provider_retry_policy = provider_retry_policy;
        self
    }

    pub fn build(self) -> ChatService {
        ChatService {
            provider: self.provider,
            store: self.store,
            tool_runtime: self.tool_runtime,
            provider_hooks: self.provider_hooks,
            policy: self.policy,
        }
    }
}

#[derive(Clone)]
pub struct ChatService {
    provider: Arc<dyn ModelProvider>,
    store: Arc<dyn ConversationStore>,
    tool_runtime: Option<Arc<dyn ToolRuntime>>,
    provider_hooks: Arc<dyn ProviderOperationHooks>,
    policy: ChatPolicy,
}

impl ChatService {
    pub fn builder(provider: Arc<dyn ModelProvider>) -> ChatServiceBuilder {
        ChatServiceBuilder::new(provider)
    }

    pub fn new(provider: Arc<dyn ModelProvider>, store: Arc<dyn ConversationStore>) -> Self {
        Self {
            provider,
            store,
            tool_runtime: None,
            provider_hooks: Arc::new(NoopOperationHooks),
            policy: ChatPolicy::default(),
        }
    }

    pub fn with_tool_runtime(mut self, tool_runtime: Arc<dyn ToolRuntime>) -> Self {
        self.tool_runtime = Some(tool_runtime);
        self
    }

    pub fn with_provider_operation_hooks(mut self, hooks: Arc<dyn ProviderOperationHooks>) -> Self {
        self.provider_hooks = hooks;
        self
    }

    pub fn with_max_tool_round_trips(mut self, max_tool_round_trips: usize) -> Self {
        self.policy.max_tool_round_trips = max_tool_round_trips;
        self
    }

    pub fn with_policy(mut self, policy: ChatPolicy) -> Self {
        self.policy = policy;
        self
    }

    pub async fn run_turn(&self, request: ChatTurnRequest) -> Result<ChatTurnResult, ChatError> {
        if request.options.stream {
            return Err(ChatError::invalid_request(
                "use stream_turn for streaming requests",
            ));
        }

        let TurnContext {
            session,
            user_message,
            mut conversation_messages,
            temperature,
            max_tokens,
        } = self.prepare_turn(request).await?;

        let mut persisted_messages = vec![user_message];
        let mut model_response = self
            .complete_with_retry(
                session.provider,
                build_request(
                    &session.model,
                    &conversation_messages,
                    temperature,
                    max_tokens,
                    false,
                    Vec::new(),
                )?,
            )
            .await?;

        let mut round_trips = 0;
        loop {
            let (assistant_message, tool_calls) = collect_output(model_response.output);
            let assistant = Message::new(Role::Assistant, assistant_message.clone());
            conversation_messages.push(assistant.clone());
            persisted_messages.push(assistant);

            let has_tool_runtime = self.tool_runtime.is_some();
            let limit_reached = has_tool_runtime
                && !tool_calls.is_empty()
                && round_trips >= self.policy.max_tool_round_trips;

            let should_run_tools = has_tool_runtime
                && self.policy.max_tool_round_trips > 0
                && !tool_calls.is_empty()
                && round_trips < self.policy.max_tool_round_trips;

            if !should_run_tools {
                self.store
                    .append_messages(&session.id, persisted_messages)
                    .await
                    .map_err(|err| err.with_phase(ChatErrorPhase::Storage))?;

                return Ok(ChatTurnResult {
                    session_id: session.id,
                    assistant_message,
                    tool_calls,
                    stop_reason: model_response.stop_reason,
                    usage: model_response.usage,
                    tool_round_limit_reached: limit_reached,
                });
            }

            let runtime = self.tool_runtime.as_ref().expect("runtime checked");
            let mut tool_results = Vec::new();
            for tool_call in tool_calls {
                let result = runtime
                    .execute(tool_call, ToolExecutionContext::new(session.id.clone()))
                    .await
                    .map_err(|err| ChatError::from(err).with_phase(ChatErrorPhase::Tooling))?;
                tool_results.push(ToolResult {
                    tool_call_id: result.tool_call_id,
                    output: result.output,
                });
            }

            round_trips += 1;
            model_response = self
                .complete_with_retry(
                    session.provider,
                    build_request(
                        &session.model,
                        &conversation_messages,
                        temperature,
                        max_tokens,
                        false,
                        tool_results,
                    )?,
                )
                .await?;
        }
    }

    pub async fn stream_turn<'a>(
        &'a self,
        request: ChatTurnRequest,
    ) -> Result<ChatEventStream<'a>, ChatError> {
        let TurnContext {
            session,
            user_message,
            mut conversation_messages,
            temperature,
            max_tokens,
        } = self.prepare_turn(request).await?;

        let provider = Arc::clone(&self.provider);
        let provider_hooks = Arc::clone(&self.provider_hooks);
        let store = Arc::clone(&self.store);
        let tool_runtime = self.tool_runtime.clone();
        let retry_policy = self.policy.provider_retry_policy.clone();
        let max_tool_round_trips = self.policy.max_tool_round_trips;

        let stream = try_stream! {
            let mut persisted_messages = vec![user_message.clone()];
            let mut round_trips = 0usize;
            let mut next_tool_results = Vec::<ToolResult>::new();

            loop {
                let request = build_request(
                    &session.model,
                    &conversation_messages,
                    temperature,
                    max_tokens,
                    true,
                    next_tool_results,
                )?;

                let mut provider_stream = {
                    let mut attempt = 1_u32;
                    loop {
                        provider_hooks.on_attempt_start(session.provider, "stream", attempt);
                        match provider.stream(request.clone()).await {
                            Ok(stream) => {
                                provider_hooks.on_success(session.provider, "stream", attempt);
                                break stream;
                            }
                            Err(err)
                                if retry_policy.should_retry(attempt, &err) => {
                                    let delay = retry_policy.backoff_for_attempt(attempt);
                                    provider_hooks.on_retry_scheduled(
                                        session.provider,
                                        "stream",
                                        attempt,
                                        delay,
                                        &err,
                                    );
                                    Delay::new(delay).await;
                                    attempt += 1;
                                }
                            Err(err) => {
                                provider_hooks.on_failure(session.provider, "stream", attempt, &err);
                                break Err::<fprovider::BoxedEventStream<'_>, _>(err)
                                    .map_err(|err| ChatError::from(err).with_phase(ChatErrorPhase::Provider))?;
                            }
                        }
                    }
                };

                let mut assistant_text = String::new();
                let mut tool_calls = BTreeMap::<String, ToolCall>::new();
                let mut stop_reason = StopReason::Other;
                let mut usage = TokenUsage::default();

                while let Some(event) = provider_stream.next().await {
                    let event = event.map_err(|err| ChatError::from(err).with_phase(ChatErrorPhase::Streaming))?;
                    match event {
                        StreamEvent::TextDelta(delta) => {
                            assistant_text.push_str(&delta);
                            yield ChatEvent::TextDelta(delta);
                        }
                        StreamEvent::ToolCallDelta(tool_call) => {
                            tool_calls.insert(tool_call.id.clone(), tool_call.clone());
                            yield ChatEvent::ToolCallDelta(tool_call);
                        }
                        StreamEvent::MessageComplete(message) => {
                            if message.role == Role::Assistant {
                                if assistant_text.is_empty() {
                                    assistant_text = message.content.clone();
                                }
                                yield ChatEvent::AssistantMessageComplete(message.content);
                            }
                        }
                        StreamEvent::ResponseComplete(response) => {
                            let (content, output_tool_calls) = collect_output(response.output);
                            if !content.is_empty() {
                                assistant_text = content;
                            }

                            for tool_call in output_tool_calls {
                                tool_calls.insert(tool_call.id.clone(), tool_call);
                            }

                            stop_reason = response.stop_reason;
                            usage = response.usage;
                        }
                    }
                }

                let tool_calls_vec = tool_calls.values().cloned().collect::<Vec<_>>();
                let assistant = Message::new(Role::Assistant, assistant_text.clone());
                conversation_messages.push(assistant.clone());
                persisted_messages.push(assistant);

                let has_tool_runtime = tool_runtime.is_some();
                let limit_reached = has_tool_runtime
                    && !tool_calls_vec.is_empty()
                    && round_trips >= max_tool_round_trips;
                let should_run_tools = has_tool_runtime
                    && !tool_calls_vec.is_empty()
                    && round_trips < max_tool_round_trips;

                if limit_reached {
                    yield ChatEvent::ToolRoundLimitReached {
                        max_round_trips: max_tool_round_trips,
                        pending_tool_calls: tool_calls_vec.len(),
                    };
                }

                if should_run_tools {
                    let runtime = tool_runtime.as_ref().expect("runtime exists");
                    let mut tool_results = Vec::new();
                    for tool_call in tool_calls_vec {
                        yield ChatEvent::ToolExecutionStarted(tool_call.clone());
                        let executed = runtime
                            .execute(tool_call.clone(), ToolExecutionContext::new(session.id.clone()))
                            .await
                            .map_err(|err| ChatError::from(err).with_phase(ChatErrorPhase::Tooling))?;
                        yield ChatEvent::ToolExecutionFinished(tool_call);
                        tool_results.push(ToolResult {
                            tool_call_id: executed.tool_call_id,
                            output: executed.output,
                        });
                    }

                    round_trips += 1;
                    next_tool_results = tool_results;
                    continue;
                }

                let turn_result = ChatTurnResult {
                    session_id: session.id.clone(),
                    assistant_message: assistant_text,
                    tool_calls: tool_calls_vec,
                    stop_reason,
                    usage,
                    tool_round_limit_reached: limit_reached,
                };

                store
                    .append_messages(&session.id, persisted_messages)
                    .await
                    .map_err(|err| err.with_phase(ChatErrorPhase::Storage))?;

                yield ChatEvent::TurnComplete(turn_result);
                break;
            }
        };

        Ok(Box::pin(stream))
    }

    async fn prepare_turn(&self, request: ChatTurnRequest) -> Result<TurnContext, ChatError> {
        if request.user_input.trim().is_empty() {
            return Err(ChatError::invalid_request("user_input must not be empty"));
        }

        let ChatTurnRequest {
            session,
            user_input,
            options,
        } = request;

        let temperature = options.temperature.or(self.policy.default_temperature);
        let max_tokens = options.max_tokens.or(self.policy.default_max_tokens);

        let prior = self
            .store
            .load_messages(&session.id)
            .await
            .map_err(|err| err.with_phase(ChatErrorPhase::Storage))?;
        let user_message = Message::new(Role::User, user_input);

        let mut conversation_messages = Vec::new();
        if let Some(system_prompt) = &session.system_prompt {
            conversation_messages.push(Message::new(Role::System, system_prompt.clone()));
        }

        conversation_messages.extend(prior);
        conversation_messages.push(user_message.clone());

        Ok(TurnContext {
            session,
            user_message,
            conversation_messages,
            temperature,
            max_tokens,
        })
    }

    async fn complete_with_retry(
        &self,
        provider_id: fprovider::ProviderId,
        request: ModelRequest,
    ) -> Result<fprovider::ModelResponse, ChatError> {
        let provider = Arc::clone(&self.provider);
        let policy = self.policy.provider_retry_policy.clone();
        let hooks = Arc::clone(&self.provider_hooks);

        execute_with_retry(
            provider_id,
            "complete",
            &policy,
            hooks.as_ref(),
            |_| {
                let provider = Arc::clone(&provider);
                let request = request.clone();
                async move { provider.complete(request).await }
            },
            |delay| async move {
                Delay::new(delay).await;
            },
        )
        .await
        .map_err(|err| ChatError::from(err).with_phase(ChatErrorPhase::Provider))
    }
}

struct TurnContext {
    session: crate::ChatSession,
    user_message: Message,
    conversation_messages: Vec<Message>,
    temperature: Option<f32>,
    max_tokens: Option<u32>,
}

fn build_request(
    model: &str,
    messages: &[Message],
    temperature: Option<f32>,
    max_tokens: Option<u32>,
    stream: bool,
    tool_results: Vec<ToolResult>,
) -> Result<ModelRequest, ChatError> {
    let mut builder = ModelRequest::builder(model.to_string()).messages(messages.to_vec());

    if let Some(value) = temperature {
        builder = builder.temperature(value);
    }

    if let Some(value) = max_tokens {
        builder = builder.max_tokens(value);
    }

    if stream {
        builder = builder.enable_streaming();
    }

    if !tool_results.is_empty() {
        builder = builder.tool_results(tool_results);
    }

    builder
        .build()
        .map_err(|err| ChatError::from(err).with_phase(ChatErrorPhase::RequestValidation))
}

fn collect_output(items: Vec<OutputItem>) -> (String, Vec<ToolCall>) {
    let mut text = String::new();
    let mut tool_calls = Vec::new();

    for item in items {
        match item {
            OutputItem::Message(message) => {
                if message.role == Role::Assistant {
                    text.push_str(&message.content);
                }
            }
            OutputItem::ToolCall(call) => tool_calls.push(call),
        }
    }

    (text, tool_calls)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use fprovider::{
        ModelResponse, ProviderFuture, ProviderId, RetryPolicy, StopReason, StreamEvent,
        TokenUsage, ToolCall, VecEventStream,
    };
    use ftooling::{ToolExecutionResult, ToolFuture};
    use futures_util::StreamExt;

    use super::*;
    use crate::{ChatErrorKind, ChatSession, InMemoryConversationStore};
    use fcommon::SessionId;

    #[derive(Debug)]
    struct FakeProvider {
        requests: Mutex<Vec<ModelRequest>>,
    }

    impl FakeProvider {
        fn new() -> Self {
            Self {
                requests: Mutex::new(Vec::new()),
            }
        }
    }

    impl ModelProvider for FakeProvider {
        fn id(&self) -> ProviderId {
            ProviderId::OpenAi
        }

        fn complete<'a>(
            &'a self,
            request: ModelRequest,
        ) -> ProviderFuture<'a, Result<ModelResponse, fprovider::ProviderError>> {
            Box::pin(async move {
                self.requests
                    .lock()
                    .expect("requests lock")
                    .push(request.clone());

                if !request.tool_results.is_empty() {
                    return Ok(ModelResponse {
                        provider: ProviderId::OpenAi,
                        model: request.model,
                        output: vec![OutputItem::Message(Message::new(
                            Role::Assistant,
                            "tool answer",
                        ))],
                        stop_reason: StopReason::EndTurn,
                        usage: TokenUsage {
                            input_tokens: 14,
                            output_tokens: 5,
                            total_tokens: 19,
                        },
                    });
                }

                Ok(ModelResponse {
                    provider: ProviderId::OpenAi,
                    model: request.model,
                    output: vec![
                        OutputItem::Message(Message::new(Role::Assistant, "assistant reply")),
                        OutputItem::ToolCall(ToolCall {
                            id: "call_1".to_string(),
                            name: "lookup".to_string(),
                            arguments: "{}".to_string(),
                        }),
                    ],
                    stop_reason: StopReason::EndTurn,
                    usage: TokenUsage {
                        input_tokens: 10,
                        output_tokens: 4,
                        total_tokens: 14,
                    },
                })
            })
        }

        fn stream<'a>(
            &'a self,
            request: ModelRequest,
        ) -> ProviderFuture<'a, Result<fprovider::BoxedEventStream<'a>, fprovider::ProviderError>>
        {
            Box::pin(async move {
                self.requests
                    .lock()
                    .expect("requests lock")
                    .push(request.clone());

                let final_response = if !request.tool_results.is_empty() {
                    ModelResponse {
                        provider: ProviderId::OpenAi,
                        model: request.model,
                        output: vec![OutputItem::Message(Message::new(
                            Role::Assistant,
                            "tool stream answer",
                        ))],
                        stop_reason: StopReason::EndTurn,
                        usage: TokenUsage {
                            input_tokens: 15,
                            output_tokens: 6,
                            total_tokens: 21,
                        },
                    }
                } else {
                    ModelResponse {
                        provider: ProviderId::OpenAi,
                        model: request.model,
                        output: vec![
                            OutputItem::Message(Message::new(Role::Assistant, "hello world")),
                            OutputItem::ToolCall(ToolCall {
                                id: "call_2".to_string(),
                                name: "search".to_string(),
                                arguments: "{}".to_string(),
                            }),
                        ],
                        stop_reason: StopReason::EndTurn,
                        usage: TokenUsage {
                            input_tokens: 12,
                            output_tokens: 6,
                            total_tokens: 18,
                        },
                    }
                };

                let stream = VecEventStream::new(vec![
                    Ok(StreamEvent::TextDelta("hello".to_string())),
                    Ok(StreamEvent::TextDelta(" world".to_string())),
                    Ok(StreamEvent::ResponseComplete(final_response)),
                ]);

                Ok(Box::pin(stream) as fprovider::BoxedEventStream<'a>)
            })
        }
    }

    #[derive(Debug, Default)]
    struct FakeToolRuntime;

    impl ToolRuntime for FakeToolRuntime {
        fn execute<'a>(
            &'a self,
            tool_call: ToolCall,
            _context: ToolExecutionContext,
        ) -> ToolFuture<'a, Result<ToolExecutionResult, ftooling::ToolError>> {
            Box::pin(async move {
                Ok(ToolExecutionResult {
                    tool_call_id: tool_call.id,
                    output: "{\"result\":\"ok\"}".to_string(),
                })
            })
        }
    }

    #[derive(Debug, Default)]
    struct FailingToolRuntime;

    impl ToolRuntime for FailingToolRuntime {
        fn execute<'a>(
            &'a self,
            tool_call: ToolCall,
            _context: ToolExecutionContext,
        ) -> ToolFuture<'a, Result<ToolExecutionResult, ftooling::ToolError>> {
            Box::pin(async move {
                Err(
                    ftooling::ToolError::invalid_arguments("missing required field")
                        .with_tool_name(tool_call.name)
                        .with_tool_call_id(tool_call.id),
                )
            })
        }
    }

    #[derive(Debug)]
    struct FlakyProvider {
        attempts: Mutex<u32>,
    }

    impl FlakyProvider {
        fn new() -> Self {
            Self {
                attempts: Mutex::new(0),
            }
        }
    }

    impl ModelProvider for FlakyProvider {
        fn id(&self) -> ProviderId {
            ProviderId::OpenAi
        }

        fn complete<'a>(
            &'a self,
            request: ModelRequest,
        ) -> ProviderFuture<'a, Result<ModelResponse, fprovider::ProviderError>> {
            Box::pin(async move {
                let mut attempts = self.attempts.lock().expect("attempt lock");
                *attempts += 1;
                if *attempts == 1 {
                    return Err(fprovider::ProviderError::timeout("temporary timeout"));
                }

                Ok(ModelResponse {
                    provider: ProviderId::OpenAi,
                    model: request.model,
                    output: vec![OutputItem::Message(Message::new(
                        Role::Assistant,
                        "retry ok",
                    ))],
                    stop_reason: StopReason::EndTurn,
                    usage: TokenUsage {
                        input_tokens: 2,
                        output_tokens: 2,
                        total_tokens: 4,
                    },
                })
            })
        }

        fn stream<'a>(
            &'a self,
            _request: ModelRequest,
        ) -> ProviderFuture<'a, Result<fprovider::BoxedEventStream<'a>, fprovider::ProviderError>>
        {
            Box::pin(async {
                Err(fprovider::ProviderError::invalid_request(
                    "not used for flaky provider",
                ))
            })
        }
    }

    #[derive(Debug)]
    struct StreamErrorProvider;

    impl ModelProvider for StreamErrorProvider {
        fn id(&self) -> ProviderId {
            ProviderId::OpenAi
        }

        fn complete<'a>(
            &'a self,
            _request: ModelRequest,
        ) -> ProviderFuture<'a, Result<ModelResponse, fprovider::ProviderError>> {
            Box::pin(async {
                Err(fprovider::ProviderError::invalid_request(
                    "complete not used for stream error provider",
                ))
            })
        }

        fn stream<'a>(
            &'a self,
            _request: ModelRequest,
        ) -> ProviderFuture<'a, Result<fprovider::BoxedEventStream<'a>, fprovider::ProviderError>>
        {
            Box::pin(async {
                let stream = VecEventStream::new(vec![
                    Ok(StreamEvent::TextDelta("partial".to_string())),
                    Err(fprovider::ProviderError::transport("stream interrupted")),
                ]);
                Ok(Box::pin(stream) as fprovider::BoxedEventStream<'a>)
            })
        }
    }

    #[derive(Debug)]
    struct FlakyStreamProvider {
        attempts: Mutex<u32>,
    }

    #[derive(Default)]
    struct RecordingProviderHooks {
        events: Mutex<Vec<String>>,
    }

    impl fprovider::ProviderOperationHooks for RecordingProviderHooks {
        fn on_attempt_start(&self, provider: ProviderId, operation: &str, attempt: u32) {
            self.events
                .lock()
                .expect("events lock")
                .push(format!("start:{provider}:{operation}:{attempt}"));
        }

        fn on_success(&self, provider: ProviderId, operation: &str, attempts: u32) {
            self.events
                .lock()
                .expect("events lock")
                .push(format!("success:{provider}:{operation}:{attempts}"));
        }
    }

    impl FlakyStreamProvider {
        fn new() -> Self {
            Self {
                attempts: Mutex::new(0),
            }
        }
    }

    impl ModelProvider for FlakyStreamProvider {
        fn id(&self) -> ProviderId {
            ProviderId::OpenAi
        }

        fn complete<'a>(
            &'a self,
            _request: ModelRequest,
        ) -> ProviderFuture<'a, Result<ModelResponse, fprovider::ProviderError>> {
            Box::pin(async {
                Err(fprovider::ProviderError::invalid_request(
                    "complete not used for flaky stream provider",
                ))
            })
        }

        fn stream<'a>(
            &'a self,
            request: ModelRequest,
        ) -> ProviderFuture<'a, Result<fprovider::BoxedEventStream<'a>, fprovider::ProviderError>>
        {
            Box::pin(async move {
                let mut attempts = self.attempts.lock().expect("attempt lock");
                *attempts += 1;
                if *attempts == 1 {
                    return Err(fprovider::ProviderError::timeout(
                        "temporary stream timeout",
                    ));
                }

                let response = ModelResponse {
                    provider: ProviderId::OpenAi,
                    model: request.model,
                    output: vec![OutputItem::Message(Message::new(
                        Role::Assistant,
                        "stream retry ok",
                    ))],
                    stop_reason: StopReason::EndTurn,
                    usage: TokenUsage {
                        input_tokens: 2,
                        output_tokens: 2,
                        total_tokens: 4,
                    },
                };

                let stream = VecEventStream::new(vec![
                    Ok(StreamEvent::TextDelta("stream".to_string())),
                    Ok(StreamEvent::ResponseComplete(response)),
                ]);
                Ok(Box::pin(stream) as fprovider::BoxedEventStream<'a>)
            })
        }
    }

    #[tokio::test]
    async fn run_turn_returns_assistant_message_and_persists_transcript() {
        let provider = Arc::new(FakeProvider::new());
        let store = Arc::new(InMemoryConversationStore::new());
        let service = ChatService::new(provider, store.clone());

        let session = ChatSession::new("s1", ProviderId::OpenAi, "gpt-4o-mini");
        let request = ChatTurnRequest::new(session.clone(), "hello");

        let result = service.run_turn(request).await.expect("turn should work");
        assert_eq!(result.session_id, SessionId::from("s1"));
        assert_eq!(result.assistant_message, "assistant reply");
        assert_eq!(result.tool_calls.len(), 1);

        let saved = store.load_messages(&session.id).await.expect("load saved");
        assert_eq!(saved.len(), 2);
        assert_eq!(saved[0].role, Role::User);
        assert_eq!(saved[1].role, Role::Assistant);
    }

    #[tokio::test]
    async fn run_turn_executes_tools_when_runtime_configured() {
        let provider = Arc::new(FakeProvider::new());
        let store = Arc::new(InMemoryConversationStore::new());
        let runtime = Arc::new(FakeToolRuntime);
        let service = ChatService::new(provider.clone(), store)
            .with_tool_runtime(runtime)
            .with_max_tool_round_trips(2);

        let session = ChatSession::new("s_tool", ProviderId::OpenAi, "gpt-4o-mini");
        let request = ChatTurnRequest::new(session, "hello");
        let result = service.run_turn(request).await.expect("turn should work");

        assert_eq!(result.assistant_message, "tool answer");
        assert!(result.tool_calls.is_empty());

        let requests = provider.requests.lock().expect("requests lock");
        assert_eq!(requests.len(), 2);
        assert!(requests[1].tool_results.len() == 1);
    }

    #[tokio::test]
    async fn run_turn_includes_history_and_system_prompt_in_provider_request() {
        let provider = Arc::new(FakeProvider::new());
        let store = Arc::new(InMemoryConversationStore::new());

        store
            .append_messages(
                &SessionId::from("s2"),
                vec![Message::new(Role::User, "prior question")],
            )
            .await
            .expect("seed store");

        let service = ChatService::new(provider.clone(), store);
        let session = ChatSession::new("s2", ProviderId::OpenAi, "gpt-4o-mini")
            .with_system_prompt("be concise");
        let request = ChatTurnRequest::new(session, "new question");

        let _ = service.run_turn(request).await.expect("turn should work");

        let requests = provider.requests.lock().expect("requests lock");
        assert_eq!(requests.len(), 1);
        let sent = &requests[0];
        assert_eq!(sent.messages.len(), 3);
        assert_eq!(sent.messages[0], Message::new(Role::System, "be concise"));
        assert_eq!(sent.messages[1], Message::new(Role::User, "prior question"));
        assert_eq!(sent.messages[2], Message::new(Role::User, "new question"));
    }

    #[tokio::test]
    async fn run_turn_rejects_empty_user_input() {
        let provider = Arc::new(FakeProvider::new());
        let store = Arc::new(InMemoryConversationStore::new());
        let service = ChatService::new(provider.clone(), store);

        let session = ChatSession::new("s3", ProviderId::OpenAi, "gpt-4o-mini");
        let request = ChatTurnRequest::new(session, "   ");

        let error = service
            .run_turn(request)
            .await
            .expect_err("turn should fail");
        assert_eq!(error.kind, ChatErrorKind::InvalidRequest);
        assert!(provider.requests.lock().expect("requests lock").is_empty());
    }

    #[tokio::test]
    async fn stream_turn_maps_provider_events_and_persists_transcript() {
        let provider = Arc::new(FakeProvider::new());
        let store = Arc::new(InMemoryConversationStore::new());
        let service = ChatService::new(provider.clone(), store.clone());

        let session = ChatSession::new("s4", ProviderId::OpenAi, "gpt-4o-mini");
        let request = ChatTurnRequest::new(session.clone(), "hello").enable_streaming();

        let mut stream = service
            .stream_turn(request)
            .await
            .expect("stream should build");
        let mut collected = Vec::new();
        while let Some(event) = stream.next().await {
            collected.push(event.expect("event should be ok"));
        }

        assert_eq!(collected.len(), 3);
        assert!(matches!(collected[0], ChatEvent::TextDelta(_)));
        assert!(matches!(collected[1], ChatEvent::TextDelta(_)));
        assert!(matches!(collected[2], ChatEvent::TurnComplete(_)));

        let final_turn = match &collected[2] {
            ChatEvent::TurnComplete(turn) => turn,
            _ => unreachable!(),
        };
        assert_eq!(final_turn.assistant_message, "hello world");
        assert_eq!(final_turn.tool_calls.len(), 1);

        let saved = store.load_messages(&session.id).await.expect("load saved");
        assert_eq!(saved.len(), 2);
        assert_eq!(saved[1], Message::new(Role::Assistant, "hello world"));

        let requests = provider.requests.lock().expect("requests lock");
        assert!(requests[0].options.stream);
    }

    #[tokio::test]
    async fn builder_applies_default_turn_options_to_requests() {
        let provider = Arc::new(FakeProvider::new());
        let service = ChatService::builder(provider.clone())
            .default_temperature(Some(0.6))
            .default_max_tokens(Some(256))
            .build();

        let session = ChatSession::new("s5", ProviderId::OpenAi, "gpt-4o-mini");
        let request = ChatTurnRequest::new(session, "hello defaults");

        let _ = service.run_turn(request).await.expect("turn should work");

        let requests = provider.requests.lock().expect("requests lock");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].options.temperature, Some(0.6));
        assert_eq!(requests[0].options.max_tokens, Some(256));
    }

    #[tokio::test]
    async fn builder_configures_tool_runtime_and_round_trip_policy() {
        let provider = Arc::new(FakeProvider::new());
        let runtime = Arc::new(FakeToolRuntime);
        let service = ChatService::builder(provider.clone())
            .tool_runtime(runtime)
            .max_tool_round_trips(2)
            .build();

        let session = ChatSession::new("s6", ProviderId::OpenAi, "gpt-4o-mini");
        let request = ChatTurnRequest::new(session, "hello tools");

        let result = service.run_turn(request).await.expect("turn should work");
        assert_eq!(result.assistant_message, "tool answer");

        let requests = provider.requests.lock().expect("requests lock");
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[1].tool_results.len(), 1);
    }

    #[tokio::test]
    async fn run_turn_retries_provider_completion_using_policy() {
        let provider = Arc::new(FlakyProvider::new());
        let policy = ChatPolicy {
            max_tool_round_trips: 0,
            default_temperature: None,
            default_max_tokens: None,
            provider_retry_policy: RetryPolicy {
                max_attempts: 2,
                initial_backoff: Duration::from_millis(0),
                max_backoff: Duration::from_millis(0),
                backoff_multiplier: 1.0,
            },
        };

        let service = ChatService::builder(provider.clone())
            .policy(policy)
            .build();
        let session = ChatSession::new("s7", ProviderId::OpenAi, "gpt-4o-mini");

        let result = service
            .run_turn(ChatTurnRequest::new(session, "hello"))
            .await
            .expect("retry should succeed");
        assert_eq!(result.assistant_message, "retry ok");

        let attempts = provider.attempts.lock().expect("attempt lock");
        assert_eq!(*attempts, 2);
    }

    #[tokio::test]
    async fn run_turn_marks_limit_reached_when_tool_cap_prevents_execution() {
        let provider = Arc::new(FakeProvider::new());
        let runtime = Arc::new(FakeToolRuntime);
        let service = ChatService::builder(provider)
            .tool_runtime(runtime)
            .max_tool_round_trips(0)
            .build();

        let session = ChatSession::new("s8", ProviderId::OpenAi, "gpt-4o-mini");
        let result = service
            .run_turn(ChatTurnRequest::new(session, "hello"))
            .await
            .expect("turn should succeed");

        assert!(result.tool_round_limit_reached);
        assert_eq!(result.tool_calls.len(), 1);
    }

    #[tokio::test]
    async fn stream_turn_executes_tools_and_emits_tool_events() {
        let provider = Arc::new(FakeProvider::new());
        let runtime = Arc::new(FakeToolRuntime);
        let service = ChatService::builder(provider)
            .tool_runtime(runtime)
            .max_tool_round_trips(2)
            .build();

        let session = ChatSession::new("s9", ProviderId::OpenAi, "gpt-4o-mini");
        let mut stream = service
            .stream_turn(ChatTurnRequest::new(session, "hello").enable_streaming())
            .await
            .expect("stream should start");

        let mut started = 0;
        let mut finished = 0;
        let mut final_result = None;

        while let Some(event) = stream.next().await {
            match event.expect("event should be ok") {
                ChatEvent::ToolExecutionStarted(_) => started += 1,
                ChatEvent::ToolExecutionFinished(_) => finished += 1,
                ChatEvent::TurnComplete(result) => final_result = Some(result),
                _ => {}
            }
        }

        assert_eq!(started, 1);
        assert_eq!(finished, 1);
        let final_result = final_result.expect("turn complete expected");
        assert_eq!(final_result.assistant_message, "tool stream answer");
        assert!(!final_result.tool_round_limit_reached);
    }

    #[tokio::test]
    async fn stream_turn_reports_streaming_phase_errors() {
        let provider = Arc::new(StreamErrorProvider);
        let service = ChatService::builder(provider).build();
        let session = ChatSession::new("s10", ProviderId::OpenAi, "gpt-4o-mini");

        let mut stream = service
            .stream_turn(ChatTurnRequest::new(session, "hello").enable_streaming())
            .await
            .expect("stream should start");

        let first = stream
            .next()
            .await
            .expect("first event should exist")
            .expect("first event should be ok");
        assert!(matches!(first, ChatEvent::TextDelta(_)));

        let second = stream
            .next()
            .await
            .expect("error event should exist")
            .expect_err("second item should be error");
        assert_eq!(second.phase, Some(ChatErrorPhase::Streaming));
        assert!(second.is_retryable());
    }

    #[tokio::test]
    async fn stream_turn_retries_stream_acquisition_using_policy() {
        let provider = Arc::new(FlakyStreamProvider::new());
        let policy = ChatPolicy {
            max_tool_round_trips: 0,
            default_temperature: None,
            default_max_tokens: None,
            provider_retry_policy: RetryPolicy {
                max_attempts: 2,
                initial_backoff: Duration::from_millis(0),
                max_backoff: Duration::from_millis(0),
                backoff_multiplier: 1.0,
            },
        };

        let service = ChatService::builder(provider.clone())
            .policy(policy)
            .build();
        let session = ChatSession::new("s11", ProviderId::OpenAi, "gpt-4o-mini");
        let mut stream = service
            .stream_turn(ChatTurnRequest::new(session, "hello").enable_streaming())
            .await
            .expect("stream should start");

        let mut saw_turn_complete = false;
        while let Some(item) = stream.next().await {
            if matches!(
                item.expect("event should be ok"),
                ChatEvent::TurnComplete(_)
            ) {
                saw_turn_complete = true;
            }
        }

        assert!(saw_turn_complete);
        let attempts = provider.attempts.lock().expect("attempt lock");
        assert_eq!(*attempts, 2);
    }

    #[tokio::test]
    async fn provider_hooks_are_called_for_complete_and_stream_operations() {
        let hooks = Arc::new(RecordingProviderHooks::default());

        let provider = Arc::new(FakeProvider::new());
        let service = ChatService::builder(provider)
            .provider_operation_hooks(hooks.clone())
            .build();
        let session = ChatSession::new("s13", ProviderId::OpenAi, "gpt-4o-mini");
        let _ = service
            .run_turn(ChatTurnRequest::new(session, "hello"))
            .await
            .expect("turn should succeed");

        let provider = Arc::new(FakeProvider::new());
        let service = ChatService::builder(provider)
            .provider_operation_hooks(hooks.clone())
            .build();
        let session = ChatSession::new("s14", ProviderId::OpenAi, "gpt-4o-mini");
        let mut stream = service
            .stream_turn(ChatTurnRequest::new(session, "hello").enable_streaming())
            .await
            .expect("stream should start");

        while let Some(item) = stream.next().await {
            item.expect("stream event should be ok");
        }

        let events = hooks.events.lock().expect("events lock").clone();
        assert!(
            events
                .iter()
                .any(|event| event == "start:openai:complete:1")
        );
        assert!(
            events
                .iter()
                .any(|event| event == "success:openai:complete:1")
        );
        assert!(events.iter().any(|event| event == "start:openai:stream:1"));
        assert!(
            events
                .iter()
                .any(|event| event == "success:openai:stream:1")
        );
    }

    #[tokio::test]
    async fn stream_turn_maps_tool_runtime_failures_to_tooling_errors() {
        let provider = Arc::new(FakeProvider::new());
        let runtime = Arc::new(FailingToolRuntime);
        let service = ChatService::builder(provider)
            .tool_runtime(runtime)
            .max_tool_round_trips(2)
            .build();

        let session = ChatSession::new("s12", ProviderId::OpenAi, "gpt-4o-mini");
        let mut stream = service
            .stream_turn(ChatTurnRequest::new(session, "hello").enable_streaming())
            .await
            .expect("stream should start");

        let mut saw_tool_start = false;
        while let Some(item) = stream.next().await {
            match item {
                Ok(ChatEvent::ToolExecutionStarted(_)) => saw_tool_start = true,
                Ok(_) => {}
                Err(error) => {
                    assert_eq!(error.phase, Some(ChatErrorPhase::Tooling));
                    assert!(error.is_user_error());
                    assert!(saw_tool_start);
                    return;
                }
            }
        }

        panic!("expected tooling error in stream");
    }
}
