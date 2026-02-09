//! Production-friendly observability hooks for provider, tool, and harness phases.
//!
//! ```rust
//! use fobserve::{MetricsObservabilityHooks, SafeProviderHooks, TracingObservabilityHooks};
//!
//! let _provider_hooks = SafeProviderHooks::new(TracingObservabilityHooks);
//! let _metrics = MetricsObservabilityHooks;
//! ```

mod metrics_hooks;
mod safe_hooks;
mod tracing_hooks;

pub use metrics_hooks::MetricsObservabilityHooks;
pub use safe_hooks::{SafeHarnessHooks, SafeProviderHooks, SafeToolHooks};
pub use tracing_hooks::TracingObservabilityHooks;

pub mod prelude {
    pub use crate::{
        MetricsObservabilityHooks, SafeHarnessHooks, SafeProviderHooks, SafeToolHooks,
        TracingObservabilityHooks,
    };
}

#[cfg(test)]
mod tests;
