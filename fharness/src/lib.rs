//! Agent harness orchestration APIs.

mod error;
mod harness;
mod traits;
mod types;

use std::sync::Arc;

use fchat::ChatEvent;

pub use error::{HarnessError, HarnessErrorKind};
pub use harness::{Harness, HarnessBuilder};
pub use traits::{
    AcceptAllValidator, FeatureSelector, FirstPendingFeatureSelector, HealthChecker,
    NoopHealthChecker, OutcomeValidator,
};
pub use types::{
    FailFastPolicy, HarnessPhase, InitializerRequest, InitializerResult, RunPolicy,
    RuntimeRunOutcome, RuntimeRunRequest, TaskIterationRequest, TaskIterationResult,
};

pub type ChatEventObserver = Arc<dyn Fn(ChatEvent) + Send + Sync>;

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use fchat::{ChatPolicy, ChatService, ChatSession, ChatTurnResult, InMemoryConversationStore};
    use fcommon::{BoxFuture, SessionId};
    use fmemory::{FeatureRecord, InMemoryMemoryBackend, MemoryBackend, SessionManifest};
    use fprovider::{
        Message, ModelProvider, ModelRequest, ModelResponse, OutputItem, ProviderFuture,
        ProviderId, StopReason, StreamEvent, TokenUsage, ToolCall, VecEventStream,
    };
    use ftooling::{ToolError, ToolExecutionContext, ToolExecutionResult, ToolFuture, ToolRuntime};

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
        ) -> ProviderFuture<'a, Result<fprovider::BoxedEventStream<'a>, fprovider::ProviderError>>
        {
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
    struct RecordingProvider {
        requests: Mutex<Vec<ModelRequest>>,
    }

    impl RecordingProvider {
        fn latest_request(&self) -> ModelRequest {
            self.requests
                .lock()
                .expect("requests lock")
                .last()
                .cloned()
                .expect("at least one request")
        }
    }

    impl ModelProvider for RecordingProvider {
        fn id(&self) -> ProviderId {
            ProviderId::OpenAi
        }

        fn complete<'a>(
            &'a self,
            request: ModelRequest,
        ) -> ProviderFuture<'a, Result<ModelResponse, fprovider::ProviderError>> {
            Box::pin(async move {
                self.requests
                    .lock()
                    .expect("requests lock")
                    .push(request.clone());
                Ok(ModelResponse {
                    provider: ProviderId::OpenAi,
                    model: request.model,
                    output: vec![OutputItem::Message(Message::new(
                        fprovider::Role::Assistant,
                        "recorded",
                    ))],
                    stop_reason: StopReason::EndTurn,
                    usage: TokenUsage::default(),
                })
            })
        }

        fn stream<'a>(
            &'a self,
            request: ModelRequest,
        ) -> ProviderFuture<'a, Result<fprovider::BoxedEventStream<'a>, fprovider::ProviderError>>
        {
            Box::pin(async move {
                self.requests
                    .lock()
                    .expect("requests lock")
                    .push(request.clone());
                let response = ModelResponse {
                    provider: ProviderId::OpenAi,
                    model: request.model,
                    output: vec![OutputItem::Message(Message::new(
                        fprovider::Role::Assistant,
                        "recorded-stream",
                    ))],
                    stop_reason: StopReason::EndTurn,
                    usage: TokenUsage::default(),
                };
                let stream = VecEventStream::new(vec![Ok(StreamEvent::ResponseComplete(response))]);
                Ok(Box::pin(stream) as fprovider::BoxedEventStream<'a>)
            })
        }
    }

    #[derive(Debug)]
    struct ToolLoopProvider;

    impl ModelProvider for ToolLoopProvider {
        fn id(&self) -> ProviderId {
            ProviderId::OpenAi
        }

        fn complete<'a>(
            &'a self,
            request: ModelRequest,
        ) -> ProviderFuture<'a, Result<ModelResponse, fprovider::ProviderError>> {
            Box::pin(async move {
                if request.tool_results.is_empty() {
                    Ok(ModelResponse {
                        provider: ProviderId::OpenAi,
                        model: request.model,
                        output: vec![OutputItem::ToolCall(ToolCall {
                            id: "call_tool_1".to_string(),
                            name: "echo".to_string(),
                            arguments: "{}".to_string(),
                        })],
                        stop_reason: StopReason::EndTurn,
                        usage: TokenUsage::default(),
                    })
                } else {
                    Ok(ModelResponse {
                        provider: ProviderId::OpenAi,
                        model: request.model,
                        output: vec![OutputItem::Message(Message::new(
                            fprovider::Role::Assistant,
                            "tool-complete",
                        ))],
                        stop_reason: StopReason::EndTurn,
                        usage: TokenUsage::default(),
                    })
                }
            })
        }

        fn stream<'a>(
            &'a self,
            request: ModelRequest,
        ) -> ProviderFuture<'a, Result<fprovider::BoxedEventStream<'a>, fprovider::ProviderError>>
        {
            Box::pin(async move {
                let response = ModelResponse {
                    provider: ProviderId::OpenAi,
                    model: request.model,
                    output: vec![OutputItem::Message(Message::new(
                        fprovider::Role::Assistant,
                        "tool-complete",
                    ))],
                    stop_reason: StopReason::EndTurn,
                    usage: TokenUsage::default(),
                };

                let stream = VecEventStream::new(vec![Ok(StreamEvent::ResponseComplete(response))]);
                Ok(Box::pin(stream) as fprovider::BoxedEventStream<'a>)
            })
        }
    }

    #[derive(Debug, Default)]
    struct EchoToolRuntime;

    impl ToolRuntime for EchoToolRuntime {
        fn execute<'a>(
            &'a self,
            tool_call: ToolCall,
            _context: ToolExecutionContext,
        ) -> ToolFuture<'a, Result<ToolExecutionResult, ToolError>> {
            Box::pin(async move {
                Ok(ToolExecutionResult {
                    tool_call_id: tool_call.id,
                    output: "ok".to_string(),
                })
            })
        }
    }

    #[derive(Debug, Default)]
    struct LastPendingFeatureSelector;

    impl FeatureSelector for LastPendingFeatureSelector {
        fn select(&self, feature_list: &[FeatureRecord]) -> Option<FeatureRecord> {
            feature_list
                .iter()
                .rev()
                .find(|feature| !feature.passes)
                .cloned()
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

    #[derive(Debug, Default)]
    struct NeverSelectFeature;

    impl FeatureSelector for NeverSelectFeature {
        fn select(&self, _feature_list: &[FeatureRecord]) -> Option<FeatureRecord> {
            None
        }
    }

    #[derive(Debug, Default)]
    struct EventuallyPassingValidator {
        calls: Mutex<usize>,
        pass_on_call: usize,
    }

    impl EventuallyPassingValidator {
        fn new(pass_on_call: usize) -> Self {
            Self {
                calls: Mutex::new(0),
                pass_on_call,
            }
        }
    }

    impl OutcomeValidator for EventuallyPassingValidator {
        fn validate<'a>(
            &'a self,
            _feature: &'a FeatureRecord,
            _result: &'a ChatTurnResult,
        ) -> BoxFuture<'a, Result<bool, HarnessError>> {
            Box::pin(async move {
                let mut calls = self.calls.lock().expect("calls lock");
                *calls += 1;
                Ok(*calls >= self.pass_on_call)
            })
        }
    }

    #[derive(Debug, Default)]
    struct FlakyCompletionProvider {
        attempts: Mutex<usize>,
        fail_for_attempts: usize,
    }

    impl FlakyCompletionProvider {
        fn new(fail_for_attempts: usize) -> Self {
            Self {
                attempts: Mutex::new(0),
                fail_for_attempts,
            }
        }
    }

    impl ModelProvider for FlakyCompletionProvider {
        fn id(&self) -> ProviderId {
            ProviderId::OpenAi
        }

        fn complete<'a>(
            &'a self,
            request: ModelRequest,
        ) -> ProviderFuture<'a, Result<ModelResponse, fprovider::ProviderError>> {
            Box::pin(async move {
                let mut attempts = self.attempts.lock().expect("attempts lock");
                *attempts += 1;
                if *attempts <= self.fail_for_attempts {
                    return Err(fprovider::ProviderError::timeout("transient failure"));
                }

                Ok(ModelResponse {
                    provider: ProviderId::OpenAi,
                    model: request.model,
                    output: vec![OutputItem::Message(Message::new(
                        fprovider::Role::Assistant,
                        "eventual-success",
                    ))],
                    stop_reason: StopReason::EndTurn,
                    usage: TokenUsage::default(),
                })
            })
        }

        fn stream<'a>(
            &'a self,
            _request: ModelRequest,
        ) -> ProviderFuture<'a, Result<fprovider::BoxedEventStream<'a>, fprovider::ProviderError>>
        {
            Box::pin(async {
                Err(fprovider::ProviderError::invalid_request(
                    "stream not used in flaky completion provider",
                ))
            })
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
                InitializerRequest::new(session_id, "run-init", "prepare task iteration")
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

        let request = InitializerRequest::new("session-1", "run-1", "Build initializer flow")
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
        assert_eq!(
            result.schema_version,
            SessionManifest::DEFAULT_SCHEMA_VERSION
        );

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

        let first =
            InitializerRequest::new("session-2", "run-1", "Initialize").with_feature_list(vec![
                FeatureRecord {
                    id: "feature-a".to_string(),
                    category: "functional".to_string(),
                    description: "first".to_string(),
                    steps: vec!["step".to_string()],
                    passes: false,
                },
            ]);

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

        let request = InitializerRequest::new("session-4", "run-1", "Build task-iteration harness");
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
                InitializerRequest::new("session-6", "run-1", "Init").with_feature_list(vec![
                    FeatureRecord {
                        id: "done".to_string(),
                        category: "functional".to_string(),
                        description: "already done".to_string(),
                        steps: vec!["step".to_string()],
                        passes: true,
                    },
                ]),
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
            .run_task_iteration(TaskIterationRequest::new(session, "run-code-1"))
            .await
            .expect("task iteration should succeed");

        assert!(result.no_pending_features);
        assert!(result.validated);
        assert_eq!(result.selected_feature_id.as_deref(), Some("feature-1"));

        let calls = health.calls.lock().expect("calls lock");
        assert_eq!(*calls, 1);

        let state = memory
            .load_bootstrap_state(&SessionId::from("session-coding"))
            .await
            .expect("state should load");
        assert!(state.feature_list[0].passes);
        assert!(
            state
                .recent_progress
                .iter()
                .any(|entry| entry.run_id == "run-code-1")
        );
        assert!(state.checkpoints.iter().any(
            |checkpoint| checkpoint.run_id == "run-code-1" && checkpoint.completed_at.is_some()
        ));
    }

    #[tokio::test]
    async fn coding_iteration_stream_path_works_and_records_handoff() {
        let memory: Arc<dyn MemoryBackend> = Arc::new(InMemoryMemoryBackend::new());
        let harness = build_harness(memory.clone(), None, None);
        initialize_for_tests(&harness, "session-stream").await;

        let session = ChatSession::new("session-stream", ProviderId::OpenAi, "gpt-4o-mini");
        let result = harness
            .run_task_iteration(
                TaskIterationRequest::new(session, "run-stream-1").enable_streaming(),
            )
            .await
            .expect("streaming task iteration should succeed");

        assert!(result.used_stream);
        assert!(result.validated);

        let state = memory
            .load_bootstrap_state(&SessionId::from("session-stream"))
            .await
            .expect("state should load");
        assert!(
            state
                .recent_progress
                .iter()
                .any(|entry| entry.run_id == "run-stream-1")
        );
    }

    #[tokio::test]
    async fn coding_iteration_does_not_mark_feature_when_not_validated() {
        let memory: Arc<dyn MemoryBackend> = Arc::new(InMemoryMemoryBackend::new());
        let harness = build_harness(memory.clone(), None, Some(Arc::new(AlwaysFailValidator)));
        initialize_for_tests(&harness, "session-unvalidated").await;

        let session = ChatSession::new("session-unvalidated", ProviderId::OpenAi, "gpt-4o-mini");
        let result = harness
            .run_task_iteration(TaskIterationRequest::new(session, "run-code-2"))
            .await
            .expect("task iteration should complete");

        assert!(!result.validated);

        let state = memory
            .load_bootstrap_state(&SessionId::from("session-unvalidated"))
            .await
            .expect("state should load");
        assert!(!state.feature_list[0].passes);
    }

    #[tokio::test]
    async fn builder_wires_provider_tooling_memory_and_chat() {
        let memory: Arc<dyn MemoryBackend> = Arc::new(InMemoryMemoryBackend::new());
        let harness = Harness::builder(memory.clone())
            .provider(Arc::new(ToolLoopProvider))
            .tool_runtime(Arc::new(EchoToolRuntime))
            .build()
            .expect("builder should wire runtime");

        initialize_for_tests(&harness, "session-builder").await;

        let session = ChatSession::new("session-builder", ProviderId::OpenAi, "gpt-4o-mini");
        let result = harness
            .run_task_iteration(TaskIterationRequest::new(session, "run-builder-1"))
            .await
            .expect("task iteration should succeed");

        assert_eq!(result.assistant_message.as_deref(), Some("tool-complete"));

        let transcript = memory
            .load_transcript_messages(&SessionId::from("session-builder"))
            .await
            .expect("transcript should load");
        assert_eq!(transcript.len(), 3);
    }

    #[tokio::test]
    async fn runtime_run_selects_initializer_then_task_iteration_phase() {
        let memory: Arc<dyn MemoryBackend> = Arc::new(InMemoryMemoryBackend::new());
        let harness = Harness::builder(memory.clone())
            .provider(Arc::new(FakeProvider))
            .build()
            .expect("builder should succeed");

        let session = ChatSession::new("session-runtime", ProviderId::OpenAi, "gpt-4o-mini");
        let request =
            RuntimeRunRequest::new(session.clone(), "run-auto-1", "phase selector objective")
                .with_feature_list(vec![FeatureRecord {
                    id: "feature-1".to_string(),
                    category: "functional".to_string(),
                    description: "phase selection".to_string(),
                    steps: vec!["initialize then code".to_string()],
                    passes: false,
                }]);

        let first = harness.run(request).await.expect("first phase should run");
        assert!(matches!(first, RuntimeRunOutcome::Initializer(_)));

        let second = harness
            .run(RuntimeRunRequest::new(
                session,
                "run-auto-2",
                "phase selector objective",
            ))
            .await
            .expect("second phase should run");

        assert!(matches!(second, RuntimeRunOutcome::TaskIteration(_)));
    }

    #[tokio::test]
    async fn coding_iteration_uses_feature_selection_strategy() {
        let memory: Arc<dyn MemoryBackend> = Arc::new(InMemoryMemoryBackend::new());
        let harness = build_harness(memory.clone(), None, None)
            .with_feature_selector(Arc::new(LastPendingFeatureSelector));

        harness
            .run_initializer(
                InitializerRequest::new("session-selector", "run-init", "feature strategy")
                    .with_feature_list(vec![
                        FeatureRecord {
                            id: "feature-a".to_string(),
                            category: "functional".to_string(),
                            description: "first pending".to_string(),
                            steps: vec!["do first".to_string()],
                            passes: false,
                        },
                        FeatureRecord {
                            id: "feature-b".to_string(),
                            category: "functional".to_string(),
                            description: "second pending".to_string(),
                            steps: vec!["do second".to_string()],
                            passes: false,
                        },
                    ]),
            )
            .await
            .expect("initializer should succeed");

        let session = ChatSession::new("session-selector", ProviderId::OpenAi, "gpt-4o-mini");
        let result = harness
            .run_task_iteration(TaskIterationRequest::new(session, "run-selector-1"))
            .await
            .expect("task iteration should succeed");

        assert_eq!(result.selected_feature_id.as_deref(), Some("feature-b"));
    }

    #[tokio::test]
    async fn builder_requires_provider_to_build_runtime() {
        let memory: Arc<dyn MemoryBackend> = Arc::new(InMemoryMemoryBackend::new());
        let error = Harness::builder(memory)
            .build()
            .err()
            .expect("provider should be required");

        assert_eq!(error.kind, HarnessErrorKind::NotReady);
    }

    #[tokio::test]
    async fn select_phase_tracks_session_initialization_state() {
        let memory: Arc<dyn MemoryBackend> = Arc::new(InMemoryMemoryBackend::new());
        let harness = Harness::builder(memory.clone())
            .provider(Arc::new(FakeProvider))
            .build()
            .expect("builder should succeed");

        let phase_before = harness
            .select_phase(&SessionId::from("session-phase"))
            .await
            .expect("phase should resolve");
        assert_eq!(phase_before, HarnessPhase::Initializer);

        harness
            .run_initializer(InitializerRequest::new(
                "session-phase",
                "run-init",
                "phase objective",
            ))
            .await
            .expect("initializer should succeed");

        let phase_after = harness
            .select_phase(&SessionId::from("session-phase"))
            .await
            .expect("phase should resolve");
        assert_eq!(phase_after, HarnessPhase::TaskIteration);
    }

    #[tokio::test]
    async fn runtime_run_initializer_applies_initializer_fields() {
        let memory: Arc<dyn MemoryBackend> = Arc::new(InMemoryMemoryBackend::new());
        let harness = Harness::builder(memory.clone())
            .provider(Arc::new(FakeProvider))
            .build()
            .expect("builder should succeed");

        let session = ChatSession::new("session-init-fields", ProviderId::OpenAi, "gpt-4o-mini");
        let outcome = harness
            .run(
                RuntimeRunRequest::new(session, "run-init-fields", "objective")
                    .with_active_branch("feature/custom")
                    .with_init_script("#!/usr/bin/env bash\necho init")
                    .with_progress_summary("custom summary")
                    .with_feature_list(vec![FeatureRecord {
                        id: "feature-custom".to_string(),
                        category: "functional".to_string(),
                        description: "custom feature".to_string(),
                        steps: vec!["step".to_string()],
                        passes: false,
                    }]),
            )
            .await
            .expect("runtime run should initialize");

        assert!(matches!(outcome, RuntimeRunOutcome::Initializer(_)));

        let state = memory
            .load_bootstrap_state(&SessionId::from("session-init-fields"))
            .await
            .expect("state should load");
        let manifest = state.manifest.expect("manifest should exist");
        assert_eq!(manifest.active_branch, "feature/custom");
        assert_eq!(
            manifest.init_script.as_deref(),
            Some("#!/usr/bin/env bash\necho init")
        );
        assert!(
            state
                .recent_progress
                .iter()
                .any(|entry| entry.summary == "custom summary")
        );
    }

    #[tokio::test]
    async fn runtime_run_forwards_prompt_override_and_streaming() {
        let memory: Arc<dyn MemoryBackend> = Arc::new(InMemoryMemoryBackend::new());
        let provider = Arc::new(RecordingProvider::default());
        let harness = Harness::builder(memory.clone())
            .provider(provider.clone())
            .build()
            .expect("builder should succeed");

        harness
            .run_initializer(
                InitializerRequest::new("session-runtime-prompt", "run-init", "objective")
                    .with_feature_list(vec![FeatureRecord {
                        id: "feature-1".to_string(),
                        category: "functional".to_string(),
                        description: "check prompt override".to_string(),
                        steps: vec!["override prompt".to_string()],
                        passes: false,
                    }]),
            )
            .await
            .expect("initializer should succeed");

        let session = ChatSession::new("session-runtime-prompt", ProviderId::OpenAi, "gpt-4o-mini");
        let outcome = harness
            .run(
                RuntimeRunRequest::new(session, "run-code", "objective")
                    .with_prompt_override("explicit prompt")
                    .enable_streaming(),
            )
            .await
            .expect("runtime run should code");

        assert!(matches!(outcome, RuntimeRunOutcome::TaskIteration(_)));

        let request = provider.latest_request();
        assert!(request.options.stream);
        let last_message = request.messages.last().expect("user message should exist");
        assert_eq!(last_message.role, fprovider::Role::User);
        assert_eq!(last_message.content, "explicit prompt");
    }

    #[tokio::test]
    async fn run_policy_enforces_strict_incremental_feature_limit() {
        let memory: Arc<dyn MemoryBackend> = Arc::new(InMemoryMemoryBackend::new());
        let error = Harness::builder(memory)
            .provider(Arc::new(FakeProvider))
            .run_policy(RunPolicy {
                max_turns_per_run: 1,
                max_features_per_run: 2,
                retry_budget: 0,
                fail_fast: FailFastPolicy::default(),
            })
            .build()
            .err()
            .expect("policy should reject non-incremental feature count");

        assert_eq!(error.kind, HarnessErrorKind::InvalidRequest);
    }

    #[tokio::test]
    async fn coding_iteration_retries_validation_when_policy_allows() {
        let memory: Arc<dyn MemoryBackend> = Arc::new(InMemoryMemoryBackend::new());
        let harness = build_harness(
            memory.clone(),
            None,
            Some(Arc::new(EventuallyPassingValidator::new(2))),
        )
        .with_run_policy(RunPolicy {
            max_turns_per_run: 3,
            max_features_per_run: 1,
            retry_budget: 2,
            fail_fast: FailFastPolicy {
                on_validation_failure: false,
                ..FailFastPolicy::default()
            },
        })
        .expect("run policy should be accepted");

        initialize_for_tests(&harness, "session-retry-validation").await;

        let session = ChatSession::new(
            "session-retry-validation",
            ProviderId::OpenAi,
            "gpt-4o-mini",
        );
        let result = harness
            .run_task_iteration(TaskIterationRequest::new(session, "run-retry-validation"))
            .await
            .expect("task iteration should succeed after retry");

        assert!(result.validated);
    }

    #[tokio::test]
    async fn coding_iteration_stops_when_turn_budget_is_exhausted() {
        let memory: Arc<dyn MemoryBackend> = Arc::new(InMemoryMemoryBackend::new());
        let harness = build_harness(memory.clone(), None, Some(Arc::new(AlwaysFailValidator)))
            .with_run_policy(RunPolicy {
                max_turns_per_run: 1,
                max_features_per_run: 1,
                retry_budget: 3,
                fail_fast: FailFastPolicy {
                    on_validation_failure: false,
                    ..FailFastPolicy::default()
                },
            })
            .expect("run policy should be accepted");

        initialize_for_tests(&harness, "session-turn-budget").await;

        let session = ChatSession::new("session-turn-budget", ProviderId::OpenAi, "gpt-4o-mini");
        let result = harness
            .run_task_iteration(TaskIterationRequest::new(session, "run-turn-budget"))
            .await
            .expect("task iteration should complete with validation failure");

        assert!(!result.validated);
    }

    #[tokio::test]
    async fn coding_iteration_retries_chat_errors_within_retry_budget() {
        let memory: Arc<dyn MemoryBackend> = Arc::new(InMemoryMemoryBackend::new());
        let harness = Harness::builder(memory.clone())
            .provider(Arc::new(FlakyCompletionProvider::new(1)))
            .validator(Arc::new(AcceptAllValidator))
            .run_policy(RunPolicy {
                max_turns_per_run: 3,
                max_features_per_run: 1,
                retry_budget: 1,
                fail_fast: FailFastPolicy {
                    on_chat_error: false,
                    ..FailFastPolicy::default()
                },
            })
            .build()
            .expect("builder should succeed");

        initialize_for_tests(&harness, "session-chat-retry").await;

        let session = ChatSession::new("session-chat-retry", ProviderId::OpenAi, "gpt-4o-mini");
        let result = harness
            .run_task_iteration(TaskIterationRequest::new(session, "run-chat-retry"))
            .await
            .expect("chat error should be retried successfully");

        assert!(result.validated);
    }

    #[tokio::test]
    async fn harness_does_not_declare_done_when_selector_returns_none_early() {
        let memory: Arc<dyn MemoryBackend> = Arc::new(InMemoryMemoryBackend::new());
        let harness = build_harness(memory.clone(), None, None)
            .with_feature_selector(Arc::new(NeverSelectFeature));
        initialize_for_tests(&harness, "session-no-early-done").await;

        let session = ChatSession::new("session-no-early-done", ProviderId::OpenAi, "gpt-4o-mini");
        let error = harness
            .run_task_iteration(TaskIterationRequest::new(session, "run-no-early-done"))
            .await
            .expect_err("selector returning none should fail completion gate");

        assert_eq!(error.kind, HarnessErrorKind::Validation);
    }

    #[tokio::test]
    async fn completion_gate_requires_all_features_to_pass_true() {
        let memory: Arc<dyn MemoryBackend> = Arc::new(InMemoryMemoryBackend::new());
        let harness = build_harness(memory.clone(), None, None);

        harness
            .run_initializer(
                InitializerRequest::new("session-completion-gate", "run-init", "completion gate")
                    .with_feature_list(vec![
                        FeatureRecord {
                            id: "feature-1".to_string(),
                            category: "functional".to_string(),
                            description: "first required feature".to_string(),
                            steps: vec!["implement 1".to_string()],
                            passes: false,
                        },
                        FeatureRecord {
                            id: "feature-2".to_string(),
                            category: "functional".to_string(),
                            description: "second required feature".to_string(),
                            steps: vec!["implement 2".to_string()],
                            passes: false,
                        },
                    ]),
            )
            .await
            .expect("initializer should succeed");

        let session =
            ChatSession::new("session-completion-gate", ProviderId::OpenAi, "gpt-4o-mini");
        let result = harness
            .run_task_iteration(TaskIterationRequest::new(session, "run-completion-gate"))
            .await
            .expect("task iteration should succeed");

        assert!(result.validated);
        assert!(!result.no_pending_features);
    }
}
