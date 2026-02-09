# Fiddlesticks Facade

`fiddlesticks` is the single-dependency gateway for the Fiddlesticks workspace.

It exposes a facade-owned, semver-stable API surface for common runtime wiring,
provider setup, and request-building flows.

## Add dependency

```toml
[dependencies]
fiddlesticks = { path = "../fiddlesticks" }
```

Enable provider adapters from one place:

```toml
[dependencies]
fiddlesticks = { path = "../fiddlesticks", features = ["provider-opencode-zen"] }
```

## API surface

- Stable namespace modules: `fiddlesticks::chat`, `fiddlesticks::harness`, `fiddlesticks::memory`, `fiddlesticks::provider`, `fiddlesticks::tooling`
- Dynamic harness builder: `AgentHarnessBuilder`
- Provider setup utilities: `build_provider_from_api_key`, `build_provider_with_config`, `list_models_with_api_key`
- Curated top-level exports for common types (`ChatService`, `Harness`, `ModelProvider`, `ToolRegistry`, ...)
- `prelude` module for ergonomic imports
- Runtime helpers: `build_runtime*`, `chat_service*`, `in_memory_backend`
- Utility constructors: message/session/turn helpers
- Macros: `fs_msg!`, `fs_messages!`, `fs_session!`

## Basic usage

```rust
use std::sync::Arc;

use fiddlesticks::prelude::*;

fn _example(provider: Arc<dyn ModelProvider>) -> Result<(), HarnessError> {
    let runtime = build_runtime(provider)?;

    let session = fs_session!(
        "session-1",
        openai,
        "gpt-4o-mini",
        "You are concise and practical."
    );

    let request = turn(session, "Summarize this repository architecture");
    let _ = (runtime, request);
    Ok(())
}
```

## Runtime wiring helpers

```rust
use std::sync::Arc;

use fiddlesticks::prelude::*;

fn _wire(provider: Arc<dyn ModelProvider>, tool_runtime: Arc<dyn ToolRuntime>) -> Result<(), HarnessError> {
    let runtime = build_runtime_with_tooling(provider, tool_runtime)?;
    let _chat = runtime.chat;
    let _harness = runtime.harness;
    Ok(())
}
```

## Observability integration

`fiddlesticks` exposes the runtime hook traits (`ProviderOperationHooks`, `ToolRuntimeHooks`, and `HarnessRuntimeHooks`) via its facade API. For ready-made tracing/metrics implementations, add `fobserve` alongside `fiddlesticks`.

```toml
[dependencies]
fiddlesticks = { path = "../fiddlesticks" }
fobserve = { path = "../fobserve" }
```

```rust
use std::sync::Arc;

use fiddlesticks::prelude::*;
use fobserve::{
    MetricsObservabilityHooks, SafeHarnessHooks, SafeProviderHooks, SafeToolHooks,
    TracingObservabilityHooks,
};

fn _with_observability(
    provider: Arc<dyn ModelProvider>,
    tool_registry: Arc<ToolRegistry>,
    memory: Arc<dyn MemoryBackend>,
) -> Result<(), HarnessError> {
    let provider_hooks = Arc::new(SafeProviderHooks::new(TracingObservabilityHooks));
    let tool_hooks = Arc::new(SafeToolHooks::new(MetricsObservabilityHooks));
    let harness_hooks = Arc::new(SafeHarnessHooks::new(TracingObservabilityHooks));

    let tool_runtime = DefaultToolRuntime::new(tool_registry).with_hooks(tool_hooks);

    let chat = ChatService::builder(Arc::clone(&provider))
        .provider_operation_hooks(provider_hooks)
        .tool_runtime(Arc::new(tool_runtime))
        .build();

    let harness = Harness::builder(memory)
        .provider(provider)
        .hooks(harness_hooks)
        .build()?;

    let _ = (chat, harness);
    Ok(())
}
```

## Utility helpers and macros

```rust
use fiddlesticks::prelude::*;

let messages = fs_messages![
    system => "You are a strict reviewer.",
    user => "Review this patch.",
];

let user = user_message("hello");
let _ = (messages, user);
```
