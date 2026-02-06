//! Tool trait contract for registry-managed capabilities.

use std::future::Future;
use std::pin::Pin;

use fprovider::ToolDefinition;

use crate::{ToolError, ToolExecutionContext};

pub type ToolFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait Tool: Send + Sync {
    fn definition(&self) -> ToolDefinition;

    fn invoke<'a>(
        &'a self,
        args_json: &'a str,
        context: &'a ToolExecutionContext,
    ) -> ToolFuture<'a, Result<String, ToolError>>;
}
