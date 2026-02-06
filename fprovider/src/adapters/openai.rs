use std::time::SystemTime;

use crate::{ProviderError, ProviderId, SecureCredentialManager};

impl SecureCredentialManager {
    pub fn set_openai_api_key(&self, api_key: impl Into<String>) -> Result<(), ProviderError> {
        let api_key = api_key.into();
        if !api_key.starts_with("sk-") {
            return Err(ProviderError::authentication(
                "OpenAI API key must start with 'sk-'",
            ));
        }

        self.set_api_key(ProviderId::OpenAi, api_key)
    }

    pub fn set_openai_browser_session(
        &self,
        session_token: impl Into<String>,
        expires_at: Option<SystemTime>,
    ) -> Result<(), ProviderError> {
        self.set_browser_session(ProviderId::OpenAi, session_token, expires_at)
    }
}
