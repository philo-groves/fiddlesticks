//! Adapter that exposes fmemory as an fchat ConversationStore.

use std::sync::Arc;

use fchat::{ChatError, ChatErrorPhase, ConversationStore};
use fcommon::{BoxFuture, SessionId};
use fprovider::Message;

use crate::backend::MemoryBackend;
use crate::error::MemoryError;

#[derive(Clone)]
pub struct MemoryConversationStore {
    backend: Arc<dyn MemoryBackend>,
}

impl MemoryConversationStore {
    pub fn new(backend: Arc<dyn MemoryBackend>) -> Self {
        Self { backend }
    }

    pub fn backend(&self) -> Arc<dyn MemoryBackend> {
        Arc::clone(&self.backend)
    }
}

impl ConversationStore for MemoryConversationStore {
    fn load_messages<'a>(
        &'a self,
        session_id: &'a SessionId,
    ) -> BoxFuture<'a, Result<Vec<Message>, ChatError>> {
        Box::pin(async move {
            self.backend
                .load_transcript_messages(session_id)
                .await
                .map_err(memory_error_to_chat_error)
        })
    }

    fn append_messages<'a>(
        &'a self,
        session_id: &'a SessionId,
        messages: Vec<Message>,
    ) -> BoxFuture<'a, Result<(), ChatError>> {
        Box::pin(async move {
            self.backend
                .append_transcript_messages(session_id, messages)
                .await
                .map_err(memory_error_to_chat_error)
        })
    }
}

fn memory_error_to_chat_error(error: MemoryError) -> ChatError {
    ChatError::store(error.message).with_phase(ChatErrorPhase::Storage)
}
