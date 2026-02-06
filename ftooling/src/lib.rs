//! Capability layer for registering and executing tools.

mod error;
mod registry;
mod runtime;
mod tool;
mod types;

pub mod prelude {
    pub use crate::{
        DefaultToolRuntime, Tool, ToolError, ToolErrorKind, ToolExecutionContext,
        ToolExecutionResult, ToolFuture, ToolRegistry, ToolRuntime,
    };
}

pub use error::{ToolError, ToolErrorKind};
pub use registry::ToolRegistry;
pub use runtime::{DefaultToolRuntime, ToolRuntime};
pub use tool::{Tool, ToolFuture};
pub use types::{ToolExecutionContext, ToolExecutionResult};
