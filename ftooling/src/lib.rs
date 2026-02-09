//! Capability layer for registering and executing tools.
//!
//! ```rust
//! use fprovider::ToolDefinition;
//! use ftooling::{DefaultToolRuntime, ToolRegistry};
//! use std::sync::Arc;
//!
//! let mut registry = ToolRegistry::new();
//! registry.register_sync_fn(
//!     ToolDefinition {
//!         name: "echo".to_string(),
//!         description: "Echoes raw arguments".to_string(),
//!         input_schema: r#"{"type":"string"}"#.to_string(),
//!     },
//!     |args, _ctx| Ok(args),
//! );
//!
//! let runtime = DefaultToolRuntime::new(Arc::new(registry));
//! assert_eq!(runtime.registry().len(), 1);
//! ```

mod args;
mod error;
mod hooks;
mod registry;
mod runtime;
mod tool;
mod types;

pub mod prelude {
    pub use crate::{
        DefaultToolRuntime, FunctionTool, NoopToolRuntimeHooks, Tool, ToolError, ToolErrorKind,
        ToolExecutionContext, ToolExecutionResult, ToolFuture, ToolRegistry, ToolRuntime,
        ToolRuntimeHooks, parse_json_object, parse_json_value, required_string,
    };
    pub use fcommon::{MetadataMap, SessionId, TraceId};
}

pub use args::{parse_json_object, parse_json_value, required_string};
pub use error::{ToolError, ToolErrorKind};
pub use fcommon::{MetadataMap, SessionId, TraceId};
pub use hooks::{NoopToolRuntimeHooks, ToolRuntimeHooks};
pub use registry::ToolRegistry;
pub use runtime::{DefaultToolRuntime, ToolRuntime};
pub use tool::{FunctionTool, Tool, ToolFuture};
pub use types::{ToolExecutionContext, ToolExecutionResult};
