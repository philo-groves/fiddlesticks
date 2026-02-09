//! Shared provider error kinds and error value helpers.
//!
//! ```rust
//! use fprovider::ProviderError;
//!
//! let auth = ProviderError::authentication("bad key");
//! assert!(!auth.retryable);
//!
//! let timeout = ProviderError::timeout("temporary timeout");
//! assert!(timeout.retryable);
//! ```

use std::error::Error;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderErrorKind {
    Authentication,
    RateLimited,
    InvalidRequest,
    Timeout,
    Transport,
    Unavailable,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderError {
    pub kind: ProviderErrorKind,
    pub message: String,
    pub retryable: bool,
}

impl ProviderError {
    pub fn new(kind: ProviderErrorKind, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            kind,
            message: message.into(),
            retryable,
        }
    }

    pub fn authentication(message: impl Into<String>) -> Self {
        Self::new(ProviderErrorKind::Authentication, message, false)
    }

    pub fn rate_limited(message: impl Into<String>) -> Self {
        Self::new(ProviderErrorKind::RateLimited, message, true)
    }

    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(ProviderErrorKind::InvalidRequest, message, false)
    }

    pub fn timeout(message: impl Into<String>) -> Self {
        Self::new(ProviderErrorKind::Timeout, message, true)
    }

    pub fn transport(message: impl Into<String>) -> Self {
        Self::new(ProviderErrorKind::Transport, message, true)
    }

    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::new(ProviderErrorKind::Unavailable, message, true)
    }

    pub fn other(message: impl Into<String>) -> Self {
        Self::new(ProviderErrorKind::Other, message, false)
    }
}

impl Display for ProviderError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)
    }
}

impl Error for ProviderError {}
