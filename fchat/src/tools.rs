//! Tool runtime contract for future tool-call execution loops.

use std::future::Future;
use std::pin::Pin;

use fprovider::ToolCall;

use crate::ChatError;

pub type ToolFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait ToolRuntime: Send + Sync {
    fn execute<'a>(&'a self, _tool_call: ToolCall) -> ToolFuture<'a, Result<String, ChatError>>;
}

#[derive(Debug, Default)]
pub struct NoopToolRuntime;

impl ToolRuntime for NoopToolRuntime {
    fn execute<'a>(&'a self, _tool_call: ToolCall) -> ToolFuture<'a, Result<String, ChatError>> {
        Box::pin(async { Err(ChatError::tooling("tool execution is not configured")) })
    }
}
