# Agent Harness API

`fharness` is the top-level orchestration layer for long-running agent workflows in Fiddlesticks.

It currently supports:

- initializer flow
- task agent incremental loop
- runtime wiring + run-level policy
- reliability + guardrails (MVP hardening)

`fharness` composes lower layers (`fmemory`, `fchat`, `ftooling`, `fprovider`) into a structured multi-run harness.

## Responsibilities

- Run initializer setup for a session (manifest + feature list + progress + checkpoint)
- Run incremental task iterations one feature at a time
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

- `Harness`: orchestrator for initializer and task iterations
- `HarnessBuilder`: runtime wiring for provider/chat/tooling/memory
- `InitializerRequest` / `InitializerResult`
- `TaskIterationRequest` / `TaskIterationResult`
- `RuntimeRunRequest` / `RuntimeRunOutcome`
- `HealthChecker` (`NoopHealthChecker` default)
- `OutcomeValidator` (`AcceptAllValidator` default)
- `FeatureSelector` (`FirstPendingFeatureSelector` default)
- `HarnessError` / `HarnessErrorKind`

## Initializer flow

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

## Task-Iteration incremental loop

`run_task_iteration(...)` executes one bounded run:

1. **Get bearings**
   - loads manifest/progress/features from `fmemory`
   - runs health check using manifest `init_plan`
2. Picks one highest-priority failing feature (`passes == false`)
3. Delegates execution to `fchat` (`run_turn` or `stream_turn`)
4. Validates outcome via `OutcomeValidator`
5. Updates artifacts:
   - marks feature passing only when validated
   - appends progress entry
   - records completed checkpoint with status/note

## Integrated runtime and policy ownership

`HarnessBuilder` wires lower-layer runtime dependencies directly:

- provider (`fprovider`)
- chat service (`fchat`) with transcript storage from `fmemory`
- tool runtime (`ftooling`)
- memory backend (`fmemory`)

`fchat` stays responsible for turn orchestration. `fharness` owns run-level policy:

- phase selection (`initializer` vs `task-iteration`)
- feature selection strategy (`FeatureSelector`)
- validation gate before marking feature pass (`OutcomeValidator`)

## Reliability + guardrails

Harness run policy now supports reliability constraints:

- `max_turns_per_run`
- `max_features_per_run = 1` (strict incremental)
- `retry_budget`
- fail-fast conditions (`health check`, `chat`, `validation`)

Completion guardrail:

- harness does not declare done early
- completion requires all required features in `feature_list` to have `passes = true`

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
    RuntimeRunOutcome::TaskIteration(result) => {
        // one task iteration completed
        assert!(result.validated);
    }
}
```

```rust
use std::sync::Arc;

use fharness::{Harness, TaskIterationRequest};

let harness = Harness::new(memory).with_chat(chat_service);
let request = TaskIterationRequest::new(session, "run-task-1");
let _result = harness.run_task_iteration(request).await?;
```

## Extensibility hooks

- `HealthChecker`
  - run startup/baseline checks before task-iteration work
- `OutcomeValidator`
  - enforce real validation gates before marking features as passing

Use `with_health_checker(...)` and `with_validator(...)` to override defaults.

## Clean handoff guarantees

Every task-iteration run records terminal outcome artifacts:

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
