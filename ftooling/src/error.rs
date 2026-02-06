//! Tool execution errors and classifications.

use std::error::Error;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolErrorKind {
    NotFound,
    InvalidArguments,
    Execution,
    Timeout,
    Unauthorized,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolError {
    pub kind: ToolErrorKind,
    pub message: String,
    pub retryable: bool,
}

impl ToolError {
    pub fn new(kind: ToolErrorKind, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            kind,
            message: message.into(),
            retryable,
        }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ToolErrorKind::NotFound, message, false)
    }

    pub fn invalid_arguments(message: impl Into<String>) -> Self {
        Self::new(ToolErrorKind::InvalidArguments, message, false)
    }

    pub fn execution(message: impl Into<String>) -> Self {
        Self::new(ToolErrorKind::Execution, message, false)
    }

    pub fn timeout(message: impl Into<String>) -> Self {
        Self::new(ToolErrorKind::Timeout, message, true)
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(ToolErrorKind::Unauthorized, message, false)
    }

    pub fn other(message: impl Into<String>) -> Self {
        Self::new(ToolErrorKind::Other, message, false)
    }
}

impl Display for ToolError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)
    }
}

impl Error for ToolError {}
