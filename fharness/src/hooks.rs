//! Runtime hook contracts for observing harness phase execution.
//!
//! ```rust
//! use fharness::{HarnessRuntimeHooks, NoopHarnessRuntimeHooks};
//!
//! fn accepts_hooks(_hooks: &dyn HarnessRuntimeHooks) {}
//!
//! let hooks = NoopHarnessRuntimeHooks;
//! accepts_hooks(&hooks);
//! ```

use std::time::Duration;

use fcommon::SessionId;

use crate::{HarnessError, HarnessPhase};

pub trait HarnessRuntimeHooks: Send + Sync {
    fn on_phase_start(&self, _phase: HarnessPhase, _session_id: &SessionId, _run_id: &str) {}

    fn on_phase_success(
        &self,
        _phase: HarnessPhase,
        _session_id: &SessionId,
        _run_id: &str,
        _elapsed: Duration,
    ) {
    }

    fn on_phase_failure(
        &self,
        _phase: HarnessPhase,
        _session_id: &SessionId,
        _run_id: &str,
        _error: &HarnessError,
        _elapsed: Duration,
    ) {
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NoopHarnessRuntimeHooks;

impl HarnessRuntimeHooks for NoopHarnessRuntimeHooks {}
