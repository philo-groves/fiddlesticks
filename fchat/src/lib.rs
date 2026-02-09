//! Conversational orchestration over model providers.
//!
//! ```rust
//! use fchat::{ChatSession, ChatTurnRequest};
//! use fprovider::ProviderId;
//!
//! let session = ChatSession::new("session-1", ProviderId::OpenAi, "gpt-4o-mini")
//!     .with_system_prompt("Be concise.");
//! let request = ChatTurnRequest::new(session, "Summarize this patch");
//! assert_eq!(request.user_input, "Summarize this patch");
//! ```

mod error;
mod service;
mod store;
mod types;

pub mod prelude {
    pub use crate::{
        ChatError, ChatErrorKind, ChatErrorPhase, ChatErrorSource, ChatEvent, ChatEventStream,
        ChatPolicy, ChatService, ChatServiceBuilder, ChatSession, ChatTurnOptions, ChatTurnRequest,
        ChatTurnRequestBuilder, ChatTurnResult, ConversationStore, InMemoryConversationStore,
    };
    pub use fcommon::{MetadataMap, SessionId, TraceId};
    pub use ftooling::{
        DefaultToolRuntime, Tool, ToolError, ToolErrorKind, ToolExecutionContext,
        ToolExecutionResult, ToolRegistry, ToolRuntime,
    };
}

pub use error::{ChatError, ChatErrorKind, ChatErrorPhase, ChatErrorSource};
pub use fcommon::{MetadataMap, SessionId, TraceId};
pub use ftooling::{
    DefaultToolRuntime, Tool, ToolError, ToolErrorKind, ToolExecutionContext, ToolExecutionResult,
    ToolRegistry, ToolRuntime,
};
pub use service::{ChatPolicy, ChatService, ChatServiceBuilder};
pub use store::{ConversationStore, InMemoryConversationStore};
pub use types::{
    ChatEvent, ChatEventStream, ChatSession, ChatTurnOptions, ChatTurnRequest,
    ChatTurnRequestBuilder, ChatTurnResult,
};
