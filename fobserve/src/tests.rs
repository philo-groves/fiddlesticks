use std::sync::{Arc, Mutex};
use std::time::Duration;

use fcommon::SessionId;
use fharness::{HarnessError, HarnessPhase, HarnessRuntimeHooks};
use fprovider::{ProviderError, ProviderId, ProviderOperationHooks, ToolCall};
use ftooling::{ToolError, ToolExecutionContext, ToolExecutionResult, ToolRuntimeHooks};

use crate::{
    MetricsObservabilityHooks, SafeHarnessHooks, SafeProviderHooks, SafeToolHooks,
    TracingObservabilityHooks,
};

fn sample_tool_call() -> ToolCall {
    ToolCall {
        id: "call-1".to_string(),
        name: "echo".to_string(),
        arguments: "{}".to_string(),
    }
}

fn sample_tool_context() -> ToolExecutionContext {
    ToolExecutionContext::new("session-1").with_trace_id("trace-1")
}

#[test]
fn tracing_hooks_smoke_test_all_callbacks() {
    let hooks = TracingObservabilityHooks;
    let provider_error = ProviderError::timeout("provider timeout");
    let tool_error = ToolError::execution("tool failed");
    let harness_error = HarnessError::validation("validation failed");

    hooks.on_attempt_start(ProviderId::OpenAi, "complete", 1);
    hooks.on_retry_scheduled(
        ProviderId::OpenAi,
        "complete",
        1,
        Duration::from_millis(10),
        &provider_error,
    );
    hooks.on_success(ProviderId::OpenAi, "complete", 2);
    hooks.on_failure(ProviderId::OpenAi, "complete", 2, &provider_error);

    hooks.on_execution_start(&sample_tool_call(), &sample_tool_context());
    hooks.on_execution_success(
        &sample_tool_call(),
        &sample_tool_context(),
        &ToolExecutionResult::new("call-1", "ok"),
        Duration::from_millis(20),
    );
    hooks.on_execution_failure(
        &sample_tool_call(),
        &sample_tool_context(),
        &tool_error,
        Duration::from_millis(20),
    );

    hooks.on_phase_start(
        HarnessPhase::Initializer,
        &SessionId::from("session-1"),
        "run-1",
    );
    hooks.on_phase_success(
        HarnessPhase::TaskIteration,
        &SessionId::from("session-1"),
        "run-2",
        Duration::from_millis(30),
    );
    hooks.on_phase_failure(
        HarnessPhase::TaskIteration,
        &SessionId::from("session-1"),
        "run-3",
        &harness_error,
        Duration::from_millis(30),
    );
}

#[test]
fn metrics_hooks_smoke_test_all_callbacks() {
    let hooks = MetricsObservabilityHooks;
    let provider_error = ProviderError::timeout("provider timeout");
    let tool_error = ToolError::execution("tool failed");
    let harness_error = HarnessError::validation("validation failed");

    hooks.on_attempt_start(ProviderId::OpenAi, "complete", 1);
    hooks.on_retry_scheduled(
        ProviderId::OpenAi,
        "complete",
        1,
        Duration::from_millis(10),
        &provider_error,
    );
    hooks.on_success(ProviderId::OpenAi, "complete", 2);
    hooks.on_failure(ProviderId::OpenAi, "complete", 2, &provider_error);

    hooks.on_execution_start(&sample_tool_call(), &sample_tool_context());
    hooks.on_execution_success(
        &sample_tool_call(),
        &sample_tool_context(),
        &ToolExecutionResult::new("call-1", "ok"),
        Duration::from_millis(20),
    );
    hooks.on_execution_failure(
        &sample_tool_call(),
        &sample_tool_context(),
        &tool_error,
        Duration::from_millis(20),
    );

    hooks.on_phase_start(
        HarnessPhase::Initializer,
        &SessionId::from("session-1"),
        "run-1",
    );
    hooks.on_phase_success(
        HarnessPhase::TaskIteration,
        &SessionId::from("session-1"),
        "run-2",
        Duration::from_millis(30),
    );
    hooks.on_phase_failure(
        HarnessPhase::TaskIteration,
        &SessionId::from("session-1"),
        "run-3",
        &harness_error,
        Duration::from_millis(30),
    );
}

#[derive(Default, Clone)]
struct RecordingProviderHooks {
    events: Arc<Mutex<Vec<&'static str>>>,
}

impl ProviderOperationHooks for RecordingProviderHooks {
    fn on_attempt_start(&self, _provider: ProviderId, _operation: &str, _attempt: u32) {
        self.events
            .lock()
            .expect("events lock")
            .push("attempt_start");
    }

