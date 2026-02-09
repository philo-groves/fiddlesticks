//! OpenAI-specific credential helpers and auth resolution policy.

use crate::{ProviderError, ProviderId, SecureCredentialManager};

use super::types::OpenAiAuth;

impl SecureCredentialManager {
    /// Stores an OpenAI API key for provider-authenticated requests.
    ///
    /// OpenAI keys are expected to start with `sk-`.
    pub fn set_openai_api_key(&self, api_key: impl Into<String>) -> Result<(), ProviderError> {
        let api_key = api_key.into();
        if !api_key.starts_with("sk-") {
            return Err(ProviderError::authentication(
                "OpenAI API key must start with 'sk-'",
            ));
        }

        self.set_api_key(ProviderId::OpenAi, api_key)
    }
}

/// Resolves OpenAI authentication from API key credentials only.
pub(crate) fn resolve_openai_auth(
    credentials: &SecureCredentialManager,
) -> Result<OpenAiAuth, ProviderError> {
    if let Some(api_key) = credentials.api_key(ProviderId::OpenAi)? {
        return Ok(OpenAiAuth::ApiKey(api_key));
    }

    Err(ProviderError::authentication(
        "no OpenAI API key configured",
    ))
}
