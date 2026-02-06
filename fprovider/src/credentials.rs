//! Secure in-memory credential and browser-session management.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};
use std::time::SystemTime;

use crate::{ProviderError, ProviderId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialKind {
    ApiKey,
    BrowserSession,
}

#[derive(PartialEq, Eq)]
pub struct SecretString {
    value: String,
}

impl SecretString {
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
        }
    }

    pub fn expose(&self) -> &str {
        self.value.as_str()
    }

    pub fn is_empty(&self) -> bool {
        self.value.is_empty()
    }
}

impl std::fmt::Debug for SecretString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[REDACTED]")
    }
}

impl Drop for SecretString {
    fn drop(&mut self) {
        unsafe {
            self.value.as_mut_vec().fill(0);
        }
    }
}

pub struct BrowserLoginSession {
    pub session_token: SecretString,
    pub expires_at: Option<SystemTime>,
}

impl BrowserLoginSession {
    pub fn new(session_token: impl Into<String>, expires_at: Option<SystemTime>) -> Self {
        Self {
            session_token: SecretString::new(session_token),
            expires_at,
        }
    }
}

impl std::fmt::Debug for BrowserLoginSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BrowserLoginSession")
            .field("session_token", &"[REDACTED]")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

pub enum ProviderCredential {
    ApiKey(SecretString),
    BrowserSession(BrowserLoginSession),
}

impl ProviderCredential {
    pub fn kind(&self) -> CredentialKind {
        match self {
            Self::ApiKey(_) => CredentialKind::ApiKey,
            Self::BrowserSession(_) => CredentialKind::BrowserSession,
        }
    }
}

impl std::fmt::Debug for ProviderCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ApiKey(_) => f.write_str("ProviderCredential::ApiKey([REDACTED])"),
            Self::BrowserSession(session) => f
                .debug_tuple("ProviderCredential::BrowserSession")
                .field(session)
                .finish(),
        }
    }
}

#[derive(Default)]
pub struct SecureCredentialManager {
    credentials: Mutex<HashMap<ProviderId, ProviderCredential>>,
}

impl SecureCredentialManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_api_key(
        &self,
        provider: ProviderId,
        api_key: impl Into<String>,
    ) -> Result<(), ProviderError> {
        let api_key = SecretString::new(api_key);
        if api_key.is_empty() {
            return Err(ProviderError::authentication("api key must not be empty"));
        }

        self.credentials_mut()?
            .insert(provider, ProviderCredential::ApiKey(api_key));
        Ok(())
    }

    pub fn set_browser_session(
        &self,
        provider: ProviderId,
        session_token: impl Into<String>,
        expires_at: Option<SystemTime>,
    ) -> Result<(), ProviderError> {
        let session = BrowserLoginSession::new(session_token, expires_at);
        if session.session_token.is_empty() {
            return Err(ProviderError::authentication(
                "browser session token must not be empty",
            ));
        }

        self.credentials_mut()?
            .insert(provider, ProviderCredential::BrowserSession(session));
        Ok(())
    }

    pub fn has_credentials(&self, provider: ProviderId) -> Result<bool, ProviderError> {
        Ok(self.credentials_ref()?.contains_key(&provider))
    }

    pub fn credential_kind(
        &self,
        provider: ProviderId,
    ) -> Result<Option<CredentialKind>, ProviderError> {
        Ok(self
            .credentials_ref()?
            .get(&provider)
            .map(|entry| entry.kind()))
    }

    pub fn with_api_key<R>(
        &self,
        provider: ProviderId,
        f: impl FnOnce(&str) -> R,
    ) -> Result<Option<R>, ProviderError> {
        let credentials = self.credentials_ref()?;
        let output = match credentials.get(&provider) {
            Some(ProviderCredential::ApiKey(secret)) => Some(f(secret.expose())),
            _ => None,
        };

        Ok(output)
    }

    pub fn with_browser_session<R>(
        &self,
        provider: ProviderId,
        f: impl FnOnce(&BrowserLoginSession) -> R,
    ) -> Result<Option<R>, ProviderError> {
        let credentials = self.credentials_ref()?;
        let output = match credentials.get(&provider) {
            Some(ProviderCredential::BrowserSession(session)) => Some(f(session)),
            _ => None,
        };

        Ok(output)
    }

    pub fn clear(&self, provider: ProviderId) -> Result<bool, ProviderError> {
        Ok(self.credentials_mut()?.remove(&provider).is_some())
    }

    fn credentials_ref(
        &self,
    ) -> Result<MutexGuard<'_, HashMap<ProviderId, ProviderCredential>>, ProviderError> {
        self.credentials
            .lock()
            .map_err(|_| ProviderError::other("credential manager lock poisoned"))
    }

    fn credentials_mut(
        &self,
    ) -> Result<MutexGuard<'_, HashMap<ProviderId, ProviderCredential>>, ProviderError> {
        self.credentials
            .lock()
            .map_err(|_| ProviderError::other("credential manager lock poisoned"))
    }
}
