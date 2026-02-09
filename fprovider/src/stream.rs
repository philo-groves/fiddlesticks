//! Streaming event contracts and in-memory stream utilities.
//!
//! ```rust
//! use fprovider::{BoxedEventStream, StreamEvent, VecEventStream};
//!
//! let stream = VecEventStream::new(vec![Ok(StreamEvent::TextDelta("hello".into()))]);
//! let _boxed: BoxedEventStream<'static> = Box::pin(stream);
//! ```

use std::collections::VecDeque;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures_core::Stream;

use crate::{Message, ModelResponse, ProviderError, ToolCall};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEvent {
    TextDelta(String),
    ToolCallDelta(ToolCall),
    MessageComplete(Message),
    ResponseComplete(ModelResponse),
}

/// Provider stream contract.
///
/// Invariants for consumers:
/// - Events are emitted in source order.
/// - `TextDelta` and `ToolCallDelta` may appear zero or more times.
/// - `MessageComplete` and `ResponseComplete` are terminal milestones and, when present,
///   arrive after all related deltas.
/// - Once the stream yields `None`, it must not yield additional items.
pub trait ModelEventStream: Stream<Item = Result<StreamEvent, ProviderError>> + Send {}

impl<T> ModelEventStream for T where T: Stream<Item = Result<StreamEvent, ProviderError>> + Send {}

pub type BoxedEventStream<'a> = Pin<Box<dyn ModelEventStream + 'a>>;

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

impl Stream for VecEventStream {
    type Item = Result<StreamEvent, ProviderError>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<StreamEvent, ProviderError>>> {
        Poll::Ready(self.events.pop_front())
    }
}
