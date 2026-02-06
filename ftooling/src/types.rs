//! Tool runtime context and execution result types.

use fcommon::{MetadataMap, SessionId, TraceId};
use fprovider::{ToolCall, ToolResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolExecutionContext {
    pub session_id: SessionId,
    pub trace_id: Option<TraceId>,
    pub metadata: MetadataMap,
}

impl ToolExecutionContext {
    pub fn new(session_id: impl Into<SessionId>) -> Self {
        Self {
            session_id: session_id.into(),
            trace_id: None,
            metadata: MetadataMap::new(),
        }
    }

    pub fn with_trace_id(mut self, trace_id: impl Into<TraceId>) -> Self {
        self.trace_id = Some(trace_id.into());
        self
    }

    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolExecutionResult {
    pub tool_call_id: String,
    pub output: String,
}

impl ToolExecutionResult {
    pub fn new(tool_call_id: impl Into<String>, output: impl Into<String>) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            output: output.into(),
        }
    }

    pub fn from_call(call: &ToolCall, output: impl Into<String>) -> Self {
        Self::new(call.id.clone(), output)
    }

    pub fn into_tool_result(self) -> ToolResult {
        ToolResult {
            tool_call_id: self.tool_call_id,
            output: self.output,
        }
    }
}
