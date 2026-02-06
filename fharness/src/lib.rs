//! Agent harness orchestration APIs.

use std::collections::HashSet;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::sync::Arc;
use std::time::SystemTime;

use fchat::{ChatError, ChatEvent, ChatService, ChatSession, ChatTurnRequest, ChatTurnResult};
use fcommon::{BoxFuture, SessionId};
use fmemory::{
    FeatureRecord, MemoryBackend, MemoryError, ProgressEntry, RunCheckpoint, RunStatus,
    SessionManifest,
};
use futures_util::StreamExt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarnessErrorKind {
    InvalidRequest,
    Memory,
    Chat,
    Validation,
    HealthCheck,
    NotReady,
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

    pub fn chat(message: impl Into<String>) -> Self {
        Self::new(HarnessErrorKind::Chat, message)
    }

    pub fn validation(message: impl Into<String>) -> Self {
        Self::new(HarnessErrorKind::Validation, message)
    }

    pub fn health_check(message: impl Into<String>) -> Self {
        Self::new(HarnessErrorKind::HealthCheck, message)
    }

    pub fn not_ready(message: impl Into<String>) -> Self {
        Self::new(HarnessErrorKind::NotReady, message)
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

impl From<ChatError> for HarnessError {
    fn from(value: ChatError) -> Self {
        HarnessError::chat(value.to_string())
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingRunRequest {
    pub session: ChatSession,
    pub run_id: String,
    pub stream: bool,
    pub prompt_override: Option<String>,
}

impl CodingRunRequest {
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
pub struct CodingRunResult {
    pub session_id: SessionId,
    pub selected_feature_id: Option<String>,
    pub validated: bool,
    pub no_pending_features: bool,
    pub used_stream: bool,
    pub assistant_message: Option<String>,
}

pub trait HealthChecker: Send + Sync {
    fn run<'a>(
        &'a self,
        session_id: &'a SessionId,
        init_script: &'a str,
    ) -> BoxFuture<'a, Result<(), HarnessError>>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NoopHealthChecker;

impl HealthChecker for NoopHealthChecker {
    fn run<'a>(
        &'a self,
        _session_id: &'a SessionId,
        _init_script: &'a str,
    ) -> BoxFuture<'a, Result<(), HarnessError>> {
        Box::pin(async { Ok(()) })
    }
}

pub trait OutcomeValidator: Send + Sync {
    fn validate<'a>(
        &'a self,
        feature: &'a FeatureRecord,
        result: &'a ChatTurnResult,
    ) -> BoxFuture<'a, Result<bool, HarnessError>>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct AcceptAllValidator;

impl OutcomeValidator for AcceptAllValidator {
    fn validate<'a>(
        &'a self,
        _feature: &'a FeatureRecord,
        _result: &'a ChatTurnResult,
    ) -> BoxFuture<'a, Result<bool, HarnessError>> {
        Box::pin(async { Ok(true) })
    }
}

#[derive(Clone)]
pub struct Harness {
    memory: Arc<dyn MemoryBackend>,
    chat: Option<Arc<ChatService>>,
    health_checker: Arc<dyn HealthChecker>,
    validator: Arc<dyn OutcomeValidator>,
    schema_version: u32,
    harness_version: String,
}

impl Harness {
    pub const DEFAULT_INIT_SCRIPT: &'static str =
        "#!/usr/bin/env bash\nset -e\npwd\ngit log --oneline -20\n";

    pub fn new(memory: Arc<dyn MemoryBackend>) -> Self {
        Self {
            memory,
            chat: None,
            health_checker: Arc::new(NoopHealthChecker),
            validator: Arc::new(AcceptAllValidator),
            schema_version: SessionManifest::DEFAULT_SCHEMA_VERSION,
            harness_version: SessionManifest::DEFAULT_HARNESS_VERSION.to_string(),
        }
    }

    pub fn with_chat(mut self, chat: Arc<ChatService>) -> Self {
        self.chat = Some(chat);
        self
    }

    pub fn with_health_checker(mut self, health_checker: Arc<dyn HealthChecker>) -> Self {
        self.health_checker = health_checker;
        self
    }

    pub fn with_validator(mut self, validator: Arc<dyn OutcomeValidator>) -> Self {
        self.validator = validator;
        self
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
        let InitializerRequest {
            session_id,
            run_id,
            active_branch,
            current_objective,
            init_script,
            feature_list,
            progress_summary,
        } = request;

        if current_objective.trim().is_empty() {
            return Err(HarnessError::invalid_request(
                "current_objective must not be empty",
            ));
        }

        let feature_list = if feature_list.is_empty() {
            self.starter_feature_list(&current_objective)
        } else {
            feature_list
        };
        validate_feature_list(&feature_list)?;

        let progress_summary = if progress_summary.trim().is_empty() {
            format!("Initializer scaffold created for objective: {current_objective}")
        } else {
            progress_summary
        };

        let init_script = init_script.unwrap_or_else(|| Self::DEFAULT_INIT_SCRIPT.to_string());

        let mut manifest = SessionManifest::new(session_id.clone(), active_branch, current_objective)
            .with_schema_version(self.schema_version)
            .with_harness_version(self.harness_version.clone());
        manifest.init_script = Some(init_script);

        let created = self
            .memory
            .initialize_session_if_missing(
                &session_id,
                manifest,
                feature_list,
                Some(ProgressEntry::new(run_id.clone(), progress_summary)),
                Some(RunCheckpoint::started(run_id)),
            )
            .await?;

        let bootstrap = self.memory.load_bootstrap_state(&session_id).await?;
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

    pub async fn run_coding_iteration(
        &self,
        request: CodingRunRequest,
    ) -> Result<CodingRunResult, HarnessError> {
        let chat = self
            .chat
            .as_ref()
            .ok_or_else(|| HarnessError::not_ready("chat service is not configured in harness"))?;

        let started_at = SystemTime::now();
        self.memory
            .record_run_checkpoint(&request.session.id, RunCheckpoint::started(request.run_id.clone()))
            .await?;

        let result = self.run_coding_iteration_inner(chat, &request).await;

        match &result {
            Ok(value) => {
                let (status, note) = if value.no_pending_features {
                    (
                        RunStatus::Succeeded,
                        "No pending features remain; clean handoff ready".to_string(),
                    )
                } else if value.validated {
                    (
                        RunStatus::Succeeded,
                        format!(
                            "Feature '{}' validated and marked passing; clean handoff ready",
                            value
                                .selected_feature_id
                                .clone()
                                .unwrap_or_else(|| "unknown".to_string())
                        ),
                    )
                } else {
                    (
                        RunStatus::Failed,
                        format!(
                            "Feature '{}' was not validated; left failing for next run",
                            value
                                .selected_feature_id
                                .clone()
                                .unwrap_or_else(|| "unknown".to_string())
                        ),
                    )
                };

                self.record_final_handoff(&request, started_at, status, note)
                    .await?;
            }
            Err(error) => {
                self.record_final_handoff(
                    &request,
                    started_at,
                    RunStatus::Failed,
                    format!("Run failed: {}", error),
                )
                .await?;
            }
        }

        result
    }

    pub fn starter_feature_list(&self, objective: &str) -> Vec<FeatureRecord> {
        vec![
            feature(
                "initializer.artifacts",
                "functional",
                format!("Initializer artifacts exist for objective: {objective}"),
                [
                    "Create init script metadata",
                    "Create session manifest",
                    "Create starter feature list",
                ],
            ),
            feature(
                "harness.baseline",
                "functional",
                "Baseline harness checks can run before coding iterations",
                [
                    "Run startup script",
                    "Verify workspace status is readable",
                    "Record baseline in progress log",
                ],
            ),
            feature(
                "chat.turn",
                "functional",
                "Chat turn execution path is available",
                [
                    "Create a chat session",
                    "Run one non-streaming turn",
                    "Persist transcript messages",
                ],
            ),
            feature(
                "chat.streaming",
                "functional",
                "Streaming turn execution emits expected events",
                [
                    "Run one streaming turn",
                    "Observe text/tool events",
                    "Observe terminal turn completion",
                ],
            ),
            feature(
                "tool.loop",
                "functional",
                "Tool loop executes and feeds results back into model",
                [
                    "Register at least one tool",
                    "Execute tool call during turn",
                    "Confirm follow-up completion",
                ],
            ),
            feature(
                "quality.regression",
                "quality",
                "Regression test pass status is tracked",
                [
                    "Run crate-level tests",
                    "Capture failures in progress log",
                    "Only mark feature pass after verification",
                ],
            ),
        ]
    }

    async fn run_coding_iteration_inner(
        &self,
        chat: &ChatService,
        request: &CodingRunRequest,
    ) -> Result<CodingRunResult, HarnessError> {
        let bootstrap = self.memory.load_bootstrap_state(&request.session.id).await?;
        let manifest = bootstrap.manifest.ok_or_else(|| {
            HarnessError::not_ready("session is not initialized; run initializer first")
        })?;

        let init_script = manifest
            .init_script
            .as_deref()
            .unwrap_or(Self::DEFAULT_INIT_SCRIPT);
        self.health_checker.run(&request.session.id, init_script).await?;

        let feature = bootstrap.feature_list.iter().find(|feature| !feature.passes).cloned();

        let Some(feature) = feature else {
            return Ok(CodingRunResult {
                session_id: request.session.id.clone(),
                selected_feature_id: None,
                validated: true,
                no_pending_features: true,
                used_stream: request.stream,
                assistant_message: None,
            });
        };

        let prompt = request.prompt_override.clone().unwrap_or_else(|| {
            build_feature_prompt(&feature, &manifest.current_objective)
        });

        let turn_request = if request.stream {
            ChatTurnRequest::builder(request.session.clone(), prompt)
                .enable_streaming()
                .build()
        } else {
            ChatTurnRequest::builder(request.session.clone(), prompt).build()
        };

        let turn_result = if request.stream {
            let mut stream = chat.stream_turn(turn_request).await?;
            let mut final_result = None;
            while let Some(item) = stream.next().await {
                match item {
                    Ok(ChatEvent::TurnComplete(turn_result)) => final_result = Some(turn_result),
                    Ok(_) => {}
                    Err(err) => return Err(HarnessError::from(err)),
                }
            }

            final_result
                .ok_or_else(|| HarnessError::chat("stream ended without TurnComplete event"))?
        } else {
            chat.run_turn(turn_request).await?
        };

        let validated = self.validator.validate(&feature, &turn_result).await?;
        if validated {
            self.memory
                .update_feature_pass(&request.session.id, &feature.id, true)
                .await?;
        }

        Ok(CodingRunResult {
            session_id: request.session.id.clone(),
            selected_feature_id: Some(feature.id),
            validated,
            no_pending_features: false,
            used_stream: request.stream,
            assistant_message: Some(turn_result.assistant_message),
        })
    }

    async fn record_final_handoff(
        &self,
        request: &CodingRunRequest,
        started_at: SystemTime,
        status: RunStatus,
        note: String,
    ) -> Result<(), HarnessError> {
        self.memory
            .record_run_checkpoint(
                &request.session.id,
                RunCheckpoint {
                    run_id: request.run_id.clone(),
                    started_at,
                    completed_at: Some(SystemTime::now()),
                    status,
                    note: Some(note.clone()),
                },
            )
            .await?;

        self.memory
            .append_progress_entry(&request.session.id, ProgressEntry::new(request.run_id.clone(), note))
            .await?;

        Ok(())
    }
}

fn feature(
    id: impl Into<String>,
    category: impl Into<String>,
    description: impl Into<String>,
    steps: impl IntoIterator<Item = impl Into<String>>,
) -> FeatureRecord {
    FeatureRecord {
        id: id.into(),
        category: category.into(),
        description: description.into(),
        steps: steps.into_iter().map(Into::into).collect(),
        passes: false,
    }
}

fn validate_feature_list(feature_list: &[FeatureRecord]) -> Result<(), HarnessError> {
    if feature_list.is_empty() {
        return Err(HarnessError::invalid_request(
            "feature_list must contain at least one feature",
        ));
    }

    let mut ids = HashSet::new();
    for feature in feature_list {
        if feature.id.trim().is_empty() {
            return Err(HarnessError::invalid_request(
                "feature_list entries require non-empty id",
            ));
        }

        if !ids.insert(feature.id.clone()) {
            return Err(HarnessError::invalid_request(format!(
                "feature_list contains duplicate id '{}': ids must be unique",
                feature.id
            )));
        }

        if feature.description.trim().is_empty() {
            return Err(HarnessError::invalid_request(format!(
                "feature '{}' must include a non-empty description",
                feature.id
            )));
        }

        if feature.steps.is_empty() {
            return Err(HarnessError::invalid_request(format!(
                "feature '{}' must include at least one validation step",
                feature.id
            )));
        }

        if feature.passes {
            return Err(HarnessError::invalid_request(format!(
                "feature '{}' cannot start with passes=true during initializer phase",
                feature.id
            )));
        }
    }

    Ok(())
}

fn build_feature_prompt(feature: &FeatureRecord, objective: &str) -> String {
    let steps = feature
        .steps
        .iter()
        .map(|step| format!("- {step}"))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "Objective: {objective}\n\nWork on one feature incrementally and leave a clean handoff.\n\nFeature: {}\nCategory: {}\nDescription: {}\nValidation steps:\n{}",
        feature.id, feature.category, feature.description, steps
    )
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use fchat::{ChatPolicy, InMemoryConversationStore};
    use fmemory::InMemoryMemoryBackend;
    use fprovider::{
        Message, ModelProvider, ModelRequest, ModelResponse, OutputItem, ProviderFuture,
        ProviderId, StopReason, StreamEvent, TokenUsage, VecEventStream,
    };

    use super::*;

    #[derive(Debug)]
    struct FakeProvider;

    impl ModelProvider for FakeProvider {
        fn id(&self) -> ProviderId {
            ProviderId::OpenAi
        }

        fn complete<'a>(
            &'a self,
            request: ModelRequest,
        ) -> ProviderFuture<'a, Result<ModelResponse, fprovider::ProviderError>> {
            Box::pin(async move {
                Ok(ModelResponse {
                    provider: ProviderId::OpenAi,
                    model: request.model,
                    output: vec![OutputItem::Message(Message::new(
                        fprovider::Role::Assistant,
                        "implemented",
                    ))],
                    stop_reason: StopReason::EndTurn,
                    usage: TokenUsage::default(),
                })
            })
        }

        fn stream<'a>(
            &'a self,
            request: ModelRequest,
        ) -> ProviderFuture<'a, Result<fprovider::BoxedEventStream<'a>, fprovider::ProviderError>> {
            Box::pin(async move {
                let response = ModelResponse {
                    provider: ProviderId::OpenAi,
                    model: request.model,
                    output: vec![OutputItem::Message(Message::new(
                        fprovider::Role::Assistant,
                        "implemented-stream",
                    ))],
                    stop_reason: StopReason::EndTurn,
                    usage: TokenUsage::default(),
                };
                let stream = VecEventStream::new(vec![
                    Ok(StreamEvent::TextDelta("implemented-stream".to_string())),
                    Ok(StreamEvent::ResponseComplete(response)),
                ]);
                Ok(Box::pin(stream) as fprovider::BoxedEventStream<'a>)
            })
        }
    }

    #[derive(Debug, Default)]
    struct RecordingHealthChecker {
        calls: Mutex<u32>,
    }

    impl HealthChecker for RecordingHealthChecker {
        fn run<'a>(
            &'a self,
            _session_id: &'a SessionId,
            _init_script: &'a str,
        ) -> BoxFuture<'a, Result<(), HarnessError>> {
            Box::pin(async move {
                *self.calls.lock().expect("calls lock") += 1;
                Ok(())
            })
        }
    }

    struct AlwaysFailValidator;

    impl OutcomeValidator for AlwaysFailValidator {
        fn validate<'a>(
            &'a self,
            _feature: &'a FeatureRecord,
            _result: &'a ChatTurnResult,
        ) -> BoxFuture<'a, Result<bool, HarnessError>> {
            Box::pin(async { Ok(false) })
        }
    }

    fn build_harness(
        memory: Arc<dyn MemoryBackend>,
        health_checker: Option<Arc<dyn HealthChecker>>,
        validator: Option<Arc<dyn OutcomeValidator>>,
    ) -> Harness {
        let provider = Arc::new(FakeProvider);
        let store = Arc::new(InMemoryConversationStore::new());
        let chat = Arc::new(
            ChatService::builder(provider)
                .store(store)
                .policy(ChatPolicy::default())
                .build(),
        );

        let harness = Harness::new(memory).with_chat(chat);
        let harness = if let Some(health_checker) = health_checker {
            harness.with_health_checker(health_checker)
        } else {
            harness
        };

        if let Some(validator) = validator {
            harness.with_validator(validator)
        } else {
            harness
        }
    }

    async fn initialize_for_tests(harness: &Harness, session_id: &str) {
        harness
            .run_initializer(
                InitializerRequest::new(session_id, "run-init", "prepare coding run")
                    .with_feature_list(vec![FeatureRecord {
                        id: "feature-1".to_string(),
                        category: "functional".to_string(),
                        description: "build one feature".to_string(),
                        steps: vec!["make it work".to_string()],
                        passes: false,
                    }]),
            )
            .await
            .expect("initializer should succeed");
    }

    #[tokio::test]
    async fn initializer_creates_bootstrap_state_on_first_run() {
        let memory: Arc<dyn MemoryBackend> = Arc::new(InMemoryMemoryBackend::new());
        let harness = Harness::new(memory.clone());

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

        let state = memory
            .load_bootstrap_state(&SessionId::from("session-1"))
            .await
            .expect("bootstrap should load");
        let manifest = state.manifest.expect("manifest should exist");
        assert!(manifest.init_script.is_some());
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

    #[tokio::test]
    async fn initializer_generates_starter_feature_list_when_missing() {
        let memory: Arc<dyn MemoryBackend> = Arc::new(InMemoryMemoryBackend::new());
        let harness = Harness::new(memory);

        let request = InitializerRequest::new("session-4", "run-1", "Build coding harness");
        let result = harness
            .run_initializer(request)
            .await
            .expect("initializer should succeed");

        assert!(result.created);
        assert!(result.feature_count >= 4);
    }

    #[tokio::test]
    async fn initializer_rejects_duplicate_or_passing_features() {
        let memory: Arc<dyn MemoryBackend> = Arc::new(InMemoryMemoryBackend::new());
        let harness = Harness::new(memory);

        let duplicate_features = vec![
            FeatureRecord {
                id: "dup".to_string(),
                category: "functional".to_string(),
                description: "first".to_string(),
                steps: vec!["step".to_string()],
                passes: false,
            },
            FeatureRecord {
                id: "dup".to_string(),
                category: "functional".to_string(),
                description: "second".to_string(),
                steps: vec!["step".to_string()],
                passes: false,
            },
        ];

        let duplicate_error = Harness::new(Arc::new(InMemoryMemoryBackend::new()))
            .run_initializer(
                InitializerRequest::new("session-5", "run-1", "Init")
                    .with_feature_list(duplicate_features),
            )
            .await
            .expect_err("duplicate ids should fail");
        assert_eq!(duplicate_error.kind, HarnessErrorKind::InvalidRequest);

        let passing_error = harness
            .run_initializer(
                InitializerRequest::new("session-6", "run-1", "Init")
                    .with_feature_list(vec![FeatureRecord {
                        id: "done".to_string(),
                        category: "functional".to_string(),
                        description: "already done".to_string(),
                        steps: vec!["step".to_string()],
                        passes: true,
                    }]),
            )
            .await
            .expect_err("pre-passing feature should fail");
        assert_eq!(passing_error.kind, HarnessErrorKind::InvalidRequest);
    }

    #[tokio::test]
    async fn coding_iteration_gets_bearings_executes_and_marks_feature_passed() {
        let memory: Arc<dyn MemoryBackend> = Arc::new(InMemoryMemoryBackend::new());
        let health = Arc::new(RecordingHealthChecker::default());
        let harness = build_harness(memory.clone(), Some(health.clone()), None);
        initialize_for_tests(&harness, "session-coding").await;

        let session = ChatSession::new("session-coding", ProviderId::OpenAi, "gpt-4o-mini");
        let result = harness
            .run_coding_iteration(CodingRunRequest::new(session, "run-code-1"))
            .await
            .expect("coding run should succeed");

        assert!(!result.no_pending_features);
        assert!(result.validated);
        assert_eq!(result.selected_feature_id.as_deref(), Some("feature-1"));

        let calls = health.calls.lock().expect("calls lock");
        assert_eq!(*calls, 1);

        let state = memory
            .load_bootstrap_state(&SessionId::from("session-coding"))
            .await
            .expect("state should load");
        assert!(state.feature_list[0].passes);
        assert!(state.recent_progress.iter().any(|entry| entry.run_id == "run-code-1"));
        assert!(state
            .checkpoints
            .iter()
            .any(|checkpoint| checkpoint.run_id == "run-code-1" && checkpoint.completed_at.is_some()));
    }

    #[tokio::test]
    async fn coding_iteration_stream_path_works_and_records_handoff() {
        let memory: Arc<dyn MemoryBackend> = Arc::new(InMemoryMemoryBackend::new());
        let harness = build_harness(memory.clone(), None, None);
        initialize_for_tests(&harness, "session-stream").await;

        let session = ChatSession::new("session-stream", ProviderId::OpenAi, "gpt-4o-mini");
        let result = harness
            .run_coding_iteration(CodingRunRequest::new(session, "run-stream-1").enable_streaming())
            .await
            .expect("streaming coding run should succeed");

        assert!(result.used_stream);
        assert!(result.validated);

        let state = memory
            .load_bootstrap_state(&SessionId::from("session-stream"))
            .await
            .expect("state should load");
        assert!(state.recent_progress.iter().any(|entry| entry.run_id == "run-stream-1"));
    }

    #[tokio::test]
    async fn coding_iteration_does_not_mark_feature_when_not_validated() {
        let memory: Arc<dyn MemoryBackend> = Arc::new(InMemoryMemoryBackend::new());
        let harness = build_harness(
            memory.clone(),
            None,
            Some(Arc::new(AlwaysFailValidator)),
        );
        initialize_for_tests(&harness, "session-unvalidated").await;

        let session = ChatSession::new("session-unvalidated", ProviderId::OpenAi, "gpt-4o-mini");
        let result = harness
            .run_coding_iteration(CodingRunRequest::new(session, "run-code-2"))
            .await
            .expect("coding run should complete");

        assert!(!result.validated);

        let state = memory
            .load_bootstrap_state(&SessionId::from("session-unvalidated"))
            .await
            .expect("state should load");
        assert!(!state.feature_list[0].passes);
    }
}
