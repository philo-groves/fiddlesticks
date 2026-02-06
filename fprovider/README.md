# Model Provider API

`fprovider` defines the **model provider abstraction** for Fiddlesticks.

Its job is simple:
> Provide a clean, provider-agnostic way to talk to language models.

Everything else in the system (chat, agents, tools) depends on *this* layer instead of directly coupling to OpenAI, Claude, or anything else.

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
- **Claude (Anthropic)**

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

let request = ModelRequest::new(
    "gpt-4o-mini",
    vec![Message::new(Role::User, "Summarize this file")],
)
.with_temperature(0.2)
.with_max_tokens(512);

request.validate()?;
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

`stream(...)` returns provider-agnostic `StreamEvent` values (`TextDelta`, `ToolCallDelta`, `MessageComplete`, `ResponseComplete`), so higher crates can handle streaming once and reuse it across providers.

---

## Feature flags

- `provider-openai`: OpenAI adapter and HTTP transport
- `provider-claude`: Claude adapter surface (in progress)
- `provider-opencode-zen`: OpenCode Zen adapter surface (in progress)
