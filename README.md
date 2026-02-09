# Fiddlesticks

Fiddlesticks is a Rust workspace for building provider-agnostic chat and agent runtimes.

The core architecture is spread across focused crates, with `fiddlesticks` as the semver-stable API layer and recommended single dependency for application code.

## Crates

- fiddlesticks (semver-stable facade API layer over all workspace crates)
- fcommon (shared primitives)
- fprovider (model/provider abstraction + adapters)
- ftooling (tool registration + execution runtime)
- fchat (chat turn orchestration)
- fmemory (harness state + transcript persistence)
- fharness (initializer + task-iteration orchestration)
- fobserve (observability hooks for provider/tool/harness phases)

## Recommended Dependency

Use the facade crate unless you intentionally want direct low-level crate control.

```toml
[dependencies]
fiddlesticks = { path = "./fiddlesticks", features = ["provider-openai"] }
```

## Crates and API Usage

### `fiddlesticks` (Semver-stable API layer)

Semver-stable entrypoint with facade-owned namespaces and convenience helpers/macros.

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

### `fobserve` (Observability hooks)

Production-safe tracing and metrics hooks for provider operations, tool execution, and harness phase lifecycle events.

```rust
use fobserve::{MetricsObservabilityHooks, SafeProviderHooks};

let hooks = SafeProviderHooks::new(MetricsObservabilityHooks);
let _ = hooks;
```

Docs: `fobserve/README.md`

## Facade Features

`fiddlesticks` exposes provider features so consumers can configure adapters in one place:

- `provider-openai`
- `provider-opencode-zen`
- `provider-anthropic`
- Prefer stable namespaces: `fiddlesticks::chat`, `fiddlesticks::harness`, `fiddlesticks::memory`,
`fiddlesticks::provider`, and `fiddlesticks::tooling`.

## Development

Run from workspace root:

```bash
cargo fmt --all
cargo check --workspace --all-features
cargo test --workspace --all-features
```

## Security Posture

- API keys are handled in-memory during runtime operations and are not persisted by the framework by default.
- Secure storage, rotation, and retrieval of API keys (for example via a secret manager, environment policy, or KMS-backed workflow) is the responsibility of teams using this framework.

## MSRV and Compatibility Policy

- Minimum Supported Rust Version (MSRV): `1.93.0`.
- `fiddlesticks` is the semver-stable API layer for the workspace; downstream apps should prefer its namespaces and helpers over direct internal crate imports.
- Breaking API changes to `fiddlesticks` only ship in major version releases and are documented in release notes.
- MSRV bumps for `fiddlesticks` are treated as compatibility-impacting changes and only happen in major releases, with migration notes when needed.

## Release Process

- `fiddlesticks` follows strict semver for public API compatibility: breaking changes are only released in new major versions.
- Secondary crates in this workspace (`fcommon`, `fprovider`, `ftooling`, `fchat`, `fmemory`, `fharness`, `fobserve`) are internal building blocks and may receive breaking changes at any time.
- Application and downstream integration code should depend on `fiddlesticks` as the stable boundary and avoid direct coupling to secondary crates unless intentionally opting into unstable internals.
- Branch naming and flow for minor release development:
  - Use `feature/*` and `fix/*` branch names where practical.
  - Merge feature and fix branches into the `prerelease` branch.
  - The `prerelease` branch is the integration branch for the next minor version.
  - When the next minor release is ready, release from the `prerelease` branch.
