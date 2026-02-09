//! Tool trait contract for registry-managed capabilities.
//!
//! ```rust
//! use fprovider::ToolDefinition;
//! use ftooling::{FunctionTool, Tool};
//!
//! let tool = FunctionTool::new(
//!     ToolDefinition {
//!         name: "echo".to_string(),
//!         description: "Echoes input".to_string(),
//!         input_schema: r#"{"type":"string"}"#.to_string(),
//!     },
//!     |args, _ctx| async move { Ok(args) },
//! );
//!
//! assert_eq!(tool.definition().name, "echo");
//! ```

use std::future::Future;
use std::sync::Arc;

use fcommon::BoxFuture;
use fprovider::ToolDefinition;

use crate::{ToolError, ToolExecutionContext};

pub type ToolFuture<'a, T> = BoxFuture<'a, T>;

pub trait Tool: Send + Sync {
    fn definition(&self) -> ToolDefinition;

    fn invoke<'a>(
        &'a self,
        args_json: &'a str,
        context: &'a ToolExecutionContext,
    ) -> ToolFuture<'a, Result<String, ToolError>>;
}

type ToolHandler = dyn Fn(String, ToolExecutionContext) -> ToolFuture<'static, Result<String, ToolError>>
    + Send
    + Sync;

pub struct FunctionTool {
    definition: ToolDefinition,
    handler: Arc<ToolHandler>,
}

impl FunctionTool {
    pub fn new<F, Fut>(definition: ToolDefinition, handler: F) -> Self
    where
        F: Fn(String, ToolExecutionContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<String, ToolError>> + Send + 'static,
    {
        let handler: Arc<ToolHandler> =
            Arc::new(move |args_json, context| Box::pin(handler(args_json, context)));

        Self {
            definition,
            handler,
        }
    }
}

impl Tool for FunctionTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    fn invoke<'a>(
        &'a self,
        args_json: &'a str,
        context: &'a ToolExecutionContext,
    ) -> ToolFuture<'a, Result<String, ToolError>> {
        let args_json = args_json.to_string();
        let context = context.clone();
        (self.handler)(args_json, context)
    }
}
