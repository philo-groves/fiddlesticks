//! Capability layer for registering and executing tools.

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
        ToolRuntimeHooks,
    };
}

pub use error::{ToolError, ToolErrorKind};
pub use hooks::{NoopToolRuntimeHooks, ToolRuntimeHooks};
pub use registry::ToolRegistry;
pub use runtime::{DefaultToolRuntime, ToolRuntime};
pub use tool::{FunctionTool, Tool, ToolFuture};
pub use types::{ToolExecutionContext, ToolExecutionResult};
