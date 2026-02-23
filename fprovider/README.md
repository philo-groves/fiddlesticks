# Model Provider API

`fprovider` defines the **model provider abstraction** for Fiddlesticks.

Its job is simple:
> Provide a clean, provider-agnostic way to talk to language models.

Everything else in the system (chat, agents, tools) depends on *this* layer instead of directly coupling to OpenAI, Anthropic, or anything else.

---

## What lives here

- Core **provider traits**
- Provider-agnostic request / response types
- Streaming abstractions (tokens, tool calls, events)
- Provider-specific adapters (behind features)

This crate does **not**:
- Define agent logic
- Define conversation state machines
- Execute tools
- Manage memory or persistence

Those concerns live higher up the stack.

---

## Supported Providers

The currently supported providers are:

- **OpenCode Zen**
- **OpenAI**
- **Anthropic**
- **Ollama**

Each provider implements the same core traits so they can be swapped without changing agent or chat logic.

---

## Design Goals

- **Minimal surface area** – only what every provider must support
- **Async-first** – providers are expected to be network-bound
- **Streaming-friendly** – even if some providers start non-streaming
- **Feature-gated implementations** – avoid pulling heavy deps unless needed
- **No provider leakage** – downstream crates should not need provider-specific types

---

## High-Level Flow

```text
fharness / fchat
        |
        v
    fprovider (traits + adapters)
        |
        v
   External model APIs
```

---

## Using `fprovider` from other crates

### 1) Add dependency

Provider-agnostic usage (recommended default):

```toml
[dependencies]
fprovider = { path = "../fprovider" }
```

If your crate needs OpenAI adapter support, enable the feature:

```toml
[dependencies]
fprovider = { path = "../fprovider", features = ["provider-openai"] }
```

### 2) Build requests with provider-agnostic types

```rust
use fprovider::{Message, ModelRequest, Role};

let request = ModelRequest::builder("gpt-4o-mini")
    .message(Message::new(Role::User, "Summarize this file"))
    .temperature(0.2)
    .max_tokens(512)
    .build()?;
```

### 3) Depend on traits, not SDK types

Higher crates should accept `dyn ModelProvider` so provider choice is runtime-configurable:

```rust
use std::sync::Arc;
use fprovider::{ModelProvider, ModelRequest, ProviderError};

pub async fn run_once(
    provider: Arc<dyn ModelProvider>,
    request: ModelRequest,
) -> Result<(), ProviderError> {
    let response = provider.complete(request).await?;
    let _ = response;
    Ok(())
}
```

### 4) Register and resolve providers

```rust
use fprovider::{ProviderId, ProviderRegistry};

let mut registry = ProviderRegistry::new();
// registry.register(openai_provider);

let provider = registry
    .get(ProviderId::OpenAi)
    .expect("OpenAI provider is not registered");
```

### 5) OpenAI adapter example

```rust
use std::sync::Arc;
use reqwest::Client;
use fprovider::{ProviderRegistry, SecureCredentialManager};
use fprovider::adapters::openai::{OpenAiHttpTransport, OpenAiProvider};

let credentials = Arc::new(SecureCredentialManager::new());
credentials.set_openai_api_key("sk-...")?;

let transport = Arc::new(OpenAiHttpTransport::new(Client::new()));
let openai = OpenAiProvider::new(credentials, transport);

let mut registry = ProviderRegistry::new();
registry.register(openai);
```

### 6) Streaming consumption

`stream(...)` returns a stream implementing `futures_core::Stream<Item = Result<StreamEvent, ProviderError>>`.
This is provider-agnostic and works with standard async ecosystem helpers.

Stream invariants:

- Events are emitted in provider/source order.
- Delta events (`TextDelta`, `ToolCallDelta`) can appear zero or more times.
- Completion milestones (`MessageComplete`, `ResponseComplete`) when present arrive after deltas.
- Once the stream returns `None`, no additional events are emitted.

```rust
use futures_util::StreamExt;
use fprovider::prelude::*;

let mut events = provider.stream(request).await?;
while let Some(event) = events.next().await {
    match event? {
        StreamEvent::TextDelta(delta) => {
            let _ = delta;
        }
        StreamEvent::ToolCallDelta(_) => {}
        StreamEvent::MessageComplete(_) => {}
        StreamEvent::ResponseComplete(_) => {}
    }
}
```

### 7) OpenAI auth policy

When `provider-openai` is enabled, `OpenAiProvider` only uses API key credentials configured via `SecureCredentialManager::set_openai_api_key`.

### 8) Credential lifecycle and auditing

`SecureCredentialManager` now supports lifecycle metadata and access auditing hooks:

- `set_api_key_with_ttl(...)` to expire API keys after a fixed TTL
- `rotate_api_key(...)` and `revoke(...)` for explicit key rotation/revocation
- `credential_metadata(...)` for sanitized metadata (`created_at`, `expires_at`, `last_used_at`, access counters)
- `with_observer(...)` for audit events that include provider/kind/action and never include secret values

### 9) Standard retry/backoff and operational hooks

`fprovider` exposes provider-agnostic resilience primitives:

- `RetryPolicy`: standardized retry attempt limits and exponential backoff settings
- `ProviderOperationHooks`: lifecycle hooks for attempts, retries, success, and failure
- `execute_with_retry(...)`: helper that applies policy + hooks around async operations

Example:

```rust
use std::time::Duration;
use fprovider::prelude::*;

let policy = RetryPolicy {
    max_attempts: 4,
    initial_backoff: Duration::from_millis(100),
    max_backoff: Duration::from_secs(2),
    backoff_multiplier: 2.0,
};

let hooks = NoopOperationHooks;

let value = execute_with_retry(
    ProviderId::OpenAi,
    "complete",
    &policy,
    &hooks,
    |_attempt| async { Ok::<_, ProviderError>("ok") },
    |_delay| async {},
)
.await?;

let _ = value;
```

---

## Feature flags

- `provider-openai`: OpenAI adapter and HTTP transport
- `provider-anthropic`: Anthropic adapter over OpenAI-compatible transport
- `provider-opencode-zen`: OpenCode Zen adapter over OpenAI-compatible transport
- `provider-ollama`: Ollama adapter over OpenAI-compatible transport
