//! Production-friendly observability hooks for provider, tool, and harness phases.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Duration;

use fharness::{HarnessError, HarnessPhase, HarnessRuntimeHooks};
use fprovider::{ProviderError, ProviderId, ProviderOperationHooks};
use ftooling::{ToolError, ToolExecutionContext, ToolExecutionResult, ToolRuntimeHooks};

pub mod prelude {
    pub use crate::{
        MetricsObservabilityHooks, SafeHarnessHooks, SafeProviderHooks, SafeToolHooks,
        TracingObservabilityHooks,
    };
}

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

pub struct SafeProviderHooks<H> {
    inner: H,
}

impl<H> SafeProviderHooks<H> {
    pub fn new(inner: H) -> Self {
        Self { inner }
    }
}

impl<H> ProviderOperationHooks for SafeProviderHooks<H>
where
    H: ProviderOperationHooks,
{
    fn on_attempt_start(&self, provider: ProviderId, operation: &str, attempt: u32) {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            self.inner.on_attempt_start(provider, operation, attempt)
        }));
    }

    fn on_retry_scheduled(
        &self,
        provider: ProviderId,
        operation: &str,
        attempt: u32,
        delay: Duration,
        error: &ProviderError,
    ) {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            self.inner
                .on_retry_scheduled(provider, operation, attempt, delay, error)
        }));
    }

    fn on_success(&self, provider: ProviderId, operation: &str, attempts: u32) {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            self.inner.on_success(provider, operation, attempts)
        }));
    }

    fn on_failure(
        &self,
        provider: ProviderId,
        operation: &str,
        attempts: u32,
        error: &ProviderError,
    ) {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            self.inner.on_failure(provider, operation, attempts, error)
        }));
    }
}

pub struct SafeToolHooks<H> {
    inner: H,
}

impl<H> SafeToolHooks<H> {
    pub fn new(inner: H) -> Self {
        Self { inner }
    }
}

impl<H> ToolRuntimeHooks for SafeToolHooks<H>
where
    H: ToolRuntimeHooks,
{
    fn on_execution_start(&self, tool_call: &fprovider::ToolCall, context: &ToolExecutionContext) {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            self.inner.on_execution_start(tool_call, context)
        }));
    }

    fn on_execution_success(
        &self,
        tool_call: &fprovider::ToolCall,
        context: &ToolExecutionContext,
        result: &ToolExecutionResult,
        elapsed: Duration,
    ) {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            self.inner
                .on_execution_success(tool_call, context, result, elapsed)
        }));
    }

    fn on_execution_failure(
        &self,
        tool_call: &fprovider::ToolCall,
        context: &ToolExecutionContext,
        error: &ToolError,
        elapsed: Duration,
    ) {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            self.inner
                .on_execution_failure(tool_call, context, error, elapsed)
        }));
    }
}

pub struct SafeHarnessHooks<H> {
    inner: H,
}

impl<H> SafeHarnessHooks<H> {
    pub fn new(inner: H) -> Self {
        Self { inner }
    }
}

impl<H> HarnessRuntimeHooks for SafeHarnessHooks<H>
where
    H: HarnessRuntimeHooks,
{
    fn on_phase_start(&self, phase: HarnessPhase, session_id: &fcommon::SessionId, run_id: &str) {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            self.inner.on_phase_start(phase, session_id, run_id)
        }));
    }

    fn on_phase_success(
        &self,
        phase: HarnessPhase,
        session_id: &fcommon::SessionId,
        run_id: &str,
        elapsed: Duration,
    ) {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            self.inner
                .on_phase_success(phase, session_id, run_id, elapsed)
        }));
    }

    fn on_phase_failure(
        &self,
        phase: HarnessPhase,
        session_id: &fcommon::SessionId,
        run_id: &str,
        error: &HarnessError,
        elapsed: Duration,
    ) {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            self.inner
                .on_phase_failure(phase, session_id, run_id, error, elapsed)
        }));
    }
}
