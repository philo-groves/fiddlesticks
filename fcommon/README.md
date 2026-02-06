# Common API

`fcommon` provides the small shared primitives used across all Fiddlesticks crates.

It intentionally stays minimal so higher layers can share identifiers, metadata,
and async signatures without taking on heavy dependencies.

## What lives here

- `SessionId`: strongly-typed session identifier
- `TraceId`: strongly-typed trace identifier
- `MetadataMap`: `HashMap<String, String>` for portable metadata
- `BoxFuture<'a, T>`: standard boxed async future alias

## Add dependency

```toml
[dependencies]
fcommon = { path = "../fcommon" }
```

## API usage

### IDs and metadata

```rust
use fcommon::{MetadataMap, SessionId, TraceId};

let session_id = SessionId::from("session-1");
let trace_id = TraceId::new("trace-abc");

let mut metadata = MetadataMap::new();
metadata.insert("tenant".to_string(), "acme".to_string());

assert_eq!(session_id.as_str(), "session-1");
assert_eq!(trace_id.to_string(), "trace-abc");
```

### Shared async contract

```rust
use fcommon::BoxFuture;

fn compute<'a>(value: &'a str) -> BoxFuture<'a, usize> {
    Box::pin(async move { value.len() })
}
```

## Design notes

- Keep this crate stable and dependency-light.
- Put cross-cutting primitives here only when multiple crates need them.
- Avoid domain logic in `fcommon`; domain behavior belongs in higher layers.
