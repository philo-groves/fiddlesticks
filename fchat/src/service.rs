//! Chat service slices for non-streaming and streaming turn orchestration.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_stream::try_stream;
use futures_util::StreamExt;
use fprovider::{
    Message, ModelProvider, ModelRequest, OutputItem, Role, StopReason, StreamEvent, ToolCall,
    ToolResult, TokenUsage,
};
use ftooling::{ToolExecutionContext, ToolRuntime};

use crate::{
    ChatError, ChatErrorPhase, ChatEvent, ChatEventStream, ChatTurnRequest, ChatTurnResult,
    ConversationStore, InMemoryConversationStore,
};

#[derive(Debug, Clone, PartialEq)]
pub struct ChatPolicy {
    pub max_tool_round_trips: usize,
    pub default_temperature: Option<f32>,
    pub default_max_tokens: Option<u32>,
}

impl Default for ChatPolicy {
    fn default() -> Self {
        Self {
            max_tool_round_trips: 4,
            default_temperature: None,
            default_max_tokens: None,
        }
    }
}

pub struct ChatServiceBuilder {
    provider: Arc<dyn ModelProvider>,
    store: Arc<dyn ConversationStore>,
    tool_runtime: Option<Arc<dyn ToolRuntime>>,
    policy: ChatPolicy,
}

impl ChatServiceBuilder {
    pub fn new(provider: Arc<dyn ModelProvider>) -> Self {
        Self {
            provider,
            store: Arc::new(InMemoryConversationStore::new()),
            tool_runtime: None,
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

    pub fn build(self) -> ChatService {
        ChatService {
            provider: self.provider,
            store: self.store,
            tool_runtime: self.tool_runtime,
            policy: self.policy,
        }
    }
}

#[derive(Clone)]
pub struct ChatService {
    provider: Arc<dyn ModelProvider>,
    store: Arc<dyn ConversationStore>,
    tool_runtime: Option<Arc<dyn ToolRuntime>>,
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
            policy: ChatPolicy::default(),
        }
    }

