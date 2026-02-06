//! Memory-layer errors for state and transcript persistence operations.

use std::error::Error;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryErrorKind {
    Storage,
    NotFound,
    InvalidRequest,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryError {
    pub kind: MemoryErrorKind,
    pub message: String,
}

impl MemoryError {
    pub fn new(kind: MemoryErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn storage(message: impl Into<String>) -> Self {
        Self::new(MemoryErrorKind::Storage, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(MemoryErrorKind::NotFound, message)
    }

    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(MemoryErrorKind::InvalidRequest, message)
    }

    pub fn other(message: impl Into<String>) -> Self {
        Self::new(MemoryErrorKind::Other, message)
    }
}

impl Display for MemoryError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)
    }
}

impl Error for MemoryError {}
