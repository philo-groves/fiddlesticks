use std::future::Future;
use std::pin::Pin;

use crate::{BoxedEventStream, ModelRequest, ModelResponse, ProviderError, ProviderId};

pub type ProviderFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait ModelProvider: Send + Sync {
    fn id(&self) -> ProviderId;

    fn complete<'a>(
        &'a self,
        request: ModelRequest,
    ) -> ProviderFuture<'a, Result<ModelResponse, ProviderError>>;

    fn stream<'a>(
        &'a self,
        request: ModelRequest,
    ) -> ProviderFuture<'a, Result<BoxedEventStream<'a>, ProviderError>>;
}
