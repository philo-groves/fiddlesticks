//! Conversation storage contracts and a basic in-memory implementation.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;

use fprovider::Message;

use crate::ChatError;

pub type ChatFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait ConversationStore: Send + Sync {
    fn load_messages<'a>(
        &'a self,
        session_id: &'a str,
    ) -> ChatFuture<'a, Result<Vec<Message>, ChatError>>;

    fn append_messages<'a>(
        &'a self,
        session_id: &'a str,
        messages: Vec<Message>,
    ) -> ChatFuture<'a, Result<(), ChatError>>;
}

#[derive(Debug, Default)]
pub struct InMemoryConversationStore {
    sessions: Mutex<HashMap<String, Vec<Message>>>,
}

impl InMemoryConversationStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ConversationStore for InMemoryConversationStore {
    fn load_messages<'a>(
        &'a self,
        session_id: &'a str,
    ) -> ChatFuture<'a, Result<Vec<Message>, ChatError>> {
        Box::pin(async move {
            let sessions = self
                .sessions
                .lock()
                .map_err(|_| ChatError::store("conversation store lock poisoned"))?;

            Ok(sessions.get(session_id).cloned().unwrap_or_default())
        })
    }

    fn append_messages<'a>(
        &'a self,
        session_id: &'a str,
        messages: Vec<Message>,
    ) -> ChatFuture<'a, Result<(), ChatError>> {
        Box::pin(async move {
            let mut sessions = self
                .sessions
                .lock()
                .map_err(|_| ChatError::store("conversation store lock poisoned"))?;

            sessions
                .entry(session_id.to_string())
                .or_default()
                .extend(messages);

            Ok(())
        })
    }
}
