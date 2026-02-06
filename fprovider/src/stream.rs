use std::collections::VecDeque;
use std::pin::Pin;
use std::task::{Context, Poll};

use crate::{Message, ModelResponse, ProviderError, ToolCall};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEvent {
    TextDelta(String),
    ToolCallDelta(ToolCall),
    MessageComplete(Message),
    ResponseComplete(ModelResponse),
}

pub trait ModelEventStream {
    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<StreamEvent, ProviderError>>>;
}

pub type BoxedEventStream<'a> = Pin<Box<dyn ModelEventStream + Send + 'a>>;

#[derive(Debug)]
pub struct VecEventStream {
    events: VecDeque<Result<StreamEvent, ProviderError>>,
}

impl VecEventStream {
    pub fn new(events: Vec<Result<StreamEvent, ProviderError>>) -> Self {
        Self {
            events: events.into(),
        }
    }
}

impl ModelEventStream for VecEventStream {
    fn poll_next(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<StreamEvent, ProviderError>>> {
        Poll::Ready(self.events.pop_front())
    }
}
