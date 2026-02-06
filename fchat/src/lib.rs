//! Conversational orchestration over model providers.

mod error;
mod service;
mod store;
mod types;

pub mod prelude {
    pub use crate::{
        ChatError, ChatErrorKind, ChatEvent, ChatEventStream, ChatService, ChatSession,
        ChatTurnRequest, ChatTurnResult, ConversationStore, InMemoryConversationStore,
    };
    pub use ftooling::{
        DefaultToolRuntime, Tool, ToolError, ToolErrorKind, ToolExecutionContext,
        ToolExecutionResult, ToolRegistry, ToolRuntime,
    };
}

pub use error::{ChatError, ChatErrorKind};
pub use service::ChatService;
pub use store::{ConversationStore, InMemoryConversationStore};
pub use types::{ChatEvent, ChatEventStream, ChatSession, ChatTurnRequest, ChatTurnResult};
pub use ftooling::{
    DefaultToolRuntime, Tool, ToolError, ToolErrorKind, ToolExecutionContext, ToolExecutionResult,
    ToolRegistry, ToolRuntime,
};
