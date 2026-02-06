//! Tool runtime trait and default registry-backed executor.

use std::sync::Arc;

use fprovider::ToolCall;

use crate::{
    ToolError, ToolExecutionContext, ToolExecutionResult, ToolFuture, ToolRegistry,
};

pub trait ToolRuntime: Send + Sync {
    fn execute<'a>(
        &'a self,
        tool_call: ToolCall,
        context: ToolExecutionContext,
    ) -> ToolFuture<'a, Result<ToolExecutionResult, ToolError>>;
}

#[derive(Clone, Default)]
pub struct DefaultToolRuntime {
    registry: Arc<ToolRegistry>,
}

impl DefaultToolRuntime {
    pub fn new(registry: Arc<ToolRegistry>) -> Self {
        Self { registry }
    }

    pub fn registry(&self) -> Arc<ToolRegistry> {
        Arc::clone(&self.registry)
    }
}

impl ToolRuntime for DefaultToolRuntime {
    fn execute<'a>(
        &'a self,
        tool_call: ToolCall,
        context: ToolExecutionContext,
    ) -> ToolFuture<'a, Result<ToolExecutionResult, ToolError>> {
        Box::pin(async move {
            let tool = self.registry.get(&tool_call.name).ok_or_else(|| {
                ToolError::not_found(format!("tool '{}' is not registered", tool_call.name))
            })?;

            let output = tool.invoke(&tool_call.arguments, &context).await?;
            Ok(ToolExecutionResult::from_call(&tool_call, output))
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use fprovider::{ToolCall, ToolDefinition};

    use super::*;
    use crate::{Tool, ToolErrorKind};

    #[derive(Debug)]
    struct EchoTool;

    impl Tool for EchoTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "echo".to_string(),
                description: "Echoes arguments".to_string(),
                input_schema: "{\"type\":\"string\"}".to_string(),
            }
        }

        fn invoke<'a>(
            &'a self,
            args_json: &'a str,
            context: &'a ToolExecutionContext,
        ) -> ToolFuture<'a, Result<String, ToolError>> {
            Box::pin(async move {
                Ok(format!(
                    "session={} args={}",
                    context.session_id,
                    args_json
                ))
            })
        }
    }

    #[derive(Debug)]
    struct BrokenTool;

    impl Tool for BrokenTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "broken".to_string(),
                description: "Always fails".to_string(),
                input_schema: "{\"type\":\"object\"}".to_string(),
            }
        }

        fn invoke<'a>(
            &'a self,
            _args_json: &'a str,
            _context: &'a ToolExecutionContext,
        ) -> ToolFuture<'a, Result<String, ToolError>> {
            Box::pin(async move { Err(ToolError::execution("tool exploded")) })
        }
    }

    #[tokio::test]
    async fn runtime_executes_registered_tool() {
        let mut registry = ToolRegistry::new();
        registry.register(EchoTool);
        let runtime = DefaultToolRuntime::new(Arc::new(registry));

        let result = runtime
            .execute(
                ToolCall {
                    id: "call_1".to_string(),
                    name: "echo".to_string(),
                    arguments: "hello".to_string(),
                },
                ToolExecutionContext::new("session-1"),
            )
            .await
            .expect("execution should succeed");

        assert_eq!(result.tool_call_id, "call_1");
        assert_eq!(result.output, "session=session-1 args=hello");
    }

    #[tokio::test]
    async fn runtime_returns_not_found_for_unknown_tool() {
        let runtime = DefaultToolRuntime::new(Arc::new(ToolRegistry::new()));

        let error = runtime
            .execute(
                ToolCall {
                    id: "call_2".to_string(),
                    name: "missing".to_string(),
                    arguments: "{}".to_string(),
                },
                ToolExecutionContext::new("session-2"),
            )
            .await
            .expect_err("execution should fail");

        assert_eq!(error.kind, ToolErrorKind::NotFound);
    }

    #[tokio::test]
    async fn runtime_propagates_tool_execution_error() {
        let mut registry = ToolRegistry::new();
        registry.register(BrokenTool);
        let runtime = DefaultToolRuntime::new(Arc::new(registry));

        let error = runtime
            .execute(
                ToolCall {
                    id: "call_3".to_string(),
                    name: "broken".to_string(),
                    arguments: "{}".to_string(),
                },
                ToolExecutionContext::new("session-3"),
            )
            .await
            .expect_err("execution should fail");

        assert_eq!(error.kind, ToolErrorKind::Execution);
        assert_eq!(error.message, "tool exploded");
    }

    #[test]
    fn registry_tracks_registered_tools() {
        let mut registry = ToolRegistry::new();
        assert!(registry.is_empty());

        registry.register(EchoTool);
        assert_eq!(registry.len(), 1);
        assert!(registry.contains("echo"));
        assert_eq!(registry.definitions().len(), 1);

        let removed = registry.remove("echo");
        assert!(removed.is_some());
        assert!(registry.is_empty());
    }
}
