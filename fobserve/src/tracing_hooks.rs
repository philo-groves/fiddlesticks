//! Tracing-based observability hooks for provider, tool runtime, and harness phases.
//!
//! ```rust
//! use fobserve::TracingObservabilityHooks;
//! use fharness::HarnessRuntimeHooks;
//!
//! fn accepts_harness_hooks(_hooks: &dyn HarnessRuntimeHooks) {}
//!
//! let hooks = TracingObservabilityHooks;
//! accepts_harness_hooks(&hooks);
//! ```

use std::time::Duration;

use fharness::{HarnessError, HarnessPhase, HarnessRuntimeHooks};
use fprovider::{ProviderError, ProviderId, ProviderOperationHooks};
use ftooling::{ToolError, ToolExecutionContext, ToolExecutionResult, ToolRuntimeHooks};

#[derive(Debug, Clone, Copy, Default)]
pub struct TracingObservabilityHooks;

impl ProviderOperationHooks for TracingObservabilityHooks {
    fn on_attempt_start(&self, provider: ProviderId, operation: &str, attempt: u32) {
        tracing::info!(
            phase = "provider",
            event = "attempt_start",
            provider = %provider,
            operation,
            attempt
        );
    }

    fn on_retry_scheduled(
        &self,
        provider: ProviderId,
        operation: &str,
        attempt: u32,
        delay: Duration,
        error: &ProviderError,
    ) {
        tracing::warn!(
            phase = "provider",
            event = "retry_scheduled",
            provider = %provider,
            operation,
            attempt,
            delay_ms = delay.as_millis() as u64,
            error_kind = ?error.kind,
            retryable = error.retryable,
            error = %error
        );
    }

    fn on_success(&self, provider: ProviderId, operation: &str, attempts: u32) {
        tracing::info!(
            phase = "provider",
            event = "success",
            provider = %provider,
            operation,
            attempts
        );
    }

    fn on_failure(
        &self,
        provider: ProviderId,
        operation: &str,
        attempts: u32,
        error: &ProviderError,
    ) {
        tracing::error!(
            phase = "provider",
            event = "failure",
            provider = %provider,
            operation,
            attempts,
            error_kind = ?error.kind,
            retryable = error.retryable,
            error = %error
        );
    }
}

impl ToolRuntimeHooks for TracingObservabilityHooks {
    fn on_execution_start(&self, tool_call: &fprovider::ToolCall, context: &ToolExecutionContext) {
        tracing::info!(
            phase = "tool",
            event = "execution_start",
            tool_name = tool_call.name,
            tool_call_id = tool_call.id,
            session_id = %context.session_id,
            trace_id = context.trace_id.as_ref().map(|id| id.as_str())
        );
    }

    fn on_execution_success(
        &self,
        tool_call: &fprovider::ToolCall,
        context: &ToolExecutionContext,
        _result: &ToolExecutionResult,
        elapsed: Duration,
    ) {
        tracing::info!(
            phase = "tool",
            event = "execution_success",
            tool_name = tool_call.name,
            tool_call_id = tool_call.id,
            session_id = %context.session_id,
            trace_id = context.trace_id.as_ref().map(|id| id.as_str()),
            elapsed_ms = elapsed.as_millis() as u64
        );
    }

    fn on_execution_failure(
        &self,
        tool_call: &fprovider::ToolCall,
        context: &ToolExecutionContext,
        error: &ToolError,
        elapsed: Duration,
    ) {
        tracing::error!(
            phase = "tool",
            event = "execution_failure",
            tool_name = tool_call.name,
            tool_call_id = tool_call.id,
            session_id = %context.session_id,
            trace_id = context.trace_id.as_ref().map(|id| id.as_str()),
            elapsed_ms = elapsed.as_millis() as u64,
            error_kind = ?error.kind,
            retryable = error.retryable,
            error = %error
        );
    }
}

impl HarnessRuntimeHooks for TracingObservabilityHooks {
    fn on_phase_start(&self, phase: HarnessPhase, session_id: &fcommon::SessionId, run_id: &str) {
        tracing::info!(
            phase = "harness",
            event = "phase_start",
            harness_phase = ?phase,
            session_id = %session_id,
            run_id
        );
    }

    fn on_phase_success(
        &self,
        phase: HarnessPhase,
        session_id: &fcommon::SessionId,
        run_id: &str,
        elapsed: Duration,
    ) {
        tracing::info!(
            phase = "harness",
            event = "phase_success",
            harness_phase = ?phase,
            session_id = %session_id,
            run_id,
            elapsed_ms = elapsed.as_millis() as u64
        );
    }

    fn on_phase_failure(
        &self,
        phase: HarnessPhase,
        session_id: &fcommon::SessionId,
        run_id: &str,
        error: &HarnessError,
        elapsed: Duration,
    ) {
        tracing::error!(
            phase = "harness",
            event = "phase_failure",
            harness_phase = ?phase,
            session_id = %session_id,
            run_id,
            elapsed_ms = elapsed.as_millis() as u64,
            error_kind = ?error.kind,
            error = %error
        );
    }
}
