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
