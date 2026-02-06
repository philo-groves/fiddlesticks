//! Conversational orchestration over model providers.

mod error;
mod service;
mod store;
mod types;

pub mod prelude {
    pub use crate::{
        ChatError, ChatErrorKind, ChatErrorPhase, ChatErrorSource, ChatEvent, ChatEventStream,
        ChatPolicy, ChatService, ChatServiceBuilder, ChatSession, ChatTurnOptions,
        ChatTurnRequest, ChatTurnResult,
        ConversationStore, InMemoryConversationStore,
    };
    pub use fcommon::{MetadataMap, SessionId, TraceId};
    pub use ftooling::{
        DefaultToolRuntime, Tool, ToolError, ToolErrorKind, ToolExecutionContext,
        ToolExecutionResult, ToolRegistry, ToolRuntime,
    };
}

pub use error::{ChatError, ChatErrorKind, ChatErrorPhase, ChatErrorSource};
pub use service::{ChatPolicy, ChatService, ChatServiceBuilder};
pub use store::{ConversationStore, InMemoryConversationStore};
pub use types::{
    ChatEvent, ChatEventStream, ChatSession, ChatTurnOptions, ChatTurnRequest, ChatTurnResult,
};
pub use fcommon::{MetadataMap, SessionId, TraceId};
pub use ftooling::{
    DefaultToolRuntime, Tool, ToolError, ToolErrorKind, ToolExecutionContext, ToolExecutionResult,
    ToolRegistry, ToolRuntime,
};
