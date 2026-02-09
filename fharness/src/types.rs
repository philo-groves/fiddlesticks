//! Harness request/response types and run policy definitions.
//!
//! ```rust
//! use fchat::ChatSession;
//! use fharness::{RunPolicy, RunPolicyMode, RuntimeRunRequest, TaskIterationRequest};
//! use fprovider::ProviderId;
//!
//! let session = ChatSession::new("session-1", ProviderId::OpenAi, "gpt-4o-mini");
//! let task = TaskIterationRequest::new(session.clone(), "run-1").enable_streaming();
//! let runtime = RuntimeRunRequest::new(session, "run-2", "Implement feature")
//!     .with_prompt_override("Use tool-assisted flow");
//! let policy = RunPolicy::bounded_batch(2);
//!
//! assert!(task.stream);
//! assert_eq!(runtime.run_id, "run-2");
//! assert_eq!(policy.mode, RunPolicyMode::BoundedBatch);
//! ```

use fchat::ChatSession;
use fcommon::SessionId;
use fmemory::{FeatureRecord, InitPlan};

use crate::HarnessError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitializerRequest {
    pub session_id: SessionId,
    pub run_id: String,
    pub active_branch: String,
    pub current_objective: String,
    pub init_plan: Option<InitPlan>,
    pub feature_list: Vec<FeatureRecord>,
    pub progress_summary: String,
}

