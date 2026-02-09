//! Runtime wiring helpers for chat and harness usage.
//!
//! ```rust
//! use std::sync::Arc;
//!
//! use fiddlesticks::{ModelProvider, build_runtime_with_memory, in_memory_backend};
//!
//! fn wire_runtime(provider: Arc<dyn ModelProvider>) {
//!     let memory = in_memory_backend();
//!     let runtime = build_runtime_with_memory(provider, memory);
//!     assert!(runtime.is_ok());
//! }
//! ```

use std::sync::Arc;

use crate::{
    ChatService, Harness, HarnessError, InMemoryMemoryBackend, MemoryBackend,
    MemoryConversationStore, ModelProvider, ToolRuntime, create_default_memory_backend,
};

#[derive(Clone)]
pub struct RuntimeBundle {
    pub memory: Arc<dyn MemoryBackend>,
    pub chat: ChatService,
    pub harness: Harness,
}

pub fn in_memory_backend() -> Arc<dyn MemoryBackend> {
    Arc::new(InMemoryMemoryBackend::new())
}

pub fn chat_service(provider: Arc<dyn ModelProvider>) -> ChatService {
    ChatService::builder(provider).build()
}

pub fn chat_service_with_memory(
    provider: Arc<dyn ModelProvider>,
    memory: Arc<dyn MemoryBackend>,
) -> ChatService {
    let store = Arc::new(MemoryConversationStore::new(memory));
    ChatService::builder(provider).store(store).build()
}

pub fn build_runtime(provider: Arc<dyn ModelProvider>) -> Result<RuntimeBundle, HarnessError> {
    let memory = create_default_memory_backend()?;
    build_runtime_with(provider, memory, None)
}

pub fn build_runtime_with_memory(
    provider: Arc<dyn ModelProvider>,
    memory: Arc<dyn MemoryBackend>,
) -> Result<RuntimeBundle, HarnessError> {
    build_runtime_with(provider, memory, None)
}

pub fn build_runtime_with_tooling(
    provider: Arc<dyn ModelProvider>,
    tool_runtime: Arc<dyn ToolRuntime>,
) -> Result<RuntimeBundle, HarnessError> {
    let memory = create_default_memory_backend()?;
    build_runtime_with(provider, memory, Some(tool_runtime))
}

pub fn build_runtime_with(
    provider: Arc<dyn ModelProvider>,
    memory: Arc<dyn MemoryBackend>,
    tool_runtime: Option<Arc<dyn ToolRuntime>>,
) -> Result<RuntimeBundle, HarnessError> {
    let store = Arc::new(MemoryConversationStore::new(Arc::clone(&memory)));

    let mut chat_builder = ChatService::builder(Arc::clone(&provider)).store(store);
    let mut harness_builder = Harness::builder(Arc::clone(&memory)).provider(provider);

    if let Some(runtime) = tool_runtime {
        chat_builder = chat_builder.tool_runtime(Arc::clone(&runtime));
        harness_builder = harness_builder.tool_runtime(runtime);
    }

    let chat = chat_builder.build();
    let harness = harness_builder.build()?;

    Ok(RuntimeBundle {
        memory,
        chat,
        harness,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::{
        DefaultToolRuntime, Message, ModelProvider, ModelRequest,
        ModelResponse, OutputItem, ProviderError, ProviderFuture, ProviderId, Role, StopReason,
        StreamEvent, TokenUsage, ToolRuntime, VecEventStream,
    };

    use super::{build_runtime_with_tooling};

    #[derive(Debug)]
    struct FakeProvider;

    impl ModelProvider for FakeProvider {
        fn id(&self) -> ProviderId {
            ProviderId::OpenAi
        }

        fn complete<'a>(
            &'a self,
            request: ModelRequest,
        ) -> ProviderFuture<'a, Result<ModelResponse, ProviderError>> {
            Box::pin(async move {
                request.validate()?;
                Ok(ModelResponse {
                    provider: ProviderId::OpenAi,
                    model: request.model,
                    output: vec![OutputItem::Message(Message::new(Role::Assistant, "done"))],
                    stop_reason: StopReason::EndTurn,
                    usage: TokenUsage::default(),
                })
            })
        }

        fn stream<'a>(
            &'a self,
            request: ModelRequest,
        ) -> ProviderFuture<'a, Result<crate::BoxedEventStream<'a>, ProviderError>> {
            Box::pin(async move {
                request.validate()?;
                let response = ModelResponse {
                    provider: ProviderId::OpenAi,
                    model: request.model,
                    output: vec![OutputItem::Message(Message::new(Role::Assistant, "done"))],
                    stop_reason: StopReason::EndTurn,
                    usage: TokenUsage::default(),
                };
                let stream = VecEventStream::new(vec![Ok(StreamEvent::ResponseComplete(response))]);
                Ok(Box::pin(stream) as crate::BoxedEventStream<'a>)
            })
        }
    }

    #[test]
    fn build_runtime_with_tooling_builds_successfully() {
        let provider: Arc<dyn ModelProvider> = Arc::new(FakeProvider);
        let tool_runtime: Arc<dyn ToolRuntime> = Arc::new(DefaultToolRuntime::default());

        let runtime =
            build_runtime_with_tooling(provider, tool_runtime).expect("runtime should build");
        let _chat = runtime.chat.clone();
        let _harness = runtime.harness.clone();
    }
}
