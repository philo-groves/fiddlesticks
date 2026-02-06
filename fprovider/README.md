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