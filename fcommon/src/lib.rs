//! Shared utilities and strongly-typed common values for workspace crates.

pub mod future {
    //! Shared async future aliases.

    use std::future::Future;
    use std::pin::Pin;

    pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
}

pub mod context {
    //! Shared metadata and cross-crate identifier newtypes.

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

pub use context::{MetadataMap, SessionId, TraceId};
pub use future::BoxFuture;

#[cfg(test)]
mod tests {
    use super::{SessionId, TraceId};

    #[test]
    fn id_newtypes_round_trip_strings() {
        let session = SessionId::new("session-1");
        let trace = TraceId::from("trace-1");

        assert_eq!(session.as_str(), "session-1");
        assert_eq!(trace.as_str(), "trace-1");
        assert_eq!(session.to_string(), "session-1");
        assert_eq!(trace.to_string(), "trace-1");
    }
}
