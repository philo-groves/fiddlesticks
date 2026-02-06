//! Tool runtime trait and default registry-backed executor.

use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_timer::Delay;
use futures_util::future::{Either, select};
use futures_util::{FutureExt, pin_mut};
use fprovider::ToolCall;

use crate::{
    NoopToolRuntimeHooks, ToolError, ToolExecutionContext, ToolExecutionResult, ToolFuture,
    ToolRegistry, ToolRuntimeHooks,
};

pub trait ToolRuntime: Send + Sync {
    fn execute<'a>(
        &'a self,
        tool_call: ToolCall,
        context: ToolExecutionContext,
    ) -> ToolFuture<'a, Result<ToolExecutionResult, ToolError>>;
}

#[derive(Clone)]
pub struct DefaultToolRuntime {
    registry: Arc<ToolRegistry>,
    hooks: Arc<dyn ToolRuntimeHooks>,
    timeout: Option<Duration>,
}

impl Default for DefaultToolRuntime {
    fn default() -> Self {
        Self::new(Arc::new(ToolRegistry::new()))
    }
}

impl DefaultToolRuntime {
    pub fn new(registry: Arc<ToolRegistry>) -> Self {
        Self {
            registry,
            hooks: Arc::new(NoopToolRuntimeHooks),
            timeout: None,
        }
    }

    pub fn registry(&self) -> Arc<ToolRegistry> {
        Arc::clone(&self.registry)
    }

    pub fn with_hooks(mut self, hooks: Arc<dyn ToolRuntimeHooks>) -> Self {
        self.hooks = hooks;
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn clear_timeout(mut self) -> Self {
        self.timeout = None;
        self
    }
}

impl ToolRuntime for DefaultToolRuntime {
    fn execute<'a>(
        &'a self,
        tool_call: ToolCall,
        context: ToolExecutionContext,
    ) -> ToolFuture<'a, Result<ToolExecutionResult, ToolError>> {
        Box::pin(async move {
            let started_at = Instant::now();
            self.hooks.on_execution_start(&tool_call, &context);

            let tool = self.registry.get(&tool_call.name).ok_or_else(|| {
                let error =
                    ToolError::not_found(format!("tool '{}' is not registered", tool_call.name));
                self.hooks
                    .on_execution_failure(&tool_call, &context, &error, started_at.elapsed());
                error
            })?;

            let invocation = tool.invoke(&tool_call.arguments, &context);

            let output = if let Some(timeout) = self.timeout {
                let invoke = invocation.fuse();
                let delay = Delay::new(timeout).fuse();
                pin_mut!(invoke, delay);

                match select(invoke, delay).await {
                    Either::Left((result, _)) => result,
                    Either::Right((_timeout_elapsed, _)) => {
                        let error = ToolError::timeout(format!(
                            "tool '{}' timed out after {:?}",
                            tool_call.name, timeout
                        ));
                        self.hooks.on_execution_failure(
                            &tool_call,
                            &context,
                            &error,
                            started_at.elapsed(),
                        );
                        return Err(error);
                    }
                }
            } else {
                invocation.await
            };

            match output {
                Ok(output) => {
                    let result = ToolExecutionResult::from_call(&tool_call, output);
                    self.hooks.on_execution_success(
                        &tool_call,
                        &context,
                        &result,
                        started_at.elapsed(),
                    );
                    Ok(result)
                }
                Err(error) => {
                    self.hooks
                        .on_execution_failure(&tool_call, &context, &error, started_at.elapsed());
                    Err(error)
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use fprovider::{ToolCall, ToolDefinition};

    use super::*;
    use crate::{Tool, ToolErrorKind, ToolRuntimeHooks};

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

    #[derive(Debug)]
    struct SlowTool;

    impl Tool for SlowTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "slow".to_string(),
                description: "Sleeps then returns".to_string(),
                input_schema: "{\"type\":\"object\"}".to_string(),
            }
        }

        fn invoke<'a>(
            &'a self,
            _args_json: &'a str,
            _context: &'a ToolExecutionContext,
        ) -> ToolFuture<'a, Result<String, ToolError>> {
            Box::pin(async move {
                Delay::new(Duration::from_millis(50)).await;
                Ok("done".to_string())
            })
        }
    }

    #[derive(Default)]
    struct RecordingHooks {
        events: Mutex<Vec<String>>,
    }

    impl ToolRuntimeHooks for RecordingHooks {
        fn on_execution_start(&self, tool_call: &ToolCall, _context: &ToolExecutionContext) {
            self.events
                .lock()
                .expect("events lock")
                .push(format!("start:{}", tool_call.name));
        }

        fn on_execution_success(
            &self,
            tool_call: &ToolCall,
            _context: &ToolExecutionContext,
            _result: &ToolExecutionResult,
            _elapsed: Duration,
        ) {
            self.events
                .lock()
                .expect("events lock")
                .push(format!("success:{}", tool_call.name));
        }

        fn on_execution_failure(
            &self,
            tool_call: &ToolCall,
            _context: &ToolExecutionContext,
            error: &ToolError,
            _elapsed: Duration,
        ) {
            self.events
                .lock()
                .expect("events lock")
                .push(format!("failure:{}:{:?}", tool_call.name, error.kind));
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

    #[tokio::test]
    async fn runtime_timeout_returns_timeout_error() {
        let mut registry = ToolRegistry::new();
        registry.register(SlowTool);
        let runtime = DefaultToolRuntime::new(Arc::new(registry)).with_timeout(Duration::from_millis(10));

        let error = runtime
            .execute(
                ToolCall {
                    id: "call_4".to_string(),
                    name: "slow".to_string(),
                    arguments: "{}".to_string(),
                },
                ToolExecutionContext::new("session-4"),
            )
            .await
            .expect_err("execution should time out");

        assert_eq!(error.kind, ToolErrorKind::Timeout);
    }

    #[tokio::test]
    async fn runtime_hooks_receive_success_and_failure_events() {
        let hooks = Arc::new(RecordingHooks::default());

        let mut success_registry = ToolRegistry::new();
        success_registry.register(EchoTool);
        let runtime =
            DefaultToolRuntime::new(Arc::new(success_registry)).with_hooks(hooks.clone());

        let _ = runtime
            .execute(
                ToolCall {
                    id: "call_5".to_string(),
                    name: "echo".to_string(),
                    arguments: "hello".to_string(),
                },
                ToolExecutionContext::new("session-5"),
            )
            .await
            .expect("execution should succeed");

        let not_found = runtime
            .execute(
                ToolCall {
                    id: "call_6".to_string(),
                    name: "missing".to_string(),
                    arguments: "{}".to_string(),
                },
                ToolExecutionContext::new("session-6"),
            )
            .await
            .expect_err("execution should fail");
        assert_eq!(not_found.kind, ToolErrorKind::NotFound);

        let events = hooks.events.lock().expect("events lock").clone();
        assert!(events.contains(&"start:echo".to_string()));
        assert!(events.contains(&"success:echo".to_string()));
        assert!(events.iter().any(|event| event.starts_with("failure:missing:NotFound")));
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
