# Fiddlesticks

Fiddlesticks is a Rust workspace for building provider-agnostic chat and agent runtimes.

The core architecture is now in place across focused crates, with `fiddlesticks` as the recommended single dependency for application code.

## Workspace Architecture

```text
fcommon (shared primitives)
   |
   +--> fprovider (model/provider abstraction + adapters)
   +--> ftooling (tool registration + execution runtime)
            |
            +--> fchat (chat turn orchestration)
            +--> fmemory (harness state + transcript persistence)
                         |
                         +--> fharness (initializer + task-iteration orchestration)

fiddlesticks (facade over all workspace crates)
```

## Recommended Dependency

Use the facade crate unless you intentionally want direct low-level crate control.

```toml
[dependencies]
fiddlesticks = { path = "./fiddlesticks", features = ["provider-openai"] }
```

## Crates and API Usage

### `fiddlesticks` (Facade)

Single entrypoint that re-exports all workspace crates and adds convenience helpers/macros.

```rust
use std::sync::Arc;

use fiddlesticks::prelude::*;

fn _example(provider: Arc<dyn ModelProvider>) -> Result<(), HarnessError> {
    let runtime = build_runtime(provider)?;
    let session = fs_session!("session-1", openai, "gpt-4o-mini");
    let request = turn(session, "Summarize this repository architecture.");
    let _ = (runtime, request);
    Ok(())
}
```

Docs: `fiddlesticks/README.md`

### `fcommon` (Shared primitives)

Cross-crate IDs, metadata, and async future aliases.

```rust
use fcommon::{MetadataMap, SessionId, TraceId};

let session_id = SessionId::from("session-1");
let trace_id = TraceId::new("trace-abc");

let mut metadata = MetadataMap::new();
metadata.insert("env".to_string(), "dev".to_string());

let _ = (session_id, trace_id, metadata);
```

Docs: `fcommon/README.md`

### `fprovider` (Model provider abstraction)

Provider-agnostic request/response model, streaming contracts, credential helpers, registry, retries, and feature-gated adapters.

```rust
use fprovider::{Message, ModelRequest, Role};

let request = ModelRequest::builder("gpt-4o-mini")
    .message(Message::new(Role::User, "Summarize this diff"))
    .temperature(0.2)
    .max_tokens(256)
    .build()?;

let _ = request;
# Ok::<(), fprovider::ProviderError>(())
```

Docs: `fprovider/README.md`

### `ftooling` (Capability runtime)

Tool registration and execution runtime that consumes `fprovider::ToolCall` values.

```rust
use std::sync::Arc;

use fprovider::ToolDefinition;
use ftooling::prelude::*;

let mut registry = ToolRegistry::new();
registry.register_sync_fn(
    ToolDefinition {
        name: "echo".to_string(),
        description: "Echo input".to_string(),
        input_schema: "{\"type\":\"string\"}".to_string(),
    },
    |args, _ctx| Ok(args),
);

let _runtime = DefaultToolRuntime::new(Arc::new(registry));
```

Docs: `ftooling/README.md`

### `fchat` (Conversation orchestration)

Turn execution (non-streaming + streaming), conversation storage integration, retries, and optional tool loop behavior.

```rust
use fchat::prelude::*;
use fprovider::ProviderId;

let session = ChatSession::new("session-1", ProviderId::OpenAi, "gpt-4o-mini")
    .with_system_prompt("You are concise.");

let request = ChatTurnRequest::builder(session, "Explain the architecture")
    .temperature(0.3)
    .max_tokens(300)
    .build();

let _ = request;
```

Docs: `fchat/README.md`

### `fmemory` (State and transcript persistence)

Session bootstrap state for harness runs plus a transcript adapter implementing `fchat::ConversationStore`.

```rust
use std::sync::Arc;

use fmemory::prelude::*;

let backend: Arc<dyn MemoryBackend> = Arc::new(InMemoryMemoryBackend::new());
let store = MemoryConversationStore::new(backend);
let _ = store;
```

Docs: `fmemory/README.md`

### `fharness` (Run orchestration)

Initializer and incremental coding-run orchestration with health checks, validation gates, and durable handoff artifacts.

```rust
use std::sync::Arc;

use fharness::{Harness, InitializerRequest};
use fmemory::{InMemoryMemoryBackend, MemoryBackend};

let memory: Arc<dyn MemoryBackend> = Arc::new(InMemoryMemoryBackend::new());
let harness = Harness::new(memory);
let request = InitializerRequest::new("session-1", "run-init-1", "Bootstrap harness state");
let _ = (harness, request);
```

Docs: `fharness/README.md`

## Facade Features

`fiddlesticks` exposes provider features so consumers can configure adapters in one place:

- `provider-openai`
- `provider-opencode-zen`
- `provider-claude`

## Development

Run from workspace root:

```bash
cargo fmt --all
cargo check --workspace --all-features
cargo test --workspace --all-features
```
