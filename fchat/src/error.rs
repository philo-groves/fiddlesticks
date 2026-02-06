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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatErrorPhase {
    RequestValidation,
    Provider,
    Tooling,
    Storage,
    Streaming,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatErrorSource {
    Provider(fprovider::ProviderErrorKind),
    Tooling(ftooling::ToolErrorKind),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatError {
    pub kind: ChatErrorKind,
    pub message: String,
    pub retryable: bool,
    pub phase: Option<ChatErrorPhase>,
    pub source: Option<ChatErrorSource>,
}

impl ChatError {
    pub fn new(kind: ChatErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            retryable: false,
            phase: None,
            source: None,
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

    pub fn with_phase(mut self, phase: ChatErrorPhase) -> Self {
        self.phase = Some(phase);
        self
    }

    pub fn with_source(mut self, source: ChatErrorSource) -> Self {
        self.source = Some(source);
        self
    }

    pub fn with_retryable(mut self, retryable: bool) -> Self {
        self.retryable = retryable;
        self
    }

    pub fn is_retryable(&self) -> bool {
        self.retryable
    }

    pub fn is_user_error(&self) -> bool {
        if self.kind == ChatErrorKind::InvalidRequest {
            return true;
        }

        matches!(
            self.source,
            Some(ChatErrorSource::Tooling(
                ftooling::ToolErrorKind::InvalidArguments
            )) | Some(ChatErrorSource::Tooling(ftooling::ToolErrorKind::NotFound))
                | Some(ChatErrorSource::Tooling(
                    ftooling::ToolErrorKind::Unauthorized
                ))
        )
    }
}

impl Display for ChatError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if let Some(phase) = self.phase {
            write!(f, "{:?} ({:?}): {}", self.kind, phase, self.message)
        } else {
            write!(f, "{:?}: {}", self.kind, self.message)
        }
    }
}

impl Error for ChatError {}

impl From<fprovider::ProviderError> for ChatError {
    fn from(value: fprovider::ProviderError) -> Self {
        ChatError::provider(value.message)
            .with_retryable(value.retryable)
            .with_phase(ChatErrorPhase::Provider)
            .with_source(ChatErrorSource::Provider(value.kind))
    }
}

impl From<ftooling::ToolError> for ChatError {
    fn from(value: ftooling::ToolError) -> Self {
        ChatError::tooling(value.message)
            .with_retryable(value.retryable)
            .with_phase(ChatErrorPhase::Tooling)
            .with_source(ChatErrorSource::Tooling(value.kind))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_error_conversion_preserves_retryability_and_source() {
        let provider_error = fprovider::ProviderError::timeout("timed out");
        let chat_error = ChatError::from(provider_error);

        assert_eq!(chat_error.kind, ChatErrorKind::Provider);
        assert!(chat_error.is_retryable());
        assert_eq!(chat_error.phase, Some(ChatErrorPhase::Provider));
        assert_eq!(
            chat_error.source,
            Some(ChatErrorSource::Provider(
                fprovider::ProviderErrorKind::Timeout
            ))
        );
    }

    #[test]
    fn tooling_invalid_arguments_is_user_error() {
        let tooling_error =
            ftooling::ToolError::invalid_arguments("bad args").with_tool_name("lookup");
        let chat_error = ChatError::from(tooling_error);

        assert_eq!(chat_error.kind, ChatErrorKind::Tooling);
        assert!(chat_error.is_user_error());
    }
}
