use std::time::SystemTime;

use crate::{BrowserLoginSession, ProviderError, ProviderId, SecureCredentialManager};

use super::types::OpenAiAuth;

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

pub(crate) fn resolve_openai_auth(
    credentials: &SecureCredentialManager,
) -> Result<OpenAiAuth, ProviderError> {
    if let Some(api_key) =
        credentials.with_api_key(ProviderId::OpenAi, |value| value.to_string())?
    {
        return Ok(OpenAiAuth::ApiKey(api_key));
    }

    if let Some(session) = credentials.with_browser_session(ProviderId::OpenAi, clone_session)? {
        if let Some(expires_at) = session.expires_at {
            if expires_at <= SystemTime::now() {
                return Err(ProviderError::authentication(
                    "OpenAI browser session has expired",
                ));
            }
        }

        return Ok(OpenAiAuth::BrowserSession(
            session.session_token.expose().to_string(),
        ));
    }

    Err(ProviderError::authentication(
        "no OpenAI credentials configured",
    ))
}

fn clone_session(session: &BrowserLoginSession) -> BrowserLoginSession {
    BrowserLoginSession::new(session.session_token.expose(), session.expires_at)
}
