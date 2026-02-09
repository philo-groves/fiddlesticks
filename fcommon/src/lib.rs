//! Shared utilities and strongly-typed common values for workspace crates.
//!
//! ```rust
//! use fcommon::{GenerationOptions, MetadataMap, SessionId, TraceId};
//!
//! let session = SessionId::from("session-1");
//! let trace = TraceId::new("trace-1");
//! let mut metadata = MetadataMap::new();
//! metadata.insert("tenant".to_string(), "acme".to_string());
//!
//! let options = GenerationOptions::default().with_temperature(0.3).enable_streaming();
//! assert_eq!(session.as_str(), "session-1");
//! assert_eq!(trace.to_string(), "trace-1");
//! assert!(options.stream);
//! ```

pub mod future {
    //! Shared async future aliases.
    //!
    //! ```rust
    //! use fcommon::BoxFuture;
    //!
    //! fn str_len<'a>(value: &'a str) -> BoxFuture<'a, usize> {
    //!     Box::pin(async move { value.len() })
    //! }
    //!
    //! let _future = str_len("hello");
    //! ```

    use std::future::Future;
    use std::pin::Pin;

    pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
}

pub mod context {
    //! Shared metadata and cross-crate identifier newtypes.
    //!
    //! ```rust
    //! use fcommon::{MetadataMap, SessionId, TraceId};
    //!
    //! let session = SessionId::new("session-42");
    //! let trace = TraceId::from("trace-42");
    //! let mut metadata = MetadataMap::new();
    //! metadata.insert("env".to_string(), "test".to_string());
    //!
    //! assert_eq!(session.to_string(), "session-42");
    //! assert_eq!(trace.as_str(), "trace-42");
    //! ```

    use std::collections::HashMap;
    use std::fmt::{Display, Formatter};

    pub type MetadataMap = HashMap<String, String>;

    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    pub struct SessionId(String);

    impl SessionId {
        pub fn new(value: impl Into<String>) -> Self {
            Self(value.into())
        }

        pub fn as_str(&self) -> &str {
            self.0.as_str()
        }
    }

    impl Display for SessionId {
        fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
            f.write_str(&self.0)
        }
    }

    impl From<String> for SessionId {
        fn from(value: String) -> Self {
            Self(value)
        }
    }

    impl From<&str> for SessionId {
        fn from(value: &str) -> Self {
            Self(value.to_string())
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    pub struct TraceId(String);

    impl TraceId {
        pub fn new(value: impl Into<String>) -> Self {
            Self(value.into())
        }

        pub fn as_str(&self) -> &str {
            self.0.as_str()
        }
    }

    impl Display for TraceId {
        fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
            f.write_str(&self.0)
        }
    }

    impl From<String> for TraceId {
        fn from(value: String) -> Self {
            Self(value)
        }
    }

    impl From<&str> for TraceId {
        fn from(value: &str) -> Self {
            Self(value.to_string())
        }
    }
}

pub mod model {
    //! Shared generation settings used by request types.
    //!
    //! ```rust
    //! use fcommon::GenerationOptions;
    //!
    //! let options = GenerationOptions::default()
    //!     .with_temperature(0.2)
    //!     .with_max_tokens(128)
    //!     .enable_streaming();
    //!
    //! assert_eq!(options.temperature, Some(0.2));
    //! assert_eq!(options.max_tokens, Some(128));
    //! assert!(options.stream);
    //! ```

    #[derive(Debug, Clone, Copy, PartialEq, Default)]
    pub struct GenerationOptions {
        pub temperature: Option<f32>,
        pub max_tokens: Option<u32>,
        pub stream: bool,
    }

    impl GenerationOptions {
        pub fn with_temperature(mut self, temperature: f32) -> Self {
            self.temperature = Some(temperature);
            self
        }

        pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
            self.max_tokens = Some(max_tokens);
            self
        }

        pub fn with_streaming(mut self, stream: bool) -> Self {
            self.stream = stream;
            self
        }

        pub fn enable_streaming(self) -> Self {
            self.with_streaming(true)
        }
    }
}

pub mod registry {
    //! Generic registry map wrapper used by runtime registries.
    //!
    //! ```rust
    //! use fcommon::Registry;
    //!
    //! let mut registry = Registry::new();
    //! registry.insert("alpha".to_string(), 1_u32);
    //!
    //! assert_eq!(registry.get("alpha"), Some(&1));
    //! assert!(registry.contains_key("alpha"));
    //! ```

    use std::borrow::Borrow;
    use std::collections::HashMap;
    use std::hash::Hash;

    #[derive(Debug, Clone)]
    pub struct Registry<K, V> {
        items: HashMap<K, V>,
    }

    impl<K, V> Default for Registry<K, V>
    where
        K: Eq + Hash,
    {
        fn default() -> Self {
            Self {
                items: HashMap::new(),
            }
        }
    }

    impl<K, V> Registry<K, V>
    where
        K: Eq + Hash,
    {
        pub fn new() -> Self {
            Self::default()
        }

        pub fn insert(&mut self, key: K, value: V) -> Option<V> {
            self.items.insert(key, value)
        }

        pub fn get<Q>(&self, key: &Q) -> Option<&V>
        where
            K: Borrow<Q>,
            Q: Eq + Hash + ?Sized,
        {
            self.items.get(key)
        }

        pub fn remove<Q>(&mut self, key: &Q) -> Option<V>
        where
            K: Borrow<Q>,
            Q: Eq + Hash + ?Sized,
        {
            self.items.remove(key)
        }

        pub fn contains_key<Q>(&self, key: &Q) -> bool
        where
            K: Borrow<Q>,
            Q: Eq + Hash + ?Sized,
        {
            self.items.contains_key(key)
        }

        pub fn values(&self) -> impl Iterator<Item = &V> {
            self.items.values()
        }

        pub fn len(&self) -> usize {
            self.items.len()
        }

        pub fn is_empty(&self) -> bool {
            self.items.is_empty()
        }
    }
}

pub use context::{MetadataMap, SessionId, TraceId};
pub use future::BoxFuture;
pub use model::GenerationOptions;
pub use registry::Registry;

#[cfg(test)]
mod tests {
    use super::{GenerationOptions, Registry, SessionId, TraceId};

    #[test]
    fn id_newtypes_round_trip_strings() {
        let session = SessionId::new("session-1");
        let trace = TraceId::from("trace-1");

        assert_eq!(session.as_str(), "session-1");
        assert_eq!(trace.as_str(), "trace-1");
        assert_eq!(session.to_string(), "session-1");
        assert_eq!(trace.to_string(), "trace-1");
    }

    #[test]
    fn generation_options_builder_helpers_set_values() {
        let options = GenerationOptions::default()
            .with_temperature(0.3)
            .with_max_tokens(123)
            .enable_streaming();

        assert_eq!(options.temperature, Some(0.3));
        assert_eq!(options.max_tokens, Some(123));
        assert!(options.stream);
    }

    #[test]
    fn generic_registry_basic_lifecycle() {
        let mut registry = Registry::new();
        assert!(registry.is_empty());

        registry.insert("alpha".to_string(), 1_u32);
        assert_eq!(registry.get("alpha"), Some(&1));
        assert!(registry.contains_key("alpha"));
        assert_eq!(registry.len(), 1);

        let removed = registry.remove("alpha");
        assert_eq!(removed, Some(1));
        assert!(registry.is_empty());
    }
}