impl InitializerRequest {
    pub fn new(
        session_id: impl Into<SessionId>,
        run_id: impl Into<String>,
        current_objective: impl Into<String>,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            run_id: run_id.into(),
            active_branch: "feature/initializer".to_string(),
            current_objective: current_objective.into(),
            init_plan: None,
            feature_list: Vec::new(),
            progress_summary: "Initializer scaffold created".to_string(),
        }
    }

    pub fn with_active_branch(mut self, active_branch: impl Into<String>) -> Self {
        self.active_branch = active_branch.into();
        self
    }

    pub fn with_init_plan(mut self, init_plan: InitPlan) -> Self {
        self.init_plan = Some(init_plan);
        self
    }

    pub fn with_feature_list(mut self, feature_list: Vec<FeatureRecord>) -> Self {
        self.feature_list = feature_list;
        self
    }

    pub fn with_progress_summary(mut self, progress_summary: impl Into<String>) -> Self {
        self.progress_summary = progress_summary.into();
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitializerResult {
    pub session_id: SessionId,
    pub created: bool,
    pub schema_version: u32,
    pub harness_version: String,
    pub feature_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskIterationRequest {
    pub session: ChatSession,
    pub run_id: String,
    pub stream: bool,
    pub prompt_override: Option<String>,
}

impl TaskIterationRequest {
    pub fn new(session: ChatSession, run_id: impl Into<String>) -> Self {
        Self {
            session,
            run_id: run_id.into(),
            stream: false,
            prompt_override: None,
        }
    }

    pub fn enable_streaming(mut self) -> Self {
        self.stream = true;
        self
    }

    pub fn with_prompt_override(mut self, prompt_override: impl Into<String>) -> Self {
        self.prompt_override = Some(prompt_override.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskIterationResult {
    pub session_id: SessionId,
    pub selected_feature_id: Option<String>,
    pub processed_feature_ids: Vec<String>,
    pub validated_feature_ids: Vec<String>,
    pub processed_feature_count: usize,
    pub validated: bool,
    pub no_pending_features: bool,
    pub used_stream: bool,
    pub assistant_message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarnessPhase {
    Initializer,
    TaskIteration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeRunOutcome {
    Initializer(InitializerResult),
    TaskIteration(TaskIterationResult),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeRunRequest {
    pub session: ChatSession,
    pub run_id: String,
    pub current_objective: String,
    pub stream: bool,
    pub prompt_override: Option<String>,
    pub init_plan: Option<InitPlan>,
    pub feature_list: Vec<FeatureRecord>,
    pub active_branch: String,
    pub progress_summary: Option<String>,
}

impl RuntimeRunRequest {
    pub fn new(
        session: ChatSession,
        run_id: impl Into<String>,
        current_objective: impl Into<String>,
    ) -> Self {
        Self {
            session,
            run_id: run_id.into(),
            current_objective: current_objective.into(),
            stream: false,
            prompt_override: None,
            init_plan: None,
            feature_list: Vec::new(),
            active_branch: "feature/initializer".to_string(),
            progress_summary: None,
        }
    }

    pub fn enable_streaming(mut self) -> Self {
        self.stream = true;
        self
    }

    pub fn with_prompt_override(mut self, prompt_override: impl Into<String>) -> Self {
        self.prompt_override = Some(prompt_override.into());
        self
    }

    pub fn with_init_plan(mut self, init_plan: InitPlan) -> Self {
        self.init_plan = Some(init_plan);
        self
    }

    pub fn with_feature_list(mut self, feature_list: Vec<FeatureRecord>) -> Self {
        self.feature_list = feature_list;
        self
    }

    pub fn with_active_branch(mut self, active_branch: impl Into<String>) -> Self {
        self.active_branch = active_branch.into();
        self
    }

    pub fn with_progress_summary(mut self, progress_summary: impl Into<String>) -> Self {
        self.progress_summary = Some(progress_summary.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailFastPolicy {
    pub on_health_check_error: bool,
    pub on_chat_error: bool,
    pub on_validation_failure: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunPolicyMode {
    StrictIncremental,
    BoundedBatch,
    UnlimitedBatch,
}

impl Default for FailFastPolicy {
    fn default() -> Self {
        Self {
            on_health_check_error: true,
            on_chat_error: false,
            on_validation_failure: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunPolicy {
    pub mode: RunPolicyMode,
    pub max_turns_per_run: usize,
    pub max_features_per_run: usize,
    pub retry_budget: usize,
    pub fail_fast: FailFastPolicy,
}

impl Default for RunPolicy {
    fn default() -> Self {
        Self {
            mode: RunPolicyMode::StrictIncremental,
            max_turns_per_run: 1,
            max_features_per_run: 1,
            retry_budget: 0,
            fail_fast: FailFastPolicy::default(),
        }
    }
}

impl RunPolicy {
    pub fn strict() -> Self {
        Self::default()
    }

    pub fn bounded_batch(max_features_per_run: usize) -> Self {
        Self {
            mode: RunPolicyMode::BoundedBatch,
            max_features_per_run,
            ..Self::default()
        }
    }

    pub fn unlimited_batch() -> Self {
        Self {
            mode: RunPolicyMode::UnlimitedBatch,
            ..Self::default()
        }
    }

    pub fn validate(&self) -> Result<(), HarnessError> {
        if self.max_turns_per_run == 0 {
            return Err(HarnessError::invalid_request(
                "run policy requires max_turns_per_run >= 1",
            ));
        }

        match self.mode {
            RunPolicyMode::StrictIncremental => {
                if self.max_features_per_run != 1 {
                    return Err(HarnessError::invalid_request(
                        "run policy strict mode requires max_features_per_run = 1",
                    ));
                }
            }
            RunPolicyMode::BoundedBatch => {
                if self.max_features_per_run == 0 {
                    return Err(HarnessError::invalid_request(
                        "run policy bounded-batch mode requires max_features_per_run >= 1",
                    ));
                }
            }
            RunPolicyMode::UnlimitedBatch => {}
        }

        Ok(())
    }

    pub fn max_features_per_run_limit(&self) -> Option<usize> {
        match self.mode {
            RunPolicyMode::StrictIncremental => Some(1),
            RunPolicyMode::BoundedBatch => Some(self.max_features_per_run),
            RunPolicyMode::UnlimitedBatch => None,
        }
    }
}