    pub fn with_tool_runtime(mut self, tool_runtime: Arc<dyn ToolRuntime>) -> Self {
        self.tool_runtime = Some(tool_runtime);
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
        if request.stream {
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
            .provider
            .complete(build_request(
                &session.model,
                &conversation_messages,
                temperature,
                max_tokens,
                false,
                Vec::new(),
            )?)
            .await
            .map_err(|err| ChatError::from(err).with_phase(ChatErrorPhase::Provider))?;

        let mut round_trips = 0;
        loop {
            let (assistant_message, tool_calls) = collect_output(model_response.output);
            let assistant = Message::new(Role::Assistant, assistant_message.clone());
            conversation_messages.push(assistant.clone());
            persisted_messages.push(assistant);

            let should_run_tools = self.tool_runtime.is_some()
                && self.policy.max_tool_round_trips > 0
                && !tool_calls.is_empty()
                && round_trips < self.policy.max_tool_round_trips;

            if !should_run_tools {
                self.store
                    .append_messages(&session.id, persisted_messages)
                    .await
                    .map_err(|err| ChatError::from(err).with_phase(ChatErrorPhase::Storage))?;

                return Ok(ChatTurnResult {
                    session_id: session.id,
                    assistant_message,
                    tool_calls,
                    stop_reason: model_response.stop_reason,
                    usage: model_response.usage,
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
                .provider
                .complete(build_request(
                    &session.model,
                    &conversation_messages,
                    temperature,
                    max_tokens,
                    false,
                    tool_results,
                )?)
                .await
                .map_err(|err| ChatError::from(err).with_phase(ChatErrorPhase::Provider))?;
        }
    }

    pub async fn stream_turn<'a>(
        &'a self,
        request: ChatTurnRequest,
    ) -> Result<ChatEventStream<'a>, ChatError> {
        let TurnContext {
            session,
            user_message,
            conversation_messages,
            temperature,
            max_tokens,
        } = self.prepare_turn(request).await?;

        let mut provider_stream = self
            .provider
            .stream(build_request(
                &session.model,
                &conversation_messages,
                temperature,
                max_tokens,
                true,
                Vec::new(),
            )?)
            .await
            .map_err(|err| ChatError::from(err).with_phase(ChatErrorPhase::Provider))?;

        let store = Arc::clone(&self.store);
        let stream = try_stream! {
            let mut assistant_text = String::new();
            let mut tool_calls = BTreeMap::<String, ToolCall>::new();
            let mut final_result = None::<ChatTurnResult>;

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
                        if message.role == Role::Assistant && assistant_text.is_empty() {
                            assistant_text = message.content.clone();
                        }

                        if message.role == Role::Assistant {
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

                        final_result = Some(ChatTurnResult {
                            session_id: session.id.clone(),
                            assistant_message: assistant_text.clone(),
                            tool_calls: tool_calls.values().cloned().collect(),
                            stop_reason: response.stop_reason,
                            usage: response.usage,
                        });
                    }
                }
            }

            let turn_result = final_result.unwrap_or(ChatTurnResult {
                session_id: session.id.clone(),
                assistant_message: assistant_text.clone(),
                tool_calls: tool_calls.values().cloned().collect(),
                stop_reason: StopReason::Other,
                usage: TokenUsage::default(),
            });

            let assistant = Message::new(Role::Assistant, turn_result.assistant_message.clone());
            store
                .append_messages(&session.id, vec![user_message, assistant])
                .await
                .map_err(|err| err.with_phase(ChatErrorPhase::Storage))?;

            yield ChatEvent::TurnComplete(turn_result);
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
            temperature,
            max_tokens,
            stream: _,
        } = request;

        let temperature = temperature.or(self.policy.default_temperature);
        let max_tokens = max_tokens.or(self.policy.default_max_tokens);

        let prior = self
            .store
            .load_messages(&session.id)
            .await
            .map_err(|err| ChatError::from(err).with_phase(ChatErrorPhase::Storage))?;
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

    use futures_util::StreamExt;
    use fprovider::{
        ModelResponse, ProviderFuture, ProviderId, StopReason, StreamEvent, TokenUsage, ToolCall,
        VecEventStream,
    };
    use ftooling::{ToolExecutionResult, ToolFuture};

    use super::*;
    use crate::{ChatErrorKind, ChatSession, InMemoryConversationStore};

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
        ) -> ProviderFuture<'a, Result<fprovider::BoxedEventStream<'a>, fprovider::ProviderError>> {
            Box::pin(async move {
                self.requests
                    .lock()
                    .expect("requests lock")
                    .push(request.clone());

                let final_response = ModelResponse {
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

    #[tokio::test]
    async fn run_turn_returns_assistant_message_and_persists_transcript() {
        let provider = Arc::new(FakeProvider::new());
        let store = Arc::new(InMemoryConversationStore::new());
        let service = ChatService::new(provider, store.clone());

        let session = ChatSession::new("s1", ProviderId::OpenAi, "gpt-4o-mini");
        let request = ChatTurnRequest::new(session.clone(), "hello");

        let result = service.run_turn(request).await.expect("turn should work");
        assert_eq!(result.session_id, "s1");
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
            .append_messages("s2", vec![Message::new(Role::User, "prior question")])
            .await
            .expect("seed store");

        let service = ChatService::new(provider.clone(), store);
        let session =
            ChatSession::new("s2", ProviderId::OpenAi, "gpt-4o-mini").with_system_prompt("be concise");
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

        let error = service.run_turn(request).await.expect_err("turn should fail");
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

        let mut stream = service.stream_turn(request).await.expect("stream should build");
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
        assert!(requests[0].stream);
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
        assert_eq!(requests[0].temperature, Some(0.6));
        assert_eq!(requests[0].max_tokens, Some(256));
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
}
