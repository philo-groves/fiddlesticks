//! Chat service slices for non-streaming and streaming turn orchestration.

use std::collections::{BTreeMap, VecDeque};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use futures_core::Stream;
use futures_util::StreamExt;
use fprovider::{
    Message, ModelProvider, ModelRequest, OutputItem, Role, StreamEvent, ToolCall,
};

use crate::{
    ChatError, ChatEvent, ChatEventStream, ChatTurnRequest, ChatTurnResult, ConversationStore,
};

#[derive(Clone)]
pub struct ChatService {
    provider: Arc<dyn ModelProvider>,
    store: Arc<dyn ConversationStore>,
}

impl ChatService {
    pub fn new(provider: Arc<dyn ModelProvider>, store: Arc<dyn ConversationStore>) -> Self {
        Self { provider, store }
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
            model_request,
        } = self.prepare_turn(request, false).await?;

        let model_response = self.provider.complete(model_request).await?;
        let (assistant_message, tool_calls) = collect_output(model_response.output);
        let assistant = Message::new(Role::Assistant, assistant_message.clone());

        self.store
            .append_messages(&session.id, vec![user_message, assistant])
            .await?;

        Ok(ChatTurnResult {
            session_id: session.id,
            assistant_message,
            tool_calls,
            stop_reason: model_response.stop_reason,
            usage: model_response.usage,
        })
    }

    pub async fn stream_turn<'a>(
        &'a self,
        request: ChatTurnRequest,
    ) -> Result<ChatEventStream<'a>, ChatError> {
        let TurnContext {
            session,
            user_message,
            model_request,
        } = self.prepare_turn(request, true).await?;

        let mut provider_stream = self.provider.stream(model_request).await?;
        let mut events = Vec::new();
        let mut assistant_text = String::new();
        let mut tool_calls = BTreeMap::<String, ToolCall>::new();
        let mut final_result = None::<ChatTurnResult>;

        while let Some(event) = provider_stream.next().await {
            let event = event.map_err(ChatError::from)?;
            match event {
                StreamEvent::TextDelta(delta) => {
                    assistant_text.push_str(&delta);
                    events.push(Ok(ChatEvent::TextDelta(delta)));
                }
                StreamEvent::ToolCallDelta(tool_call) => {
                    tool_calls.insert(tool_call.id.clone(), tool_call.clone());
                    events.push(Ok(ChatEvent::ToolCallDelta(tool_call)));
                }
                StreamEvent::MessageComplete(message) => {
                    if message.role == Role::Assistant && assistant_text.is_empty() {
                        assistant_text = message.content.clone();
                    }

                    if message.role == Role::Assistant {
                        events.push(Ok(ChatEvent::AssistantMessageComplete(message.content)));
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
            stop_reason: fprovider::StopReason::Other,
            usage: fprovider::TokenUsage::default(),
        });

        let assistant = Message::new(Role::Assistant, turn_result.assistant_message.clone());
        self.store
            .append_messages(&session.id, vec![user_message, assistant])
            .await?;

        events.push(Ok(ChatEvent::TurnComplete(turn_result)));
        Ok(Box::pin(BufferedChatEventStream::new(events)))
    }

    async fn prepare_turn(
        &self,
        request: ChatTurnRequest,
        stream: bool,
    ) -> Result<TurnContext, ChatError> {
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

        let prior = self.store.load_messages(&session.id).await?;
        let user_message = Message::new(Role::User, user_input);

        let mut messages = Vec::new();
        if let Some(system_prompt) = &session.system_prompt {
            messages.push(Message::new(Role::System, system_prompt.clone()));
        }

        messages.extend(prior);
        messages.push(user_message.clone());

        let mut builder = ModelRequest::builder(session.model.clone()).messages(messages);
        if let Some(value) = temperature {
            builder = builder.temperature(value);
        }

        if let Some(value) = max_tokens {
            builder = builder.max_tokens(value);
        }

        if stream {
            builder = builder.enable_streaming();
        }

        Ok(TurnContext {
            session,
            user_message,
            model_request: builder.build()?,
        })
    }
}

struct TurnContext {
    session: crate::ChatSession,
    user_message: Message,
    model_request: ModelRequest,
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

#[derive(Debug)]
struct BufferedChatEventStream {
    events: VecDeque<Result<ChatEvent, ChatError>>,
}

impl BufferedChatEventStream {
    fn new(events: Vec<Result<ChatEvent, ChatError>>) -> Self {
        Self {
            events: events.into(),
        }
    }
}

impl Stream for BufferedChatEventStream {
    type Item = Result<ChatEvent, ChatError>;

    fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(self.events.pop_front())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use futures_util::StreamExt;
    use fprovider::{
        ModelResponse, ProviderFuture, ProviderId, StopReason, StreamEvent, TokenUsage, ToolCall,
        VecEventStream,
    };

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
}
