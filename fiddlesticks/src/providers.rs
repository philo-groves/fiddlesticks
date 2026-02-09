//! Stable provider construction surface for facade consumers.
//!
//! ```rust
//! use std::time::Duration;
//!
//! use fiddlesticks::{ProviderBuildConfig, ProviderId};
//!
//! let config = ProviderBuildConfig::new(ProviderId::OpenAi, "test-key")
//!     .with_timeout(Duration::from_secs(30));
//!
//! assert_eq!(config.provider_id, ProviderId::OpenAi);
//! assert_eq!(config.timeout, Duration::from_secs(30));
//! ```

use std::sync::Arc;
use std::time::Duration;

use reqwest::Client;

use crate::{ModelProvider, ProviderError, ProviderId, SecretString, SecureCredentialManager};

#[derive(Debug, Clone)]
pub struct ProviderBuildConfig {
    pub provider_id: ProviderId,
    pub api_key: SecretString,
    pub timeout: Duration,
}

impl ProviderBuildConfig {
    pub fn new(provider_id: ProviderId, api_key: impl Into<String>) -> Self {
        Self {
            provider_id,
            api_key: SecretString::new(api_key),
            timeout: Duration::from_secs(90),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

pub fn build_provider_from_api_key(
    provider_id: ProviderId,
    api_key: impl Into<String>,
) -> Result<Arc<dyn ModelProvider>, ProviderError> {
    build_provider_with_config(ProviderBuildConfig::new(provider_id, api_key))
}

/// Builds a provider from a strict [`ProviderBuildConfig`].
///
/// Empty API keys are rejected before any HTTP calls are attempted.
///
/// ```rust
/// use fiddlesticks::{ProviderBuildConfig, ProviderErrorKind, ProviderId, build_provider_with_config};
///
/// let result = build_provider_with_config(ProviderBuildConfig::new(ProviderId::OpenAi, "   "));
/// let err = result.err().expect("empty API key should be rejected");
/// assert_eq!(err.kind, ProviderErrorKind::Authentication);
/// ```
pub fn build_provider_with_config(
    config: ProviderBuildConfig,
) -> Result<Arc<dyn ModelProvider>, ProviderError> {
    let api_key = config.api_key.expose().trim().to_string();
    if api_key.is_empty() {
        return Err(ProviderError::authentication(
            "provider API key must not be empty",
        ));
    }

    let credentials = Arc::new(SecureCredentialManager::new());
    let http = Client::builder()
        .timeout(config.timeout)
        .build()
        .map_err(|err| ProviderError::transport(err.to_string()))?;

    match config.provider_id {
        ProviderId::OpenAi => build_openai_provider(credentials, api_key, http),
        ProviderId::Anthropic => build_anthropic_provider(credentials, api_key, http),
        ProviderId::OpenCodeZen => build_zen_provider(credentials, api_key, http),
    }
}

pub async fn list_models_with_api_key(
    provider_id: ProviderId,
    api_key: impl Into<String>,
) -> Result<Vec<String>, ProviderError> {
    let api_key = api_key.into();
    match provider_id {
        ProviderId::OpenCodeZen => list_zen_models(api_key).await,
        ProviderId::OpenAi | ProviderId::Anthropic => Err(ProviderError::invalid_request(
            "model listing is currently supported for OpenCode Zen only",
        )),
    }
}

#[cfg(feature = "provider-openai")]
fn build_openai_provider(
    credentials: Arc<SecureCredentialManager>,
    api_key: String,
    http: Client,
) -> Result<Arc<dyn ModelProvider>, ProviderError> {
    credentials.set_openai_api_key(api_key)?;
    let transport = Arc::new(fprovider::adapters::openai::OpenAiHttpTransport::new(http));
    Ok(Arc::new(fprovider::adapters::openai::OpenAiProvider::new(
        credentials,
        transport,
    )))
}

#[cfg(not(feature = "provider-openai"))]
fn build_openai_provider(
    _credentials: Arc<SecureCredentialManager>,
    _api_key: String,
    _http: Client,
) -> Result<Arc<dyn ModelProvider>, ProviderError> {
    Err(ProviderError::invalid_request(
        "provider-openai feature is not enabled on fiddlesticks",
    ))
}

#[cfg(feature = "provider-anthropic")]
fn build_anthropic_provider(
    credentials: Arc<SecureCredentialManager>,
    api_key: String,
    http: Client,
) -> Result<Arc<dyn ModelProvider>, ProviderError> {
    credentials.set_anthropic_api_key(api_key)?;
    let transport =
        Arc::new(fprovider::adapters::anthropic::AnthropicProvider::default_http_transport(http));
    Ok(Arc::new(
        fprovider::adapters::anthropic::AnthropicProvider::new(credentials, transport),
    ))
}

#[cfg(not(feature = "provider-anthropic"))]
fn build_anthropic_provider(
    _credentials: Arc<SecureCredentialManager>,
    _api_key: String,
    _http: Client,
) -> Result<Arc<dyn ModelProvider>, ProviderError> {
    Err(ProviderError::invalid_request(
        "provider-anthropic feature is not enabled on fiddlesticks",
    ))
}

#[cfg(feature = "provider-opencode-zen")]
fn build_zen_provider(
    credentials: Arc<SecureCredentialManager>,
    api_key: String,
    http: Client,
) -> Result<Arc<dyn ModelProvider>, ProviderError> {
    credentials.set_opencode_zen_api_key(api_key)?;
    let transport = Arc::new(
        fprovider::adapters::opencode_zen::OpenCodeZenProvider::default_http_transport(http),
    );
    Ok(Arc::new(
        fprovider::adapters::opencode_zen::OpenCodeZenProvider::new(credentials, transport),
    ))
}

#[cfg(not(feature = "provider-opencode-zen"))]
fn build_zen_provider(
    _credentials: Arc<SecureCredentialManager>,
    _api_key: String,
    _http: Client,
) -> Result<Arc<dyn ModelProvider>, ProviderError> {
    Err(ProviderError::invalid_request(
        "provider-opencode-zen feature is not enabled on fiddlesticks",
    ))
}

#[cfg(feature = "provider-opencode-zen")]
async fn list_zen_models(api_key: String) -> Result<Vec<String>, ProviderError> {
    fprovider::adapters::opencode_zen::list_zen_models_with_api_key(api_key).await
}

#[cfg(not(feature = "provider-opencode-zen"))]
async fn list_zen_models(_api_key: String) -> Result<Vec<String>, ProviderError> {
    Err(ProviderError::invalid_request(
        "provider-opencode-zen feature is not enabled on fiddlesticks",
    ))
}
