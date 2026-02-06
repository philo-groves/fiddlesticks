//! Chat-layer errors and classification.

use std::error::Error;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatErrorKind {
    InvalidRequest,
    Provider,
    Store,
    Tooling,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatError {
    pub kind: ChatErrorKind,
    pub message: String,
}

impl ChatError {
    pub fn new(kind: ChatErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(ChatErrorKind::InvalidRequest, message)
    }

    pub fn provider(message: impl Into<String>) -> Self {
        Self::new(ChatErrorKind::Provider, message)
    }

    pub fn store(message: impl Into<String>) -> Self {
        Self::new(ChatErrorKind::Store, message)
    }

    pub fn tooling(message: impl Into<String>) -> Self {
        Self::new(ChatErrorKind::Tooling, message)
    }
}

impl Display for ChatError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)
    }
}

impl Error for ChatError {}

impl From<fprovider::ProviderError> for ChatError {
    fn from(value: fprovider::ProviderError) -> Self {
        ChatError::provider(value.to_string())
    }
}