    fn on_retry_scheduled(
        &self,
        _provider: ProviderId,
        _operation: &str,
        _attempt: u32,
        _delay: Duration,
        _error: &ProviderError,
    ) {
        self.events
            .lock()
            .expect("events lock")
            .push("retry_scheduled");
    }

    fn on_success(&self, _provider: ProviderId, _operation: &str, _attempts: u32) {
        self.events.lock().expect("events lock").push("success");
    }

    fn on_failure(
        &self,
        _provider: ProviderId,
        _operation: &str,
        _attempts: u32,
        _error: &ProviderError,
    ) {
        self.events.lock().expect("events lock").push("failure");
    }
}

#[derive(Default, Clone)]
struct RecordingToolHooks {
    events: Arc<Mutex<Vec<&'static str>>>,
}

impl ToolRuntimeHooks for RecordingToolHooks {
    fn on_execution_start(&self, _tool_call: &ToolCall, _context: &ToolExecutionContext) {
        self.events.lock().expect("events lock").push("start");
    }

    fn on_execution_success(
        &self,
        _tool_call: &ToolCall,
        _context: &ToolExecutionContext,
        _result: &ToolExecutionResult,
        _elapsed: Duration,
    ) {
        self.events.lock().expect("events lock").push("success");
    }

    fn on_execution_failure(
        &self,
        _tool_call: &ToolCall,
        _context: &ToolExecutionContext,
        _error: &ToolError,
        _elapsed: Duration,
    ) {
        self.events.lock().expect("events lock").push("failure");
    }
}

#[derive(Default, Clone)]
struct RecordingHarnessHooks {
    events: Arc<Mutex<Vec<&'static str>>>,
}

impl HarnessRuntimeHooks for RecordingHarnessHooks {
    fn on_phase_start(&self, _phase: HarnessPhase, _session_id: &SessionId, _run_id: &str) {
        self.events.lock().expect("events lock").push("start");
    }

    fn on_phase_success(
        &self,
        _phase: HarnessPhase,
        _session_id: &SessionId,
        _run_id: &str,
        _elapsed: Duration,
    ) {
        self.events.lock().expect("events lock").push("success");
    }

    fn on_phase_failure(
        &self,
        _phase: HarnessPhase,
        _session_id: &SessionId,
        _run_id: &str,
        _error: &HarnessError,
        _elapsed: Duration,
    ) {
        self.events.lock().expect("events lock").push("failure");
    }
}

struct PanicProviderHooks;

impl ProviderOperationHooks for PanicProviderHooks {
    fn on_attempt_start(&self, _provider: ProviderId, _operation: &str, _attempt: u32) {
        panic!("attempt_start panic");
    }

    fn on_retry_scheduled(
        &self,
        _provider: ProviderId,
        _operation: &str,
        _attempt: u32,
        _delay: Duration,
        _error: &ProviderError,
    ) {
        panic!("retry_scheduled panic");
    }

    fn on_success(&self, _provider: ProviderId, _operation: &str, _attempts: u32) {
        panic!("success panic");
    }

    fn on_failure(
        &self,
        _provider: ProviderId,
        _operation: &str,
        _attempts: u32,
        _error: &ProviderError,
    ) {
        panic!("failure panic");
    }
}

struct PanicToolHooks;

impl ToolRuntimeHooks for PanicToolHooks {
    fn on_execution_start(&self, _tool_call: &ToolCall, _context: &ToolExecutionContext) {
        panic!("start panic");
    }

    fn on_execution_success(
        &self,
        _tool_call: &ToolCall,
        _context: &ToolExecutionContext,
        _result: &ToolExecutionResult,
        _elapsed: Duration,
    ) {
        panic!("success panic");
    }

    fn on_execution_failure(
        &self,
        _tool_call: &ToolCall,
        _context: &ToolExecutionContext,
        _error: &ToolError,
        _elapsed: Duration,
    ) {
        panic!("failure panic");
    }
}

struct PanicHarnessHooks;

impl HarnessRuntimeHooks for PanicHarnessHooks {
    fn on_phase_start(&self, _phase: HarnessPhase, _session_id: &SessionId, _run_id: &str) {
        panic!("start panic");
    }

    fn on_phase_success(
        &self,
        _phase: HarnessPhase,
        _session_id: &SessionId,
        _run_id: &str,
        _elapsed: Duration,
    ) {
        panic!("success panic");
    }

