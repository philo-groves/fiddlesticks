# Observability Hooks API

`fobserve` provides production-oriented observability hooks for Fiddlesticks runtimes.

It offers ready-made `tracing` and `metrics` hook implementations across:

- provider operations (`fprovider`)
- tool execution lifecycle (`ftooling`)
- harness phase lifecycle (`fharness`)

## Responsibilities

- Emit structured tracing events for provider/tool/harness phases
- Emit counters and histograms for operational metrics
- Provide panic-safe wrappers so hook code cannot take down runtime execution

`fobserve` does **not**:

- decide retry/tool/phase policy (`fprovider`, `fchat`, `fharness` own policy)
- ship a metrics exporter or tracing subscriber (application owns wiring)

## Add dependency

```toml
[dependencies]
fobserve = { path = "../fobserve" }
fchat = { path = "../fchat" }
fharness = { path = "../fharness" }
ftooling = { path = "../ftooling" }
```

## Core types

- `TracingObservabilityHooks`
- `MetricsObservabilityHooks`
- `SafeProviderHooks<H>`
- `SafeToolHooks<H>`
- `SafeHarnessHooks<H>`

## Module layout

- `tracing_hooks`: `TracingObservabilityHooks` implementations for provider/tool/harness hooks
- `metrics_hooks`: `MetricsObservabilityHooks` implementations for provider/tool/harness hooks
- `safe_hooks`: panic-isolating wrappers (`SafeProviderHooks`, `SafeToolHooks`, `SafeHarnessHooks`)
- `lib`: crate exports and prelude

## Hook coverage

- Provider: attempt start, retry scheduled, success, failure
- Tool runtime: execution start, success, failure (with elapsed time)
- Harness: phase start, success, failure (initializer/task-iteration)

## Production-safe wrappers

Use `Safe*Hooks` wrappers when you need strong isolation from observer panics.

- `SafeProviderHooks` wraps `ProviderOperationHooks`
- `SafeToolHooks` wraps `ToolRuntimeHooks`
- `SafeHarnessHooks` wraps `HarnessRuntimeHooks`

Each wrapper catches panics in hook callbacks and suppresses them so core runtime flows continue.

## End-to-end wiring example

```rust
use std::sync::Arc;

use fchat::ChatService;
use fharness::Harness;
use fobserve::{
    MetricsObservabilityHooks, SafeHarnessHooks, SafeProviderHooks, SafeToolHooks,
    TracingObservabilityHooks,
};
use ftooling::DefaultToolRuntime;

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
# Ok::<(), fharness::HarnessError>(())
```

## Notes

- `fobserve` intentionally emits generic metric names with labels so apps can map dashboards/alerts per deployment.
- `tracing`/`metrics` output requires app-level subscriber/recorder setup.

## Testing

`fobserve` includes unit tests that:

- smoke-test all tracing and metrics callbacks
- verify safe wrappers delegate when inner hooks succeed
- verify safe wrappers swallow panics from inner hooks
