use std::collections::HashSet;
use std::sync::Arc;
use std::time::SystemTime;

use fchat::{ChatEvent, ChatPolicy, ChatService, ChatTurnRequest, ChatTurnResult};
use fcommon::SessionId;
use fmemory::{
    FeatureRecord, InitPlan, InitStep, MemoryBackend, MemoryConversationStore, ProgressEntry,
    RunCheckpoint, RunStatus, SessionManifest,
};
use fprovider::ModelProvider;
use ftooling::ToolRuntime;
use futures_util::StreamExt;

use crate::{
    AcceptAllValidator, ChatEventObserver, FeatureSelector, FirstPendingFeatureSelector,
    HarnessError, HarnessPhase, HealthChecker, InitializerRequest, InitializerResult,
    NoopHealthChecker, OutcomeValidator, RunPolicy, RuntimeRunOutcome, RuntimeRunRequest,
    TaskIterationRequest, TaskIterationResult,
};

pub struct HarnessBuilder {
    memory: Arc<dyn MemoryBackend>,
    provider: Option<Arc<dyn ModelProvider>>,
    tool_runtime: Option<Arc<dyn ToolRuntime>>,
    chat_policy: ChatPolicy,
    health_checker: Arc<dyn HealthChecker>,
    validator: Arc<dyn OutcomeValidator>,
    feature_selector: Arc<dyn FeatureSelector>,
    run_policy: RunPolicy,
    schema_version: u32,
    harness_version: String,
}

impl HarnessBuilder {
    pub fn new(memory: Arc<dyn MemoryBackend>) -> Self {
        Self {
            memory,
            provider: None,
            tool_runtime: None,
            chat_policy: ChatPolicy::default(),
            health_checker: Arc::new(NoopHealthChecker),
            validator: Arc::new(AcceptAllValidator),
            feature_selector: Arc::new(FirstPendingFeatureSelector),
            run_policy: RunPolicy::default(),
            schema_version: SessionManifest::DEFAULT_SCHEMA_VERSION,
            harness_version: SessionManifest::DEFAULT_HARNESS_VERSION.to_string(),
        }
    }

    pub fn provider(mut self, provider: Arc<dyn ModelProvider>) -> Self {
        self.provider = Some(provider);
        self
    }

    pub fn tool_runtime(mut self, tool_runtime: Arc<dyn ToolRuntime>) -> Self {
        self.tool_runtime = Some(tool_runtime);
        self
    }

    pub fn chat_policy(mut self, chat_policy: ChatPolicy) -> Self {
        self.chat_policy = chat_policy;
        self
    }

    pub fn health_checker(mut self, health_checker: Arc<dyn HealthChecker>) -> Self {
        self.health_checker = health_checker;
        self
    }

    pub fn validator(mut self, validator: Arc<dyn OutcomeValidator>) -> Self {
        self.validator = validator;
        self
    }

    pub fn feature_selector(mut self, feature_selector: Arc<dyn FeatureSelector>) -> Self {
        self.feature_selector = feature_selector;
        self
    }

    pub fn run_policy(mut self, run_policy: RunPolicy) -> Self {
        self.run_policy = run_policy;
        self
    }

    pub fn schema_version(mut self, schema_version: u32) -> Self {
        self.schema_version = schema_version;
        self
    }

    pub fn harness_version(mut self, harness_version: impl Into<String>) -> Self {
        self.harness_version = harness_version.into();
        self
    }

    pub fn build(self) -> Result<Harness, HarnessError> {
        self.run_policy.validate()?;

        let provider = self
            .provider
            .ok_or_else(|| HarnessError::not_ready("provider is required to build chat runtime"))?;

        let store = Arc::new(MemoryConversationStore::new(self.memory.clone()));
        let mut chat_builder = ChatService::builder(provider)
            .store(store)
            .policy(self.chat_policy);

        if let Some(tool_runtime) = self.tool_runtime {
            chat_builder = chat_builder.tool_runtime(tool_runtime);
        }

        let chat = Arc::new(chat_builder.build());

        Ok(Harness {
            memory: self.memory,
            chat: Some(chat),
            health_checker: self.health_checker,
            validator: self.validator,
            feature_selector: self.feature_selector,
            run_policy: self.run_policy,
            schema_version: self.schema_version,
            harness_version: self.harness_version,
        })
    }
}

