//! Unified facade over the Fiddlesticks workspace crates.
//!
//! This crate is designed to be the single dependency for most applications.
//! It re-exports the core fiddlesticks crates and provides convenience utilities
//! and macros for common setup and request-building flows.

mod macros;

pub mod prelude;
pub mod runtime;
pub mod util;

pub use fchat;
pub use fcommon;
pub use fharness;
pub use fmemory;
pub use fprovider;
pub use ftooling;

pub use fchat::{
    ChatError, ChatErrorKind, ChatErrorPhase, ChatErrorSource, ChatEvent, ChatEventStream,
    ChatPolicy, ChatService, ChatServiceBuilder, ChatSession, ChatTurnOptions, ChatTurnRequest,
    ChatTurnRequestBuilder, ChatTurnResult, ConversationStore, InMemoryConversationStore,
};
pub use fcommon::{BoxFuture, MetadataMap, SessionId, TraceId};
pub use fharness::{
    AcceptAllValidator, FailFastPolicy, FeatureSelector, FirstPendingFeatureSelector, Harness,
    HarnessBuilder, HarnessError, HarnessErrorKind, HarnessPhase, HealthChecker,
    InitializerRequest, InitializerResult, NoopHealthChecker, OutcomeValidator, RunPolicy,
    RuntimeRunOutcome, RuntimeRunRequest, TaskIterationRequest, TaskIterationResult,
};
pub use fmemory::{
    BootstrapState, FeatureRecord, InMemoryMemoryBackend, MemoryBackend, MemoryConversationStore,
    MemoryError, MemoryErrorKind, ProgressEntry, RunCheckpoint, RunStatus, SessionManifest,
};
pub use fprovider::{
    BoxedEventStream, BrowserLoginSession, CredentialKind, Message, ModelEventStream,
    ModelProvider, ModelRequest, ModelRequestBuilder, ModelResponse, NoopOperationHooks,
    OutputItem, ProviderCredential, ProviderError, ProviderErrorKind, ProviderFuture, ProviderId,
    ProviderOperationHooks, ProviderRegistry, RetryPolicy, Role, SecretString,
    SecureCredentialManager, StopReason, StreamEvent, TokenUsage, ToolCall, ToolDefinition,
    ToolResult, VecEventStream, execute_with_retry,
};
pub use ftooling::{
    DefaultToolRuntime, FunctionTool, NoopToolRuntimeHooks, Tool, ToolError, ToolErrorKind,
    ToolExecutionContext, ToolExecutionResult, ToolFuture, ToolRegistry, ToolRuntime,
    ToolRuntimeHooks, parse_json_object, parse_json_value, required_string,
};

pub use runtime::{
    RuntimeBundle, build_runtime, build_runtime_with, build_runtime_with_memory,
    build_runtime_with_tooling, chat_service, chat_service_with_memory, in_memory_backend,
};
pub use util::{
    assistant_message, parse_provider_id, session, streaming_turn, system_message, tool_message,
    turn, user_message,
};

#[cfg(test)]
mod tests {
    use crate::{ProviderId, Role};

    #[test]
    fn fs_msg_macro_creates_expected_message() {
        let message = crate::fs_msg!(user => "hello");
        assert_eq!(message.role, Role::User);
        assert_eq!(message.content, "hello");
    }

    #[test]
    fn fs_messages_macro_builds_message_vector() {
        let messages = crate::fs_messages![
            system => "You are concise.",
            user => "Summarize the repo",
        ];

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, Role::System);
        assert_eq!(messages[1].role, Role::User);
    }

    #[test]
    fn fs_session_macro_supports_provider_shorthand_and_prompt() {
        let session = crate::fs_session!(
            "session-1",
            openai,
            "gpt-4o-mini",
            "You are concise and technical."
        );

        assert_eq!(session.provider, ProviderId::OpenAi);
        assert_eq!(
            session.system_prompt.as_deref(),
            Some("You are concise and technical.")
        );
    }
}
