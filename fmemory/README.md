# Context Layer API

`fmemory` provides durable state and transcript persistence for Fiddlesticks.

It is the persistence layer used by `fharness` for initializer/coding-run artifacts and by `fchat` (through an adapter) for conversation history.

## Responsibilities

- Persist session bootstrap artifacts (manifest, feature list, progress, run checkpoints)
- Persist transcript messages
- Expose a `MemoryBackend` contract for harness logic
- Adapt memory transcript storage to `fchat::ConversationStore`

`fmemory` does **not**:

- Execute model calls (`fprovider`)
- Orchestrate turns (`fchat`)
- Execute tools (`ftooling`)
- Decide multi-run harness strategy (`fharness`)

## Add dependency

```toml
[dependencies]
fmemory = { path = "../fmemory" }
```

## Core types

- `MemoryBackend`: async persistence trait
- `SqliteMemoryBackend`: default durable backend implementation
- `InMemoryMemoryBackend`: ephemeral test-oriented backend implementation
- `MemoryBackendConfig`: backend selection configuration
- `MemoryConversationStore`: adapter implementing `fchat::ConversationStore`
- `SessionManifest`: harness session metadata (+ schema/harness versions)
- `FeatureRecord`: feature checklist item
- `ProgressEntry`: per-run progress log entry
- `RunCheckpoint`: run lifecycle status record
- `BootstrapState`: manifest + feature/progress/checkpoint aggregate

## Session initialization guards

`MemoryBackend` includes explicit initializer-safe methods:

- `is_initialized(session_id)`
- `initialize_session_if_missing(...)`

`initialize_session_if_missing(...)` is idempotent:

- returns `true` when it creates state for the first time
- returns `false` when state already exists (no overwrite)

## Basic backend usage

```rust
use fcommon::SessionId;
use fmemory::prelude::*;

async fn seed_backend(backend: &dyn MemoryBackend) -> Result<(), MemoryError> {
    let session = SessionId::from("session-1");

    let created = backend
        .initialize_session_if_missing(
            &session,
            SessionManifest::new(session.clone(), "feature/init", "Initialize harness"),
            vec![FeatureRecord {
                id: "feature-1".to_string(),
                category: "functional".to_string(),
                description: "Initializer artifacts exist".to_string(),
                steps: vec!["write init state".to_string()],
                passes: false,
            }],
            Some(ProgressEntry::new("run-1", "Initializer scaffold created")),
            Some(RunCheckpoint::started("run-1")),
        )
        .await?;

    let _ = created;
    Ok(())
}
```

## Backend selection

`fmemory` supports configurable backend construction:

```rust
use fmemory::{MemoryBackendConfig, create_default_memory_backend, create_memory_backend};

let sqlite_default = create_default_memory_backend()?;
let in_memory = create_memory_backend(MemoryBackendConfig::InMemory)?;
let sqlite_custom = create_memory_backend(MemoryBackendConfig::Sqlite {
    path: "./state/fmemory.sqlite3".into(),
})?;

let _ = (sqlite_default, in_memory, sqlite_custom);
# Ok::<(), fmemory::MemoryError>(())
```

Default backend path is `~/.fiddlesticks/fmemory.sqlite3` (or `%USERPROFILE%/.fiddlesticks/fmemory.sqlite3` on Windows). Override it with `FMEMORY_SQLITE_PATH`.

## `fchat` integration adapter

`MemoryConversationStore` lets `fchat` use `fmemory` without direct store duplication:

```rust
use std::sync::Arc;

use fmemory::prelude::*;

let backend: Arc<dyn MemoryBackend> = Arc::new(InMemoryMemoryBackend::new());
let store = MemoryConversationStore::new(backend);
let _ = store;
```

## Versioning fields

`SessionManifest` includes:

- `schema_version` (default: `1`)
- `harness_version` (default: `"v0"`)

These support forward migration for future harness behaviors.

## Error model

`MemoryErrorKind` variants:

- `Storage`
- `NotFound`
- `InvalidRequest`
- `Other`
