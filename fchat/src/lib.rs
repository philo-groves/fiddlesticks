//! Conversational orchestration over model providers.

mod error;
mod service;
mod store;
mod tools;
mod types;

pub mod prelude {
    pub use crate::{
        ChatError, ChatErrorKind, ChatEvent, ChatEventStream, ChatService, ChatSession,
        ChatTurnRequest, ChatTurnResult, ConversationStore, InMemoryConversationStore,
        NoopToolRuntime, ToolRuntime,
    };
}

pub use error::{ChatError, ChatErrorKind};
pub use service::ChatService;
pub use store::{ConversationStore, InMemoryConversationStore};
pub use tools::{NoopToolRuntime, ToolRuntime};
pub use types::{ChatEvent, ChatEventStream, ChatSession, ChatTurnRequest, ChatTurnResult};
