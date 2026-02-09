//! Feature-gated provider adapter modules.

#[cfg(feature = "provider-opencode-zen")]
pub mod opencode_zen;

#[cfg(feature = "provider-openai")]
pub mod openai;

#[cfg(feature = "provider-anthropic")]
pub mod anthropic;
