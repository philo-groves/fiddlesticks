# Conversational API

`fchat` is the conversation orchestration layer for Fiddlesticks.

It sits above `fprovider` and is responsible for handling chat turns, session history loading/saving, and assembling provider requests from conversational state.

It can also integrate with `ftooling` for provider tool-call execution loops.

## Responsibilities

- Own chat-session and turn request/response types
- Load prior transcript messages from a conversation store
- Build and execute provider requests through `fprovider::ModelProvider`
- Persist new user/assistant transcript messages

`fchat` does **not**:

- Implement model-provider transports (that belongs to `fprovider`)
- Execute tools (that belongs to `ftooling`)
- Define memory retrieval/summarization engines (that belongs to `fmemory`)

## Current implementation scope

The implementation currently supports:

- Non-streaming turn execution via `ChatService::run_turn(...)`
- Live streaming turn execution via `ChatService::stream_turn(...)`
- Session-level system prompt injection
- In-memory transcript storage implementation for local use/tests
- Optional tool-call execution loop via `ftooling::ToolRuntime`

## Add dependency

```toml
[dependencies]
fchat = { path = "../fchat" }
ftooling = { path = "../ftooling" }
fprovider = { path = "../fprovider", features = ["provider-openai"] }
```

## Basic usage

```rust
use std::sync::Arc;

use fchat::prelude::*;
use fprovider::ProviderId;

async fn run_chat(provider: Arc<dyn fprovider::ModelProvider>) -> Result<(), ChatError> {
    let chat = ChatService::builder(provider)
        .default_temperature(Some(0.2))
        .default_max_tokens(Some(400))
        .build();

    let session = ChatSession::new("session-1", ProviderId::OpenAi, "gpt-4o-mini")
        .with_system_prompt("You are concise and helpful.");

    let request = ChatTurnRequest::new(session, "Summarize this repo layout");

    let result = chat.run_turn(request).await?;

    println!("assistant: {}", result.assistant_message);
    Ok(())
}
```

## High-level builders and defaults

`fchat` includes an opinionated builder path so you can configure once and keep turn calls lightweight.

- `ChatService::builder(provider)` defaults to `InMemoryConversationStore`
- `ChatPolicy::default()` is applied unless overridden
- per-turn values are merged with service defaults (`ChatTurnRequest` values win)

```rust
use std::sync::Arc;

use fchat::prelude::*;

fn build_service(provider: Arc<dyn fprovider::ModelProvider>) -> ChatService {
    ChatService::builder(provider)
        .default_temperature(Some(0.3))
        .default_max_tokens(Some(512))
        .max_tool_round_trips(4)
        .build()
}
```

For turn-level overrides, use `ChatTurnOptions`:

```rust
use fchat::prelude::*;

let options = ChatTurnOptions {
    temperature: Some(0.7),
    max_tokens: Some(120),
    stream: false,
};

let request = ChatTurnRequest::new(session, "Explain this quickly")
    .with_options(options);
```

## Streaming usage

```rust
use std::sync::Arc;

use futures_util::StreamExt;
use fchat::prelude::*;
use fprovider::ProviderId;

async fn run_streaming(provider: Arc<dyn fprovider::ModelProvider>) -> Result<(), ChatError> {
    let store = Arc::new(InMemoryConversationStore::new());
    let chat = ChatService::new(provider, store);

    let session = ChatSession::new("session-stream", ProviderId::OpenAi, "gpt-4o-mini");
    let request = ChatTurnRequest::new(session, "Stream this response").enable_streaming();

    let mut events = chat.stream_turn(request).await?;
    while let Some(event) = events.next().await {
        match event? {
            ChatEvent::TextDelta(delta) => {
                println!("delta: {}", delta);
            }
            ChatEvent::ToolCallDelta(_) => {}
            ChatEvent::AssistantMessageComplete(_) => {}
            ChatEvent::TurnComplete(result) => {
                println!("final: {}", result.assistant_message);
            }
        }
    }

    Ok(())
}
```

Current streaming semantics:

- `stream_turn` maps provider stream events into chat-layer events.
- Transcript persistence still occurs before `TurnComplete` is emitted.
- Events are forwarded as they arrive from the provider stream.

## Tool loop usage (`ftooling` integration)

When configured, `ChatService::run_turn(...)` can execute provider tool calls and continue model turns.

```rust
use std::sync::Arc;

use fchat::prelude::*;
use fprovider::ProviderId;
use ftooling::prelude::*;

fn build_chat(provider: Arc<dyn fprovider::ModelProvider>) -> ChatService {
    let mut registry = ToolRegistry::new();
    registry.register_sync_fn(
        fprovider::ToolDefinition {
            name: "echo".to_string(),
            description: "Echo tool".to_string(),
            input_schema: "{\"type\":\"string\"}".to_string(),
        },
        |args, _ctx| Ok(args),
    );

    let runtime = Arc::new(DefaultToolRuntime::new(Arc::new(registry)));
    let store = Arc::new(InMemoryConversationStore::new());

    ChatService::new(provider, store)
        .with_tool_runtime(runtime)
        .with_max_tool_round_trips(4)
}

fn _session() -> ChatSession {
    ChatSession::new("session-tools", ProviderId::OpenAi, "gpt-4o-mini")
}
```

Tool loop semantics:

- Tool execution is only used when both a runtime is configured and `max_tool_round_trips > 0`.
- Each provider `ToolCall` is executed through `ftooling::ToolRuntime`.
- Tool outputs are returned to the provider as `ToolResult` values for follow-up completions.
- Loop stops when no tool calls remain or max round-trips is reached.

## Public API overview

- `ChatService`: turn orchestrator over provider + store
- `ChatSession`: session metadata (`id`, `provider`, `model`, optional `system_prompt`)
- `ChatTurnRequest`: user input + per-turn model params
- `ChatTurnResult`: assistant text + tool calls + stop reason + usage
- `ChatEvent`: streaming event envelope (`TextDelta`, `ToolCallDelta`, `AssistantMessageComplete`, `TurnComplete`)
- `ChatEventStream`: stream alias for chat event consumers
- `ConversationStore`: async conversation history contract
- `InMemoryConversationStore`: default in-crate store implementation
- `with_tool_runtime(...)`: opt-in `ftooling::ToolRuntime` integration
- `with_max_tool_round_trips(...)`: cap recursive tool/model rounds

## Error model

`ChatErrorKind` variants:

- `InvalidRequest`
- `Provider`
- `Store`
- `Tooling`

Provider errors from `fprovider` are mapped into `ChatErrorKind::Provider`.
Tool errors from `ftooling` are mapped into `ChatErrorKind::Tooling`.

`ChatError` also exposes:

- `retryable`: normalized retry hint for higher layers
- `phase`: where the failure occurred (`Provider`, `Tooling`, `Storage`, `Streaming`, etc.)
- `source`: source error kind (`ProviderErrorKind` or `ToolErrorKind`)
- helper methods: `is_retryable()` and `is_user_error()`
