//! Metrics-based observability hooks for provider, tool runtime, and harness phases.
//!
//! ```rust
//! use fobserve::MetricsObservabilityHooks;
//! use fprovider::ProviderOperationHooks;
//!
//! fn accepts_provider_hooks(_hooks: &dyn ProviderOperationHooks) {}
//!
//! let hooks = MetricsObservabilityHooks;
//! accepts_provider_hooks(&hooks);
//! ```

use std::time::Duration;

use fharness::{HarnessError, HarnessPhase, HarnessRuntimeHooks};
use fprovider::{ProviderError, ProviderId, ProviderOperationHooks};
use ftooling::{ToolError, ToolExecutionContext, ToolExecutionResult, ToolRuntimeHooks};

#[derive(Debug, Clone, Copy, Default)]
pub struct MetricsObservabilityHooks;

impl ProviderOperationHooks for MetricsObservabilityHooks {
    fn on_attempt_start(&self, provider: ProviderId, operation: &str, _attempt: u32) {
        metrics::counter!(
            "fiddlesticks_provider_attempt_start_total",
            "provider" => provider.to_string(),
            "operation" => operation.to_string()
        )
        .increment(1);
    }

    fn on_retry_scheduled(
        &self,
        provider: ProviderId,
        operation: &str,
        _attempt: u32,
        delay: Duration,
        error: &ProviderError,
    ) {
        metrics::counter!(
            "fiddlesticks_provider_retry_scheduled_total",
            "provider" => provider.to_string(),
            "operation" => operation.to_string(),
            "error_kind" => format!("{:?}", error.kind)
        )
        .increment(1);
        metrics::histogram!(
            "fiddlesticks_provider_retry_delay_seconds",
            "provider" => provider.to_string(),
            "operation" => operation.to_string()
        )
        .record(delay.as_secs_f64());
    }

    fn on_success(&self, provider: ProviderId, operation: &str, attempts: u32) {
        metrics::counter!(
            "fiddlesticks_provider_success_total",
            "provider" => provider.to_string(),
            "operation" => operation.to_string()
        )
        .increment(1);
        metrics::histogram!(
            "fiddlesticks_provider_attempts_per_success",
            "provider" => provider.to_string(),
            "operation" => operation.to_string()
        )
        .record(attempts as f64);
    }

    fn on_failure(
        &self,
        provider: ProviderId,
        operation: &str,
        attempts: u32,
        error: &ProviderError,
    ) {
        metrics::counter!(
            "fiddlesticks_provider_failure_total",
            "provider" => provider.to_string(),
            "operation" => operation.to_string(),
            "error_kind" => format!("{:?}", error.kind)
        )
        .increment(1);
        metrics::histogram!(
            "fiddlesticks_provider_attempts_per_failure",
            "provider" => provider.to_string(),
            "operation" => operation.to_string()
        )
        .record(attempts as f64);
    }
}

impl ToolRuntimeHooks for MetricsObservabilityHooks {
    fn on_execution_start(&self, tool_call: &fprovider::ToolCall, _context: &ToolExecutionContext) {
        metrics::counter!(
            "fiddlesticks_tool_execution_start_total",
            "tool_name" => tool_call.name.clone()
        )
        .increment(1);
    }

    fn on_execution_success(
        &self,
        tool_call: &fprovider::ToolCall,
        _context: &ToolExecutionContext,
        _result: &ToolExecutionResult,
        elapsed: Duration,
    ) {
        metrics::counter!(
            "fiddlesticks_tool_execution_success_total",
            "tool_name" => tool_call.name.clone()
        )
        .increment(1);
        metrics::histogram!(
            "fiddlesticks_tool_execution_duration_seconds",
            "tool_name" => tool_call.name.clone(),
            "status" => "success"
        )
        .record(elapsed.as_secs_f64());
    }

    fn on_execution_failure(
        &self,
        tool_call: &fprovider::ToolCall,
        _context: &ToolExecutionContext,
        error: &ToolError,
        elapsed: Duration,
    ) {
        metrics::counter!(
            "fiddlesticks_tool_execution_failure_total",
            "tool_name" => tool_call.name.clone(),
            "error_kind" => format!("{:?}", error.kind)
        )
        .increment(1);
        metrics::histogram!(
            "fiddlesticks_tool_execution_duration_seconds",
            "tool_name" => tool_call.name.clone(),
            "status" => "failure"
        )
        .record(elapsed.as_secs_f64());
    }
}

impl HarnessRuntimeHooks for MetricsObservabilityHooks {
    fn on_phase_start(&self, phase: HarnessPhase, _session_id: &fcommon::SessionId, _run_id: &str) {
        metrics::counter!("fiddlesticks_harness_phase_start_total", "phase" => format!("{:?}", phase))
            .increment(1);
    }

    fn on_phase_success(
        &self,
        phase: HarnessPhase,
        _session_id: &fcommon::SessionId,
        _run_id: &str,
        elapsed: Duration,
    ) {
        metrics::counter!(
            "fiddlesticks_harness_phase_success_total",
            "phase" => format!("{:?}", phase)
        )
        .increment(1);
        metrics::histogram!(
            "fiddlesticks_harness_phase_duration_seconds",
            "phase" => format!("{:?}", phase),
            "status" => "success"
        )
        .record(elapsed.as_secs_f64());
    }

    fn on_phase_failure(
        &self,
        phase: HarnessPhase,
        _session_id: &fcommon::SessionId,
        _run_id: &str,
        error: &HarnessError,
        elapsed: Duration,
    ) {
        metrics::counter!(
            "fiddlesticks_harness_phase_failure_total",
            "phase" => format!("{:?}", phase),
            "error_kind" => format!("{:?}", error.kind)
        )
        .increment(1);
        metrics::histogram!(
            "fiddlesticks_harness_phase_duration_seconds",
            "phase" => format!("{:?}", phase),
            "status" => "failure"
        )
        .record(elapsed.as_secs_f64());
    }
}
