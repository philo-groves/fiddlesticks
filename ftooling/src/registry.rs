//! Tool registry for lookup by tool definition name.

use std::future::Future;
use std::sync::Arc;

use fcommon::Registry;
use fprovider::ToolDefinition;

use crate::{FunctionTool, Tool, ToolError, ToolExecutionContext};

#[derive(Default)]
pub struct ToolRegistry {
    tools: Registry<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<T>(&mut self, tool: T)
    where
        T: Tool + 'static,
    {
        let name = tool.definition().name;
        self.tools.insert(name, Arc::new(tool));
    }

    pub fn register_fn<F, Fut>(&mut self, definition: ToolDefinition, handler: F)
    where
        F: Fn(String, ToolExecutionContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<String, ToolError>> + Send + 'static,
    {
        self.register(FunctionTool::new(definition, handler));
    }

    pub fn register_sync_fn<F>(&mut self, definition: ToolDefinition, handler: F)
    where
        F: Fn(String, ToolExecutionContext) -> Result<String, ToolError> + Send + Sync + 'static,
    {
        self.register_fn(definition, move |args_json, context| {
            let output = handler(args_json, context);
            async move { output }
        });
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    pub fn contains(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    pub fn remove(&mut self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.remove(name)
    }

    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools.values().map(|tool| tool.definition()).collect()
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}
