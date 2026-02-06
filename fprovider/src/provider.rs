//! Core async provider trait and future alias.

use fcommon::BoxFuture;

use crate::{BoxedEventStream, ModelRequest, ModelResponse, ProviderError, ProviderId};

pub type ProviderFuture<'a, T> = BoxFuture<'a, T>;

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
