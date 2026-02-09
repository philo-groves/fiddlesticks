use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Duration;

use fharness::{HarnessError, HarnessPhase, HarnessRuntimeHooks};
use fprovider::{ProviderError, ProviderId, ProviderOperationHooks};
use ftooling::{ToolError, ToolExecutionContext, ToolExecutionResult, ToolRuntimeHooks};

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
