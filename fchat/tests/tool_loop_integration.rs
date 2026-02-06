use std::sync::Arc;

use fchat::prelude::*;
use fprovider::{
    Message, ModelProvider, ModelRequest, ModelResponse, OutputItem, ProviderError, ProviderFuture,
    ProviderId, Role, StopReason, StreamEvent, TokenUsage, ToolCall, VecEventStream,
};
use futures_util::StreamExt;

#[derive(Debug)]
struct ToolLoopProvider;

impl ModelProvider for ToolLoopProvider {
    fn id(&self) -> ProviderId {
        ProviderId::OpenAi
    }

    fn complete<'a>(
        &'a self,
        request: ModelRequest,
    ) -> ProviderFuture<'a, Result<ModelResponse, ProviderError>> {
        Box::pin(async move {
            if request.tool_results.is_empty() {
                return Ok(ModelResponse {
                    provider: ProviderId::OpenAi,
                    model: request.model,
                    output: vec![OutputItem::ToolCall(ToolCall {
                        id: "tool_call_1".to_string(),
                        name: "echo".to_string(),
                        arguments: "{\"text\":\"hello\"}".to_string(),
                    })],
                    stop_reason: StopReason::ToolUse,
                    usage: TokenUsage {
                        input_tokens: 5,
                        output_tokens: 2,
                        total_tokens: 7,
                    },
                });
            }

            Ok(ModelResponse {
                provider: ProviderId::OpenAi,
                model: request.model,
                output: vec![OutputItem::Message(Message::new(Role::Assistant, "done"))],
                stop_reason: StopReason::EndTurn,
                usage: TokenUsage {
                    input_tokens: 6,
                    output_tokens: 2,
                    total_tokens: 8,
                },
            })
        })
    }

    fn stream<'a>(
        &'a self,
        request: ModelRequest,
    ) -> ProviderFuture<'a, Result<fprovider::BoxedEventStream<'a>, ProviderError>> {
        Box::pin(async move {
            if request.tool_results.is_empty() {
                let response = ModelResponse {
                    provider: ProviderId::OpenAi,
                    model: request.model,
                    output: vec![OutputItem::ToolCall(ToolCall {
                        id: "tool_call_1".to_string(),
                        name: "echo".to_string(),
                        arguments: "{\"text\":\"hello\"}".to_string(),
                    })],
                    stop_reason: StopReason::ToolUse,
                    usage: TokenUsage {
                        input_tokens: 5,
                        output_tokens: 2,
                        total_tokens: 7,
                    },
                };

                let stream = VecEventStream::new(vec![
                    Ok(StreamEvent::ToolCallDelta(ToolCall {
                        id: "tool_call_1".to_string(),
                        name: "echo".to_string(),
                        arguments: "{\"text\":\"hello\"}".to_string(),
                    })),
                    Ok(StreamEvent::ResponseComplete(response)),
                ]);

                return Ok(Box::pin(stream) as fprovider::BoxedEventStream<'a>);
            }

            let response = ModelResponse {
                provider: ProviderId::OpenAi,
                model: request.model,
                output: vec![OutputItem::Message(Message::new(Role::Assistant, "done"))],
                stop_reason: StopReason::EndTurn,
                usage: TokenUsage {
                    input_tokens: 6,
                    output_tokens: 2,
                    total_tokens: 8,
                },
            };

            let stream = VecEventStream::new(vec![
                Ok(StreamEvent::TextDelta("done".to_string())),
                Ok(StreamEvent::ResponseComplete(response)),
            ]);

            Ok(Box::pin(stream) as fprovider::BoxedEventStream<'a>)
        })
    }
}

#[tokio::test]
async fn chat_tool_loop_executes_registered_tool_and_completes_turn() {
    let provider = Arc::new(ToolLoopProvider);

    let mut registry = ToolRegistry::new();
    registry.register_sync_fn(
        fprovider::ToolDefinition {
            name: "echo".to_string(),
            description: "Echoes text".to_string(),
            input_schema: "{\"type\":\"object\"}".to_string(),
        },
        |args, _ctx| Ok(args),
    );

    let runtime = Arc::new(DefaultToolRuntime::new(Arc::new(registry)));
    let service = ChatService::builder(provider)
        .tool_runtime(runtime)
        .max_tool_round_trips(2)
        .build();

    let session = ChatSession::new("int-s1", ProviderId::OpenAi, "gpt-4o-mini");
    let result = service
        .run_turn(ChatTurnRequest::new(session, "go"))
        .await
        .expect("turn should succeed");

    assert_eq!(result.assistant_message, "done");
    assert!(!result.tool_round_limit_reached);
}

#[tokio::test]
async fn chat_stream_tool_loop_surfaces_tooling_error_context() {
    let provider = Arc::new(ToolLoopProvider);

    let mut registry = ToolRegistry::new();
    registry.register_sync_fn(
        fprovider::ToolDefinition {
            name: "echo".to_string(),
            description: "Always fails".to_string(),
            input_schema: "{\"type\":\"object\"}".to_string(),
        },
        |_args, _ctx| Err(ToolError::invalid_arguments("bad tool input")),
    );

    let runtime = Arc::new(DefaultToolRuntime::new(Arc::new(registry)));
    let service = ChatService::builder(provider)
        .tool_runtime(runtime)
        .max_tool_round_trips(2)
        .build();

    let session = ChatSession::new("int-s2", ProviderId::OpenAi, "gpt-4o-mini");
    let mut stream = service
        .stream_turn(ChatTurnRequest::new(session, "go").enable_streaming())
        .await
        .expect("stream should start");

    while let Some(item) = stream.next().await {
        match item {
            Ok(_) => continue,
            Err(err) => {
                assert_eq!(err.kind, ChatErrorKind::Tooling);
                assert_eq!(err.phase, Some(ChatErrorPhase::Tooling));
                assert!(err.is_user_error());
                return;
            }
        }
    }

    panic!("expected tooling failure from stream");
}
