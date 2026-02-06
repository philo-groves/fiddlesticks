//! Agent harness orchestration APIs.

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::sync::Arc;

use fcommon::SessionId;
use fmemory::{FeatureRecord, MemoryBackend, MemoryError, ProgressEntry, RunCheckpoint, SessionManifest};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarnessErrorKind {
    InvalidRequest,
    Memory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessError {
    pub kind: HarnessErrorKind,
    pub message: String,
}

impl HarnessError {
    pub fn new(kind: HarnessErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(HarnessErrorKind::InvalidRequest, message)
    }

    pub fn memory(message: impl Into<String>) -> Self {
        Self::new(HarnessErrorKind::Memory, message)
    }
}

impl Display for HarnessError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)
    }
}

impl Error for HarnessError {}

impl From<MemoryError> for HarnessError {
    fn from(value: MemoryError) -> Self {
        HarnessError::memory(value.message)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitializerRequest {
    pub session_id: SessionId,
    pub run_id: String,
    pub active_branch: String,
    pub current_objective: String,
    pub init_script: Option<String>,
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
            init_script: None,
            feature_list: Vec::new(),
            progress_summary: "Initializer scaffold created".to_string(),
        }
    }

    pub fn with_active_branch(mut self, active_branch: impl Into<String>) -> Self {
        self.active_branch = active_branch.into();
        self
    }

    pub fn with_init_script(mut self, init_script: impl Into<String>) -> Self {
        self.init_script = Some(init_script.into());
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

#[derive(Clone)]
pub struct Harness {
    memory: Arc<dyn MemoryBackend>,
    schema_version: u32,
    harness_version: String,
}

impl Harness {
    pub fn new(memory: Arc<dyn MemoryBackend>) -> Self {
        Self {
            memory,
            schema_version: SessionManifest::DEFAULT_SCHEMA_VERSION,
            harness_version: SessionManifest::DEFAULT_HARNESS_VERSION.to_string(),
        }
    }

    pub fn with_schema_version(mut self, schema_version: u32) -> Self {
        self.schema_version = schema_version;
        self
    }

    pub fn with_harness_version(mut self, harness_version: impl Into<String>) -> Self {
        self.harness_version = harness_version.into();
        self
    }

    pub async fn run_initializer(
        &self,
        request: InitializerRequest,
    ) -> Result<InitializerResult, HarnessError> {
        if request.current_objective.trim().is_empty() {
            return Err(HarnessError::invalid_request(
                "current_objective must not be empty",
            ));
        }

        let mut manifest = SessionManifest::new(
            request.session_id.clone(),
            request.active_branch,
            request.current_objective,
        )
        .with_schema_version(self.schema_version)
        .with_harness_version(self.harness_version.clone());
        manifest.init_script = request.init_script;

        let created = self
            .memory
            .initialize_session_if_missing(
                &request.session_id,
                manifest,
                request.feature_list,
                Some(ProgressEntry::new(request.run_id.clone(), request.progress_summary)),
                Some(RunCheckpoint::started(request.run_id)),
            )
            .await?;

        let bootstrap = self.memory.load_bootstrap_state(&request.session_id).await?;
        let manifest = bootstrap
            .manifest
            .ok_or_else(|| HarnessError::memory("manifest missing after initializer run"))?;

        Ok(InitializerResult {
            session_id: manifest.session_id,
            created,
            schema_version: manifest.schema_version,
            harness_version: manifest.harness_version,
            feature_count: bootstrap.feature_list.len(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use fmemory::{InMemoryMemoryBackend, MemoryBackend};

    use super::*;

    #[tokio::test]
    async fn initializer_creates_bootstrap_state_on_first_run() {
        let memory: Arc<dyn MemoryBackend> = Arc::new(InMemoryMemoryBackend::new());
        let harness = Harness::new(memory);

        let request = InitializerRequest::new("session-1", "run-1", "Build phase 2")
            .with_init_script("#!/usr/bin/env bash\necho start")
            .with_feature_list(vec![FeatureRecord {
                id: "feature-1".to_string(),
                category: "functional".to_string(),
                description: "initializer creates artifacts".to_string(),
                steps: vec!["write feature list".to_string()],
                passes: false,
            }]);

        let result = harness
            .run_initializer(request)
            .await
            .expect("initializer should succeed");
        assert!(result.created);
        assert_eq!(result.feature_count, 1);
        assert_eq!(result.schema_version, SessionManifest::DEFAULT_SCHEMA_VERSION);
    }

    #[tokio::test]
    async fn initializer_is_idempotent_when_session_already_initialized() {
        let memory: Arc<dyn MemoryBackend> = Arc::new(InMemoryMemoryBackend::new());
        let harness = Harness::new(memory);

        let first = InitializerRequest::new("session-2", "run-1", "Initialize")
            .with_feature_list(vec![FeatureRecord {
                id: "feature-a".to_string(),
                category: "functional".to_string(),
                description: "first".to_string(),
                steps: vec!["step".to_string()],
                passes: false,
            }]);

        let second = InitializerRequest::new("session-2", "run-2", "Should not overwrite")
            .with_feature_list(vec![FeatureRecord {
                id: "feature-b".to_string(),
                category: "functional".to_string(),
                description: "second".to_string(),
                steps: vec!["step".to_string()],
                passes: false,
            }]);

        let first_result = harness
            .run_initializer(first)
            .await
            .expect("first init should succeed");
        assert!(first_result.created);
        assert_eq!(first_result.feature_count, 1);

        let second_result = harness
            .run_initializer(second)
            .await
            .expect("second init should succeed");
        assert!(!second_result.created);
        assert_eq!(second_result.feature_count, 1);
    }

    #[tokio::test]
    async fn initializer_rejects_empty_objective() {
        let memory: Arc<dyn MemoryBackend> = Arc::new(InMemoryMemoryBackend::new());
        let harness = Harness::new(memory);

        let request = InitializerRequest::new("session-3", "run-1", "   ");
        let error = harness
            .run_initializer(request)
            .await
            .expect_err("initializer should fail");

        assert_eq!(error.kind, HarnessErrorKind::InvalidRequest);
    }
}
