# Capability Layer API

`ftooling` provides the tool registration and execution runtime for Fiddlesticks.

It is designed to plug into `fchat` tool loops while staying provider-agnostic by using `fprovider` tool contracts.

## Responsibilities

- Register tools and expose their `ToolDefinition` metadata
- Execute tool calls from model output (`fprovider::ToolCall`)
- Return tool outputs as structured execution results
- Offer runtime hooks and timeout controls for observability and resilience

`ftooling` does **not**:

- Decide when to call tools during a conversation (`fchat` owns that)
- Persist conversation state (`fmemory` owns that)
- Implement model transport/provider adapters (`fprovider` owns that)

## Add dependency

```toml
[dependencies]
ftooling = { path = "../ftooling" }
fprovider = { path = "../fprovider" }
```

## Core types

- `Tool`: trait for executable capabilities
- `ToolRegistry`: registry keyed by tool name
- `ToolRuntime`: runtime contract for tool execution
- `DefaultToolRuntime`: registry-backed runtime implementation
- `ToolExecutionContext`: session/trace metadata passed to tools
- `ToolExecutionResult`: normalized output payload
- `ToolError`: typed error with retryability and optional call metadata

## Easiest registration path

You can register tools without implementing a custom struct.

### Sync closure

```rust
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
```

### Async closure

```rust
use fprovider::ToolDefinition;
use ftooling::prelude::*;

let mut registry = ToolRegistry::new();
registry.register_fn(
    ToolDefinition {
        name: "uppercase".to_string(),
        description: "Uppercase input".to_string(),
        input_schema: "{\"type\":\"string\"}".to_string(),
    },
    |args, _ctx| async move { Ok(args.to_uppercase()) },
);
```

## Runtime usage

```rust
use std::sync::Arc;

use fprovider::ToolCall;
use ftooling::prelude::*;

async fn run_once(registry: ToolRegistry) -> Result<ToolExecutionResult, ToolError> {
    let runtime = DefaultToolRuntime::new(Arc::new(registry))
        .with_timeout(std::time::Duration::from_secs(2));

    runtime
        .execute(
            ToolCall {
                id: "call_1".to_string(),
                name: "echo".to_string(),
                arguments: "hello".to_string(),
            },
            ToolExecutionContext::new("session-1").with_trace_id("trace-abc"),
        )
        .await
}
```

## Hooks and timeout

- `DefaultToolRuntime::with_hooks(...)` attaches runtime lifecycle hooks
- `DefaultToolRuntime::with_timeout(...)` enforces per-call timeout
- Hook events include start/success/failure with elapsed duration

```rust
use std::sync::Arc;
use std::time::Duration;

use ftooling::prelude::*;

let runtime = DefaultToolRuntime::new(Arc::new(ToolRegistry::new()))
    .with_hooks(Arc::new(NoopToolRuntimeHooks))
    .with_timeout(Duration::from_millis(500));

let _ = runtime;
```

## Error model

`ToolErrorKind` variants:

- `NotFound`
- `InvalidArguments`
- `Execution`
- `Timeout`
- `Unauthorized`
- `Other`

`ToolError` includes:

- `retryable` for policy decisions
- optional `tool_name` and `tool_call_id` for richer context
- helper methods: `is_retryable()`, `is_user_error()`

## Integration with `fchat`

`fchat` can consume `ftooling::ToolRuntime` directly:

- configure on `ChatService` via `.with_tool_runtime(...)`
- cap loops with `.with_max_tool_round_trips(...)`
- `fchat` maps `ToolError` to `ChatErrorKind::Tooling`
