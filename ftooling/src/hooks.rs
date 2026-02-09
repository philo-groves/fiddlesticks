//! Runtime hooks for tool execution lifecycle events.
//!
//! ```rust
//! use ftooling::{NoopToolRuntimeHooks, ToolRuntimeHooks};
//!
//! fn assert_hooks_trait(_hooks: &dyn ToolRuntimeHooks) {}
//!
//! let hooks = NoopToolRuntimeHooks;
//! assert_hooks_trait(&hooks);
//! ```

use std::time::Duration;

use fprovider::ToolCall;

use crate::{ToolError, ToolExecutionContext, ToolExecutionResult};

pub trait ToolRuntimeHooks: Send + Sync {
    fn on_execution_start(&self, _tool_call: &ToolCall, _context: &ToolExecutionContext) {}

    fn on_execution_success(
        &self,
        _tool_call: &ToolCall,
        _context: &ToolExecutionContext,
        _result: &ToolExecutionResult,
        _elapsed: Duration,
    ) {
    }

    fn on_execution_failure(
        &self,
        _tool_call: &ToolCall,
        _context: &ToolExecutionContext,
        _error: &ToolError,
        _elapsed: Duration,
    ) {
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NoopToolRuntimeHooks;

impl ToolRuntimeHooks for NoopToolRuntimeHooks {}
