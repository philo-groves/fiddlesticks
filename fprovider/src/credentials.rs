//! Secure in-memory credential and browser-session management.
//!
//! ```rust
//! use fprovider::{ProviderId, SecureCredentialManager};
//!
//! let manager = SecureCredentialManager::new();
//! manager
//!     .set_api_key(ProviderId::OpenAi, "sk-test-123")
//!     .expect("api key should store");
//!
//! let has_key = manager
//!     .has_credentials(ProviderId::OpenAi)
//!     .expect("lookup should succeed");
//! assert!(has_key);
//!
//! let copied = manager
//!     .with_api_key(ProviderId::OpenAi, |value| value.to_string())
//!     .expect("read should succeed");
//! assert_eq!(copied, Some("sk-test-123".to_string()));
//! ```

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime};

use crate::{ProviderError, ProviderId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialKind {
    ApiKey,
    BrowserSession,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialAccessAction {
    Set,
    Rotated,
    AccessGranted,
    AccessDenied,
    Cleared,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CredentialAccessEvent {
    pub provider: ProviderId,
    pub kind: Option<CredentialKind>,
    pub action: CredentialAccessAction,
}

pub trait CredentialAccessObserver: Send + Sync {
    fn on_event(&self, event: CredentialAccessEvent);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CredentialMetadata {
    pub created_at: SystemTime,
    pub expires_at: Option<SystemTime>,
    pub last_used_at: Option<SystemTime>,
    pub last_rotated_at: Option<SystemTime>,
    pub access_count: u64,
}

impl CredentialMetadata {
    fn new(created_at: SystemTime, expires_at: Option<SystemTime>) -> Self {
        Self {
            created_at,
            expires_at,
            last_used_at: None,
            last_rotated_at: None,
            access_count: 0,
        }
    }

    fn with_rotation(now: SystemTime, expires_at: Option<SystemTime>) -> Self {
        Self {
            created_at: now,
            expires_at,
            last_used_at: None,
            last_rotated_at: Some(now),
            access_count: 0,
        }
    }

    fn mark_used(&mut self, now: SystemTime) {
        self.last_used_at = Some(now);
        self.access_count = self.access_count.saturating_add(1);
    }

    fn is_expired(&self, now: SystemTime) -> bool {
        match self.expires_at {
            Some(expires_at) => expires_at <= now,
            None => false,
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
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

pub struct SecureCredentialManager {
    credentials: Mutex<HashMap<ProviderId, CredentialEntry>>,
    observer: Option<Arc<dyn CredentialAccessObserver>>,
}

struct CredentialEntry {
    credential: ProviderCredential,
    metadata: CredentialMetadata,
}

impl Default for SecureCredentialManager {
    fn default() -> Self {
        Self {
            credentials: Mutex::new(HashMap::new()),
            observer: None,
        }
    }
}

impl SecureCredentialManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_observer(observer: Arc<dyn CredentialAccessObserver>) -> Self {
        Self {
            credentials: Mutex::new(HashMap::new()),
            observer: Some(observer),
        }
    }

    pub fn set_api_key(
        &self,
        provider: ProviderId,
        api_key: impl Into<String>,
    ) -> Result<(), ProviderError> {
        self.set_api_key_with_expiry(provider, api_key, None)
    }

    pub fn set_api_key_with_ttl(
        &self,
        provider: ProviderId,
        api_key: impl Into<String>,
        ttl: Duration,
    ) -> Result<(), ProviderError> {
        if ttl.is_zero() {
            return Err(ProviderError::authentication(
                "api key ttl must be greater than zero",
            ));
        }

        let now = SystemTime::now();
        let expires_at = now.checked_add(ttl).ok_or_else(|| {
            ProviderError::authentication("api key ttl is out of supported range")
        })?;

        self.set_api_key_with_expiry(provider, api_key, Some(expires_at))
    }

    pub fn rotate_api_key(
        &self,
        provider: ProviderId,
        api_key: impl Into<String>,
        ttl: Option<Duration>,
    ) -> Result<(), ProviderError> {
        match ttl {
            Some(ttl) => self.set_api_key_with_ttl(provider, api_key, ttl),
            None => self.set_api_key(provider, api_key),
        }
    }

    fn set_api_key_with_expiry(
        &self,
        provider: ProviderId,
        api_key: impl Into<String>,
        expires_at: Option<SystemTime>,
    ) -> Result<(), ProviderError> {
        let api_key = SecretString::new(api_key);
        if api_key.is_empty() {
            return Err(ProviderError::authentication("api key must not be empty"));
        }

        let action =
            self.insert_credential(provider, ProviderCredential::ApiKey(api_key), expires_at)?;
        self.emit(CredentialAccessEvent {
            provider,
            kind: Some(CredentialKind::ApiKey),
            action,
        });
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

        let action = self.insert_credential(
            provider,
            ProviderCredential::BrowserSession(session),
            expires_at,
        )?;
        self.emit(CredentialAccessEvent {
            provider,
            kind: Some(CredentialKind::BrowserSession),
            action,
        });
        Ok(())
    }

    pub fn has_credentials(&self, provider: ProviderId) -> Result<bool, ProviderError> {
        let mut credentials = self.credentials_mut()?;
        if Self::remove_if_expired(&mut credentials, provider, SystemTime::now()) {
            return Ok(false);
        }

        Ok(credentials.contains_key(&provider))
    }

    pub fn credential_kind(
        &self,
        provider: ProviderId,
    ) -> Result<Option<CredentialKind>, ProviderError> {
        let mut credentials = self.credentials_mut()?;
        if Self::remove_if_expired(&mut credentials, provider, SystemTime::now()) {
            return Ok(None);
        }

        Ok(credentials
            .get(&provider)
            .map(|entry| entry.credential.kind()))
    }

    pub fn credential_metadata(
        &self,
        provider: ProviderId,
    ) -> Result<Option<CredentialMetadata>, ProviderError> {
        let mut credentials = self.credentials_mut()?;
        if Self::remove_if_expired(&mut credentials, provider, SystemTime::now()) {
            return Ok(None);
        }

        Ok(credentials.get(&provider).map(|entry| entry.metadata))
    }

    pub fn with_api_key<R>(
        &self,
        provider: ProviderId,
        f: impl FnOnce(&str) -> R,
    ) -> Result<Option<R>, ProviderError> {
        let now = SystemTime::now();
        let mut credentials = self.credentials_mut()?;

        if Self::remove_if_expired(&mut credentials, provider, now) {
            drop(credentials);
            self.emit(CredentialAccessEvent {
                provider,
                kind: Some(CredentialKind::ApiKey),
                action: CredentialAccessAction::AccessDenied,
            });
            return Ok(None);
        }

        let output = match credentials.get_mut(&provider) {
            Some(entry) => {
                if !matches!(entry.credential, ProviderCredential::ApiKey(_)) {
                    None
                } else {
                    entry.metadata.mark_used(now);
                    match &entry.credential {
                        ProviderCredential::ApiKey(secret) => Some(f(secret.expose())),
                        _ => None,
                    }
                }
            }
            None => None,
        };

        let action = if output.is_some() {
            CredentialAccessAction::AccessGranted
        } else {
            CredentialAccessAction::AccessDenied
        };

        drop(credentials);
        self.emit(CredentialAccessEvent {
            provider,
            kind: Some(CredentialKind::ApiKey),
            action,
        });

        Ok(output)
    }

    pub fn api_key(&self, provider: ProviderId) -> Result<Option<SecretString>, ProviderError> {
        self.with_api_key(provider, |value| SecretString::new(value.to_string()))
    }

    pub fn with_browser_session<R>(
        &self,
        provider: ProviderId,
        f: impl FnOnce(&BrowserLoginSession) -> R,
    ) -> Result<Option<R>, ProviderError> {
        let now = SystemTime::now();
        let mut credentials = self.credentials_mut()?;

        if Self::remove_if_expired(&mut credentials, provider, now) {
            drop(credentials);
            self.emit(CredentialAccessEvent {
                provider,
                kind: Some(CredentialKind::BrowserSession),
                action: CredentialAccessAction::AccessDenied,
            });
            return Ok(None);
        }

        let output = match credentials.get_mut(&provider) {
            Some(entry) => {
                if !matches!(entry.credential, ProviderCredential::BrowserSession(_)) {
                    None
                } else {
                    entry.metadata.mark_used(now);
                    match &entry.credential {
                        ProviderCredential::BrowserSession(session) => Some(f(session)),
                        _ => None,
                    }
                }
            }
            None => None,
        };

        let action = if output.is_some() {
            CredentialAccessAction::AccessGranted
        } else {
            CredentialAccessAction::AccessDenied
        };

        drop(credentials);
        self.emit(CredentialAccessEvent {
            provider,
            kind: Some(CredentialKind::BrowserSession),
            action,
        });

        Ok(output)
    }

    pub fn browser_session(
        &self,
        provider: ProviderId,
    ) -> Result<Option<BrowserLoginSession>, ProviderError> {
        self.with_browser_session(provider, |session| BrowserLoginSession {
            session_token: session.session_token.clone(),
            expires_at: session.expires_at,
        })
    }

    pub fn clear(&self, provider: ProviderId) -> Result<bool, ProviderError> {
        self.revoke(provider)
    }

    pub fn revoke(&self, provider: ProviderId) -> Result<bool, ProviderError> {
        let mut credentials = self.credentials_mut()?;
        let removed = credentials.remove(&provider);
        let kind = removed.as_ref().map(|entry| entry.credential.kind());
        let had_value = removed.is_some();
        drop(credentials);

        if had_value {
            self.emit(CredentialAccessEvent {
                provider,
                kind,
                action: CredentialAccessAction::Cleared,
            });
        }

        Ok(had_value)
    }

    fn credentials_mut(
        &self,
    ) -> Result<MutexGuard<'_, HashMap<ProviderId, CredentialEntry>>, ProviderError> {
        self.credentials
            .lock()
            .map_err(|_| ProviderError::other("credential manager lock poisoned"))
    }

    fn insert_credential(
        &self,
        provider: ProviderId,
        credential: ProviderCredential,
        expires_at: Option<SystemTime>,
    ) -> Result<CredentialAccessAction, ProviderError> {
        let now = SystemTime::now();
        let mut credentials = self.credentials_mut()?;
        let action = if credentials.contains_key(&provider) {
            CredentialAccessAction::Rotated
        } else {
            CredentialAccessAction::Set
        };

        let metadata = match action {
            CredentialAccessAction::Set => CredentialMetadata::new(now, expires_at),
            CredentialAccessAction::Rotated => CredentialMetadata::with_rotation(now, expires_at),
            _ => unreachable!(),
        };

        credentials.insert(
            provider,
            CredentialEntry {
                credential,
                metadata,
            },
        );

        Ok(action)
    }

    fn remove_if_expired(
        credentials: &mut HashMap<ProviderId, CredentialEntry>,
        provider: ProviderId,
        now: SystemTime,
    ) -> bool {
        let is_expired = credentials
            .get(&provider)
            .map(|entry| entry.metadata.is_expired(now))
            .unwrap_or(false);

        if is_expired {
            credentials.remove(&provider);
            return true;
        }

        false
    }

    fn emit(&self, event: CredentialAccessEvent) {
        if let Some(observer) = &self.observer {
            observer.on_event(event);
        }
    }
}
