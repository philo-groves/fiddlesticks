# Conversational API

`fchat` is the conversation orchestration layer for Fiddlesticks.

It sits above `fprovider` and is responsible for handling chat turns, session history loading/saving, and assembling provider requests from conversational state.

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

The first implementation slice currently supports:

- Non-streaming turn execution via `ChatService::run_turn(...)`
- Buffered streaming turn execution via `ChatService::stream_turn(...)`
- Session-level system prompt injection
- In-memory transcript storage implementation for local use/tests
- Tool runtime trait and no-op placeholder implementation

Tool-execution loops are planned next.

## Add dependency

```toml
[dependencies]
fchat = { path = "../fchat" }
fprovider = { path = "../fprovider", features = ["provider-openai"] }
```

## Basic usage

```rust
use std::sync::Arc;

use fchat::prelude::*;
use fprovider::ProviderId;

async fn run_chat(provider: Arc<dyn fprovider::ModelProvider>) -> Result<(), ChatError> {
    let store = Arc::new(InMemoryConversationStore::new());
    let chat = ChatService::new(provider, store);

    let session = ChatSession::new("session-1", ProviderId::OpenAi, "gpt-4o-mini")
        .with_system_prompt("You are concise and helpful.");

    let request = ChatTurnRequest::new(session, "Summarize this repo layout")
        .with_temperature(0.2)
        .with_max_tokens(400);

    let result = chat.run_turn(request).await?;

    println!("assistant: {}", result.assistant_message);
    Ok(())
}
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

Current streaming semantics in this slice:

- `stream_turn` maps provider stream events into chat-layer events.
- Transcript persistence still occurs before `TurnComplete` is emitted.
- Events are currently buffered internally before being exposed to callers.

## Public API overview

- `ChatService`: turn orchestrator over provider + store
- `ChatSession`: session metadata (`id`, `provider`, `model`, optional `system_prompt`)
- `ChatTurnRequest`: user input + per-turn model params
- `ChatTurnResult`: assistant text + tool calls + stop reason + usage
- `ChatEvent`: streaming event envelope (`TextDelta`, `ToolCallDelta`, `AssistantMessageComplete`, `TurnComplete`)
- `ChatEventStream`: stream alias for chat event consumers
- `ConversationStore`: async conversation history contract
- `InMemoryConversationStore`: default in-crate store implementation
- `ToolRuntime`: tool execution contract (future tool loop integration)
- `NoopToolRuntime`: placeholder runtime that returns tooling-not-configured

## Error model

`ChatErrorKind` variants:

- `InvalidRequest`
- `Provider`
- `Store`
- `Tooling`

Provider errors from `fprovider` are mapped into `ChatErrorKind::Provider`.
