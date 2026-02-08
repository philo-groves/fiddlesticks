//! Feature-gated provider adapter modules.

#[cfg(feature = "provider-opencode-zen")]
pub mod opencode_zen;

#[cfg(feature = "provider-openai")]
pub mod openai;

#[cfg(any(feature = "provider-anthropic", feature = "provider-claude"))]
pub mod anthropic;

#[cfg(feature = "provider-claude")]
pub mod claude;
