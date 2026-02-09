//! Harness-level error types and conversion helpers.
//!
//! ```rust
//! use fharness::{HarnessError, HarnessErrorKind};
//!
//! let err = HarnessError::validation("outcome failed checks");
//! assert_eq!(err.kind, HarnessErrorKind::Validation);
//! assert!(err.to_string().contains("outcome failed checks"));
//! ```

use std::error::Error;
use std::fmt::{Display, Formatter};

use fchat::ChatError;
use fmemory::MemoryError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarnessErrorKind {
    InvalidRequest,
    Memory,
    Chat,
    Validation,
    HealthCheck,
    NotReady,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessError {
    pub kind: HarnessErrorKind,
    pub message: String,
}

impl HarnessError {
    pub fn new(kind: HarnessErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(HarnessErrorKind::InvalidRequest, message)
    }

    pub fn memory(message: impl Into<String>) -> Self {
        Self::new(HarnessErrorKind::Memory, message)
    }

    pub fn chat(message: impl Into<String>) -> Self {
        Self::new(HarnessErrorKind::Chat, message)
    }

    pub fn validation(message: impl Into<String>) -> Self {
        Self::new(HarnessErrorKind::Validation, message)
    }

    pub fn health_check(message: impl Into<String>) -> Self {
        Self::new(HarnessErrorKind::HealthCheck, message)
    }

    pub fn not_ready(message: impl Into<String>) -> Self {
        Self::new(HarnessErrorKind::NotReady, message)
    }
}

impl Display for HarnessError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)
    }
}

impl Error for HarnessError {}

impl From<MemoryError> for HarnessError {
    fn from(value: MemoryError) -> Self {
        HarnessError::memory(value.message)
    }
}

impl From<ChatError> for HarnessError {
    fn from(value: ChatError) -> Self {
        HarnessError::chat(value.to_string())
    }
}
