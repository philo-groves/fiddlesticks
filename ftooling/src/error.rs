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
    pub tool_name: Option<String>,
    pub tool_call_id: Option<String>,
}

impl ToolError {
    pub fn new(kind: ToolErrorKind, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            kind,
            message: message.into(),
            retryable,
            tool_name: None,
            tool_call_id: None,
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

    pub fn with_tool_name(mut self, tool_name: impl Into<String>) -> Self {
        self.tool_name = Some(tool_name.into());
        self
    }

    pub fn with_tool_call_id(mut self, tool_call_id: impl Into<String>) -> Self {
        self.tool_call_id = Some(tool_call_id.into());
        self
    }

    pub fn is_retryable(&self) -> bool {
        self.retryable
    }

    pub fn is_user_error(&self) -> bool {
        matches!(
            self.kind,
            ToolErrorKind::InvalidArguments | ToolErrorKind::NotFound | ToolErrorKind::Unauthorized
        )
    }
}

impl Display for ToolError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match (&self.tool_name, &self.tool_call_id) {
            (Some(tool_name), Some(tool_call_id)) => write!(
                f,
                "{:?} [tool={}, call_id={}]: {}",
                self.kind, tool_name, tool_call_id, self.message
            ),
            (Some(tool_name), None) => {
                write!(f, "{:?} [tool={}]: {}", self.kind, tool_name, self.message)
            }
            _ => write!(f, "{:?}: {}", self.kind, self.message),
        }
    }
}

impl Error for ToolError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helper_methods_report_retryable_and_user_error() {
        let timeout = ToolError::timeout("slow");
        assert!(timeout.is_retryable());
        assert!(!timeout.is_user_error());

        let invalid = ToolError::invalid_arguments("bad args");
        assert!(!invalid.is_retryable());
        assert!(invalid.is_user_error());
    }

    #[test]
    fn context_fields_are_included_in_display() {
        let error = ToolError::not_found("missing")
            .with_tool_name("lookup")
            .with_tool_call_id("call_1");

        let rendered = error.to_string();
        assert!(rendered.contains("lookup"));
        assert!(rendered.contains("call_1"));
    }
}
