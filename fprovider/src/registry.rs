//! Provider registry for runtime provider lookup and swapping.
//!
//! ```rust
//! use fprovider::ProviderRegistry;
//!
//! let registry = ProviderRegistry::new();
//! assert!(registry.is_empty());
//! assert_eq!(registry.len(), 0);
//! ```

use std::sync::Arc;

use fcommon::Registry;

use crate::{ModelProvider, ProviderId};

#[derive(Default)]
pub struct ProviderRegistry {
    providers: Registry<ProviderId, Arc<dyn ModelProvider>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<P>(&mut self, provider: P)
    where
        P: ModelProvider + 'static,
    {
        self.providers.insert(provider.id(), Arc::new(provider));
    }

    pub fn get(&self, provider_id: ProviderId) -> Option<Arc<dyn ModelProvider>> {
        self.providers.get(&provider_id).cloned()
    }

    pub fn remove(&mut self, provider_id: ProviderId) -> Option<Arc<dyn ModelProvider>> {
        self.providers.remove(&provider_id)
    }

    pub fn contains(&self, provider_id: ProviderId) -> bool {
        self.providers.contains_key(&provider_id)
    }

    pub fn len(&self) -> usize {
        self.providers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }
}