#[derive(Clone)]
pub struct Harness {
    memory: Arc<dyn MemoryBackend>,
    chat: Option<Arc<ChatService>>,
    health_checker: Arc<dyn HealthChecker>,
    validator: Arc<dyn OutcomeValidator>,
    feature_selector: Arc<dyn FeatureSelector>,
    run_policy: RunPolicy,
    schema_version: u32,
    harness_version: String,
}

impl Harness {
    pub fn default_init_plan() -> InitPlan {
        InitPlan::new(vec![
            InitStep::command("git", ["status", "--short", "--branch"]),
            InitStep::command("git", ["log", "--oneline", "-20"]),
        ])
    }

    pub fn new(memory: Arc<dyn MemoryBackend>) -> Self {
        Self {
            memory,
            chat: None,
            health_checker: Arc::new(NoopHealthChecker),
            validator: Arc::new(AcceptAllValidator),
            feature_selector: Arc::new(FirstPendingFeatureSelector),
            run_policy: RunPolicy::default(),
            schema_version: SessionManifest::DEFAULT_SCHEMA_VERSION,
            harness_version: SessionManifest::DEFAULT_HARNESS_VERSION.to_string(),
        }
    }

    pub fn builder(memory: Arc<dyn MemoryBackend>) -> HarnessBuilder {
        HarnessBuilder::new(memory)
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

    pub fn with_feature_selector(mut self, feature_selector: Arc<dyn FeatureSelector>) -> Self {
        self.feature_selector = feature_selector;
        self
    }

    pub fn with_run_policy(mut self, run_policy: RunPolicy) -> Result<Self, HarnessError> {
        run_policy.validate()?;
        self.run_policy = run_policy;
        Ok(self)
    }

    pub fn with_schema_version(mut self, schema_version: u32) -> Self {
        self.schema_version = schema_version;
        self
    }

    pub fn with_harness_version(mut self, harness_version: impl Into<String>) -> Self {
        self.harness_version = harness_version.into();
        self
    }

    pub async fn select_phase(&self, session_id: &SessionId) -> Result<HarnessPhase, HarnessError> {
        if self.memory.is_initialized(session_id).await? {
            Ok(HarnessPhase::TaskIteration)
        } else {
            Ok(HarnessPhase::Initializer)
        }
    }

    pub async fn run(&self, request: RuntimeRunRequest) -> Result<RuntimeRunOutcome, HarnessError> {
        self.run_with_observer(request, None).await
    }

    pub async fn run_with_observer(
        &self,
        request: RuntimeRunRequest,
        event_observer: Option<ChatEventObserver>,
    ) -> Result<RuntimeRunOutcome, HarnessError> {
        let phase = self.select_phase(&request.session.id).await?;
        match phase {
            HarnessPhase::Initializer => {
                let mut initializer = InitializerRequest::new(
                    request.session.id.clone(),
                    request.run_id.clone(),
                    request.current_objective,
                )
                .with_active_branch(request.active_branch);

                if let Some(init_plan) = request.init_plan {
                    initializer = initializer.with_init_plan(init_plan);
                }

                if !request.feature_list.is_empty() {
                    initializer = initializer.with_feature_list(request.feature_list);
                }

                if let Some(progress_summary) = request.progress_summary {
                    initializer = initializer.with_progress_summary(progress_summary);
                }

                self.run_initializer(initializer)
                    .await
                    .map(RuntimeRunOutcome::Initializer)
            }
            HarnessPhase::TaskIteration => {
                let mut task_iteration = TaskIterationRequest::new(request.session, request.run_id);
                if request.stream {
                    task_iteration = task_iteration.enable_streaming();
                }

                if let Some(prompt_override) = request.prompt_override {
                    task_iteration = task_iteration.with_prompt_override(prompt_override);
                }

                self.run_task_iteration_with_observer(task_iteration, event_observer)
                    .await
                    .map(RuntimeRunOutcome::TaskIteration)
            }
        }
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
            init_plan,
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

        let init_plan = init_plan.unwrap_or_else(Self::default_init_plan);

        let mut manifest =
            SessionManifest::new(session_id.clone(), active_branch, current_objective)
                .with_schema_version(self.schema_version)
                .with_harness_version(self.harness_version.clone());
        manifest.init_plan = Some(init_plan);

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

    pub async fn run_task_iteration(
        &self,
        request: TaskIterationRequest,
    ) -> Result<TaskIterationResult, HarnessError> {
        self.run_task_iteration_with_observer(request, None).await
    }

    pub async fn run_task_iteration_with_observer(
        &self,
        request: TaskIterationRequest,
        event_observer: Option<ChatEventObserver>,
    ) -> Result<TaskIterationResult, HarnessError> {
        let chat = self
            .chat
            .as_ref()
            .ok_or_else(|| HarnessError::not_ready("chat service is not configured in harness"))?;

        let started_at = SystemTime::now();
        self.memory
            .record_run_checkpoint(
                &request.session.id,
                RunCheckpoint::started(request.run_id.clone()),
            )
            .await?;

        let result = self
            .run_task_iteration_inner(chat, &request, event_observer)
            .await;

        match &result {
            Ok(value) => {
                let (status, note) = if value.no_pending_features {
                    (
                        RunStatus::Succeeded,
                        "All required features pass=true in feature_list; completion gate satisfied"
                            .to_string(),
                    )
                } else if value.validated {
                    (
                        RunStatus::Succeeded,
                        format!(
                            "Feature '{}' validated and marked passing; remaining required features still pending",
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
                    "Create init plan metadata",
                    "Create session manifest",
                    "Create starter feature list",
                ],
            ),
            feature(
                "harness.baseline",
                "functional",
                "Baseline harness checks can run before task iterations",
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

    async fn run_task_iteration_inner(
        &self,
        chat: &ChatService,
        request: &TaskIterationRequest,
        event_observer: Option<ChatEventObserver>,
    ) -> Result<TaskIterationResult, HarnessError> {
        let bootstrap = self
            .memory
            .load_bootstrap_state(&request.session.id)
            .await?;
        let manifest = bootstrap.manifest.ok_or_else(|| {
            HarnessError::not_ready("session is not initialized; run initializer first")
        })?;

        let init_plan = manifest
            .init_plan
            .clone()
            .unwrap_or_else(Self::default_init_plan);
        if let Err(error) = self
            .health_checker
            .run(&request.session.id, &init_plan)
            .await
        {
            if self.run_policy.fail_fast.on_health_check_error {
                return Err(error);
            }
        }

        if all_required_features_passed(&bootstrap.feature_list) {
            return Ok(TaskIterationResult {
                session_id: request.session.id.clone(),
                selected_feature_id: None,
                validated: true,
                no_pending_features: true,
                used_stream: request.stream,
                assistant_message: None,
            });
        }

        let feature = self.feature_selector.select(&bootstrap.feature_list);

        let Some(feature) = feature else {
            return Err(HarnessError::validation(
                "feature selector returned no work before required features reached passes=true",
            ));
        };

        let mut turns_used = 0usize;
        let mut retries_remaining = self.run_policy.retry_budget;

        while turns_used < self.run_policy.max_turns_per_run {
            turns_used += 1;

            let prompt = request
                .prompt_override
                .clone()
                .unwrap_or_else(|| build_feature_prompt(&feature, &manifest.current_objective));

            let turn_request = if request.stream {
                ChatTurnRequest::builder(request.session.clone(), prompt)
                    .enable_streaming()
                    .build()
            } else {
                ChatTurnRequest::builder(request.session.clone(), prompt).build()
            };

            let turn_result = match self
                .execute_turn(chat, turn_request, event_observer.clone())
                .await
            {
                Ok(result) => result,
                Err(error) => {
                    if self.run_policy.fail_fast.on_chat_error
                        || retries_remaining == 0
                        || turns_used >= self.run_policy.max_turns_per_run
                    {
                        return Err(error);
                    }

                    retries_remaining -= 1;
                    continue;
                }
            };

            let validated = self.validator.validate(&feature, &turn_result).await?;
            if validated {
                self.memory
                    .update_feature_pass(&request.session.id, &feature.id, true)
                    .await?;
                let all_features_passing = self
                    .session_all_required_features_passed(&request.session.id)
                    .await?;

                return Ok(TaskIterationResult {
                    session_id: request.session.id.clone(),
                    selected_feature_id: Some(feature.id.clone()),
                    validated: true,
                    no_pending_features: all_features_passing,
                    used_stream: request.stream,
                    assistant_message: Some(turn_result.assistant_message),
                });
            }

            if self.run_policy.fail_fast.on_validation_failure
                || retries_remaining == 0
                || turns_used >= self.run_policy.max_turns_per_run
            {
                return Ok(TaskIterationResult {
                    session_id: request.session.id.clone(),
                    selected_feature_id: Some(feature.id.clone()),
                    validated: false,
                    no_pending_features: false,
                    used_stream: request.stream,
                    assistant_message: Some(turn_result.assistant_message),
                });
            }

            retries_remaining -= 1;
        }

        Ok(TaskIterationResult {
            session_id: request.session.id.clone(),
            selected_feature_id: Some(feature.id),
            validated: false,
            no_pending_features: false,
            used_stream: request.stream,
            assistant_message: None,
        })
    }

    async fn execute_turn(
        &self,
        chat: &ChatService,
        turn_request: ChatTurnRequest,
        event_observer: Option<ChatEventObserver>,
    ) -> Result<ChatTurnResult, HarnessError> {
        if turn_request.options.stream {
            let mut stream = chat.stream_turn(turn_request).await?;
            let mut final_result = None;
            while let Some(item) = stream.next().await {
                match item {
                    Ok(event) => {
                        if let Some(observer) = event_observer.as_ref() {
                            observer(event.clone());
                        }
                        if let ChatEvent::TurnComplete(turn_result) = event {
                            final_result = Some(turn_result);
                        }
                    }
                    Err(err) => return Err(HarnessError::from(err)),
                }
            }

            final_result
                .ok_or_else(|| HarnessError::chat("stream ended without TurnComplete event"))
        } else {
            chat.run_turn(turn_request)
                .await
                .map_err(HarnessError::from)
        }
    }

    async fn session_all_required_features_passed(
        &self,
        session_id: &SessionId,
    ) -> Result<bool, HarnessError> {
        let bootstrap = self.memory.load_bootstrap_state(session_id).await?;
        Ok(all_required_features_passed(&bootstrap.feature_list))
    }

    async fn record_final_handoff(
        &self,
        request: &TaskIterationRequest,
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
            .append_progress_entry(
                &request.session.id,
                ProgressEntry::new(request.run_id.clone(), note),
            )
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

fn all_required_features_passed(feature_list: &[FeatureRecord]) -> bool {
    !feature_list.is_empty() && feature_list.iter().all(|feature| feature.passes)
}

#[cfg(test)]
mod tests {
    use fmemory::FeatureRecord;

    use super::{all_required_features_passed, validate_feature_list};

    #[test]
    fn validate_feature_list_rejects_empty_input() {
        let error = validate_feature_list(&[]).expect_err("empty list should fail");
        assert_eq!(error.message, "feature_list must contain at least one feature");
    }

    #[test]
    fn all_required_features_passed_requires_non_empty_and_all_true() {
        assert!(!all_required_features_passed(&[]));

        let mixed = vec![
            FeatureRecord {
                id: "a".to_string(),
                category: "functional".to_string(),
                description: "a".to_string(),
                steps: vec!["x".to_string()],
                passes: true,
            },
            FeatureRecord {
                id: "b".to_string(),
                category: "functional".to_string(),
                description: "b".to_string(),
                steps: vec!["y".to_string()],
                passes: false,
            },
        ];
        assert!(!all_required_features_passed(&mixed));

        let all_passing = vec![FeatureRecord {
            id: "c".to_string(),
            category: "functional".to_string(),
            description: "c".to_string(),
            steps: vec!["z".to_string()],
            passes: true,
        }];
        assert!(all_required_features_passed(&all_passing));
    }
}