    fn on_phase_failure(
        &self,
        _phase: HarnessPhase,
        _session_id: &SessionId,
        _run_id: &str,
        _error: &HarnessError,
        _elapsed: Duration,
    ) {
        panic!("failure panic");
    }
}

#[test]
fn safe_provider_hooks_delegate_when_inner_succeeds() {
    let inner = RecordingProviderHooks::default();
    let events = Arc::clone(&inner.events);
    let hooks = SafeProviderHooks::new(inner);
    let provider_error = ProviderError::timeout("provider timeout");

    hooks.on_attempt_start(ProviderId::OpenAi, "complete", 1);
    hooks.on_retry_scheduled(
        ProviderId::OpenAi,
        "complete",
        1,
        Duration::from_millis(10),
        &provider_error,
    );
    hooks.on_success(ProviderId::OpenAi, "complete", 2);
    hooks.on_failure(ProviderId::OpenAi, "complete", 2, &provider_error);

    assert_eq!(events.lock().expect("events lock").len(), 4);
}

#[test]
fn safe_tool_hooks_delegate_when_inner_succeeds() {
    let inner = RecordingToolHooks::default();
    let events = Arc::clone(&inner.events);
    let hooks = SafeToolHooks::new(inner);
    let tool_error = ToolError::execution("tool failed");

    hooks.on_execution_start(&sample_tool_call(), &sample_tool_context());
    hooks.on_execution_success(
        &sample_tool_call(),
        &sample_tool_context(),
        &ToolExecutionResult::new("call-1", "ok"),
        Duration::from_millis(20),
    );
    hooks.on_execution_failure(
        &sample_tool_call(),
        &sample_tool_context(),
        &tool_error,
        Duration::from_millis(20),
    );

    assert_eq!(events.lock().expect("events lock").len(), 3);
}

#[test]
fn safe_harness_hooks_delegate_when_inner_succeeds() {
    let inner = RecordingHarnessHooks::default();
    let events = Arc::clone(&inner.events);
    let hooks = SafeHarnessHooks::new(inner);
    let harness_error = HarnessError::validation("validation failed");

    hooks.on_phase_start(
        HarnessPhase::Initializer,
        &SessionId::from("session-1"),
        "run-1",
    );
    hooks.on_phase_success(
        HarnessPhase::TaskIteration,
        &SessionId::from("session-1"),
        "run-2",
        Duration::from_millis(30),
    );
    hooks.on_phase_failure(
        HarnessPhase::TaskIteration,
        &SessionId::from("session-1"),
        "run-3",
        &harness_error,
        Duration::from_millis(30),
    );

    assert_eq!(events.lock().expect("events lock").len(), 3);
}

#[test]
fn safe_provider_hooks_swallow_panics() {
    let hooks = SafeProviderHooks::new(PanicProviderHooks);
    let provider_error = ProviderError::timeout("provider timeout");

    hooks.on_attempt_start(ProviderId::OpenAi, "complete", 1);
    hooks.on_retry_scheduled(
        ProviderId::OpenAi,
        "complete",
        1,
        Duration::from_millis(10),
        &provider_error,
    );
    hooks.on_success(ProviderId::OpenAi, "complete", 2);
    hooks.on_failure(ProviderId::OpenAi, "complete", 2, &provider_error);
}

#[test]
fn safe_tool_hooks_swallow_panics() {
    let hooks = SafeToolHooks::new(PanicToolHooks);
    let tool_error = ToolError::execution("tool failed");

    hooks.on_execution_start(&sample_tool_call(), &sample_tool_context());
    hooks.on_execution_success(
        &sample_tool_call(),
        &sample_tool_context(),
        &ToolExecutionResult::new("call-1", "ok"),
        Duration::from_millis(20),
    );
    hooks.on_execution_failure(
        &sample_tool_call(),
        &sample_tool_context(),
        &tool_error,
        Duration::from_millis(20),
    );
}

#[test]
fn safe_harness_hooks_swallow_panics() {
    let hooks = SafeHarnessHooks::new(PanicHarnessHooks);
    let harness_error = HarnessError::validation("validation failed");

    hooks.on_phase_start(
        HarnessPhase::Initializer,
        &SessionId::from("session-1"),
        "run-1",
    );
    hooks.on_phase_success(
        HarnessPhase::TaskIteration,
        &SessionId::from("session-1"),
        "run-2",
        Duration::from_millis(30),
    );
    hooks.on_phase_failure(
        HarnessPhase::TaskIteration,
        &SessionId::from("session-1"),
        "run-3",
        &harness_error,
        Duration::from_millis(30),
    );
}
