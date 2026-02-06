# Agent Harness API

`fharness` is the top-level orchestration layer for long-running agent workflows in Fiddlesticks.

It currently supports:

- Phase 2: initializer flow
- Phase 3: coding agent incremental loop
- Phase 4: runtime wiring + run-level policy

`fharness` composes lower layers (`fmemory`, `fchat`, `ftooling`, `fprovider`) into a structured multi-run harness.

## Responsibilities

- Run initializer setup for a session (manifest + feature list + progress + checkpoint)
- Run incremental coding iterations one feature at a time
- Enforce clean handoff by recording explicit run outcomes
- Coordinate health checks, execution, validation, and persistence updates

`fharness` does **not**:

- Implement provider transports (`fprovider`)
- Implement turn orchestration internals (`fchat`)
- Implement tool runtimes (`ftooling`)
- Implement persistence internals (`fmemory`)

## Add dependency

```toml
[dependencies]
fharness = { path = "../fharness" }
fmemory = { path = "../fmemory" }
fchat = { path = "../fchat" }
```

## Core types

- `Harness`: orchestrator for initializer and coding iterations
- `HarnessBuilder`: runtime wiring for provider/chat/tooling/memory
- `InitializerRequest` / `InitializerResult`
- `CodingRunRequest` / `CodingRunResult`
- `RuntimeRunRequest` / `RuntimeRunOutcome`
- `HealthChecker` (`NoopHealthChecker` default)
- `OutcomeValidator` (`AcceptAllValidator` default)
- `FeatureSelector` (`FirstPendingFeatureSelector` default)
- `HarnessError` / `HarnessErrorKind`

## Phase 2: Initializer flow

`run_initializer(...)`:

1. Validates objective and feature list rules
2. Builds/versions `SessionManifest`
3. Uses `initialize_session_if_missing(...)` for idempotent initialization
4. Persists initial progress + run checkpoint

If no feature list is provided, `Harness` generates a starter skeleton via `starter_feature_list(...)`.

```rust
use std::sync::Arc;

use fharness::{Harness, InitializerRequest};
use fmemory::{InMemoryMemoryBackend, MemoryBackend};

let memory: Arc<dyn MemoryBackend> = Arc::new(InMemoryMemoryBackend::new());
let harness = Harness::new(memory);

let request = InitializerRequest::new("session-1", "run-init-1", "Build incremental harness");
let _result = harness.run_initializer(request).await?;
```

## Phase 3: Coding incremental loop

`run_coding_iteration(...)` executes one bounded run:

1. **Get bearings**
   - loads manifest/progress/features from `fmemory`
   - runs health check using manifest `init_script`
2. Picks one highest-priority failing feature (`passes == false`)
3. Delegates execution to `fchat` (`run_turn` or `stream_turn`)
4. Validates outcome via `OutcomeValidator`
5. Updates artifacts:
   - marks feature passing only when validated
   - appends progress entry
   - records completed checkpoint with status/note

## Phase 4: Integrated runtime and policy ownership

`HarnessBuilder` wires lower-layer runtime dependencies directly:

- provider (`fprovider`)
- chat service (`fchat`) with transcript storage from `fmemory`
- tool runtime (`ftooling`)
- memory backend (`fmemory`)

`fchat` stays responsible for turn orchestration. `fharness` owns run-level policy:

- phase selection (`initializer` vs `coding`)
- feature selection strategy (`FeatureSelector`)
- validation gate before marking feature pass (`OutcomeValidator`)

```rust
use std::sync::Arc;

use fharness::{Harness, RuntimeRunRequest, RuntimeRunOutcome};
use fmemory::{InMemoryMemoryBackend, MemoryBackend};

let memory: Arc<dyn MemoryBackend> = Arc::new(InMemoryMemoryBackend::new());
let harness = Harness::builder(memory)
    .provider(provider)
    .tool_runtime(tool_runtime)
    .build()?;

match harness.run(RuntimeRunRequest::new(session, "run-1", "Build feature loop")).await? {
    RuntimeRunOutcome::Initializer(_) => {
        // session bootstrapped
    }
    RuntimeRunOutcome::Coding(result) => {
        // one coding iteration completed
        assert!(result.validated);
    }
}
```

```rust
use std::sync::Arc;

use fharness::{CodingRunRequest, Harness};

let harness = Harness::new(memory).with_chat(chat_service);
let request = CodingRunRequest::new(session, "run-code-1");
let _result = harness.run_coding_iteration(request).await?;
```

## Extensibility hooks

- `HealthChecker`
  - run startup/baseline checks before coding work
- `OutcomeValidator`
  - enforce real validation gates before marking features as passing

Use `with_health_checker(...)` and `with_validator(...)` to override defaults.

## Clean handoff guarantees

Every coding run records terminal outcome artifacts:

- run checkpoint with explicit `status` (`Succeeded`/`Failed`) and note
- progress entry summarizing what happened

This avoids ambiguous handoff state across context windows.

## Error model

`HarnessErrorKind` variants:

- `InvalidRequest`
- `Memory`
- `Chat`
- `Validation`
- `HealthCheck`
- `NotReady`
