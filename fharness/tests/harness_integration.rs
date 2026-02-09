use std::sync::Arc;

use fchat::ChatSession;
use fcommon::{BoxFuture, SessionId};
use fharness::{
    Harness, InitializerRequest, OutcomeValidator, RunPolicy, RuntimeRunOutcome, RuntimeRunRequest,
    TaskIterationRequest,
};
use fmemory::{FeatureRecord, InMemoryMemoryBackend, MemoryBackend};
use fprovider::{
    Message, ModelProvider, ModelRequest, ModelResponse, OutputItem, ProviderFuture, ProviderId,
    StopReason, TokenUsage,
};

#[derive(Debug)]
struct FixedAssistantProvider;

impl ModelProvider for FixedAssistantProvider {
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
        _request: ModelRequest,
    ) -> ProviderFuture<'a, Result<fprovider::BoxedEventStream<'a>, fprovider::ProviderError>> {
        Box::pin(async {
            Err(fprovider::ProviderError::invalid_request(
                "streaming not needed for this integration test provider",
            ))
        })
    }
}

struct AlwaysFailValidator;

impl OutcomeValidator for AlwaysFailValidator {
    fn validate<'a>(
        &'a self,
        _feature: &'a FeatureRecord,
        _result: &'a fchat::ChatTurnResult,
    ) -> BoxFuture<'a, Result<bool, fharness::HarnessError>> {
        Box::pin(async { Ok(false) })
    }
}

fn feature(id: &str) -> FeatureRecord {
    FeatureRecord {
        id: id.to_string(),
        category: "functional".to_string(),
        description: format!("implement {id}"),
        steps: vec!["write code".to_string()],
        passes: false,
    }
}

#[tokio::test]
async fn initializer_creates_required_artifacts() {
    let memory: Arc<dyn MemoryBackend> = Arc::new(InMemoryMemoryBackend::new());
    let harness = Harness::new(memory.clone());

    let result = harness
        .run_initializer(
            InitializerRequest::new("integration-init", "run-init-1", "ship integration tests")
                .with_feature_list(vec![feature("feature-1")]),
        )
        .await
        .expect("initializer should succeed");

    assert!(result.created);

    let bootstrap = memory
        .load_bootstrap_state(&SessionId::from("integration-init"))
        .await
        .expect("bootstrap state should load");

    assert!(bootstrap.manifest.is_some());
    assert_eq!(bootstrap.feature_list.len(), 1);
    assert_eq!(bootstrap.feature_list[0].id, "feature-1");
    assert!(
        bootstrap
            .recent_progress
            .iter()
            .any(|entry| entry.run_id == "run-init-1")
    );
    assert!(
        bootstrap
            .checkpoints
            .iter()
            .any(|cp| cp.run_id == "run-init-1")
    );
}

#[tokio::test]
async fn task_iteration_picks_one_failing_feature_and_updates_progress() {
    let memory: Arc<dyn MemoryBackend> = Arc::new(InMemoryMemoryBackend::new());
    let harness = Harness::builder(memory.clone())
        .provider(Arc::new(FixedAssistantProvider))
        .build()
        .expect("builder should succeed");

    harness
        .run_initializer(
            InitializerRequest::new("integration-code-1", "run-init", "implement features")
                .with_feature_list(vec![feature("feature-a"), feature("feature-b")]),
        )
        .await
        .expect("initializer should succeed");

    let session = ChatSession::new("integration-code-1", ProviderId::OpenAi, "gpt-4o-mini");
    let result = harness
        .run_task_iteration(TaskIterationRequest::new(session, "run-code-1"))
        .await
        .expect("task iteration should succeed");

    assert_eq!(result.selected_feature_id.as_deref(), Some("feature-a"));
    assert_eq!(result.processed_feature_ids, vec!["feature-a".to_string()]);
    assert_eq!(result.validated_feature_ids, vec!["feature-a".to_string()]);
    assert_eq!(result.processed_feature_count, 1);
    assert!(result.validated);
    assert!(!result.no_pending_features);

    let bootstrap = memory
        .load_bootstrap_state(&SessionId::from("integration-code-1"))
        .await
        .expect("bootstrap state should load");

    assert!(
        bootstrap
            .feature_list
            .iter()
            .any(|f| f.id == "feature-a" && f.passes)
    );
    assert!(
        bootstrap
            .feature_list
            .iter()
            .any(|f| f.id == "feature-b" && !f.passes)
    );
    assert!(
        bootstrap
            .recent_progress
            .iter()
            .any(|entry| entry.run_id == "run-code-1")
    );
    assert!(
        bootstrap
            .checkpoints
            .iter()
            .any(|cp| cp.run_id == "run-code-1" && cp.completed_at.is_some())
    );
}

#[tokio::test]
async fn bounded_batch_mode_processes_up_to_feature_limit() {
    let memory: Arc<dyn MemoryBackend> = Arc::new(InMemoryMemoryBackend::new());
    let harness = Harness::builder(memory.clone())
        .provider(Arc::new(FixedAssistantProvider))
        .run_policy(RunPolicy::bounded_batch(2))
        .build()
        .expect("builder should succeed");

    harness
        .run_initializer(
            InitializerRequest::new("integration-bounded", "run-init", "implement features")
                .with_feature_list(vec![
                    feature("feature-a"),
                    feature("feature-b"),
                    feature("feature-c"),
                ]),
        )
        .await
        .expect("initializer should succeed");

    let session = ChatSession::new("integration-bounded", ProviderId::OpenAi, "gpt-4o-mini");
    let result = harness
        .run_task_iteration(TaskIterationRequest::new(session, "run-code-1"))
        .await
        .expect("task iteration should succeed");

    assert_eq!(result.processed_feature_count, 2);
    assert_eq!(result.processed_feature_ids.len(), 2);
    assert_eq!(result.validated_feature_ids.len(), 2);
    assert!(result.validated);
    assert!(!result.no_pending_features);

    let bootstrap = memory
        .load_bootstrap_state(&SessionId::from("integration-bounded"))
        .await
        .expect("bootstrap state should load");

    assert!(
        bootstrap
            .feature_list
            .iter()
            .any(|f| f.id == "feature-a" && f.passes)
    );
    assert!(
        bootstrap
            .feature_list
            .iter()
            .any(|f| f.id == "feature-b" && f.passes)
    );
    assert!(
        bootstrap
            .feature_list
            .iter()
            .any(|f| f.id == "feature-c" && !f.passes)
    );
}

#[tokio::test]
async fn unlimited_batch_mode_processes_all_pending_features() {
    let memory: Arc<dyn MemoryBackend> = Arc::new(InMemoryMemoryBackend::new());
    let harness = Harness::builder(memory.clone())
        .provider(Arc::new(FixedAssistantProvider))
        .run_policy(RunPolicy::unlimited_batch())
        .build()
        .expect("builder should succeed");

    harness
        .run_initializer(
            InitializerRequest::new("integration-unlimited", "run-init", "implement features")
                .with_feature_list(vec![
                    feature("feature-a"),
                    feature("feature-b"),
                    feature("feature-c"),
                ]),
        )
        .await
        .expect("initializer should succeed");

    let session = ChatSession::new("integration-unlimited", ProviderId::OpenAi, "gpt-4o-mini");
    let result = harness
        .run_task_iteration(TaskIterationRequest::new(session, "run-code-1"))
        .await
        .expect("task iteration should succeed");

    assert_eq!(result.processed_feature_count, 3);
    assert_eq!(result.processed_feature_ids.len(), 3);
    assert_eq!(result.validated_feature_ids.len(), 3);
    assert!(result.validated);
    assert!(result.no_pending_features);

    let bootstrap = memory
        .load_bootstrap_state(&SessionId::from("integration-unlimited"))
        .await
        .expect("bootstrap state should load");
    assert!(bootstrap.feature_list.iter().all(|f| f.passes));
}

#[tokio::test]
async fn task_iteration_does_not_mark_feature_pass_without_validation() {
    let memory: Arc<dyn MemoryBackend> = Arc::new(InMemoryMemoryBackend::new());
    let harness = Harness::builder(memory.clone())
        .provider(Arc::new(FixedAssistantProvider))
        .validator(Arc::new(AlwaysFailValidator))
        .build()
        .expect("builder should succeed");

    harness
        .run_initializer(
            InitializerRequest::new("integration-no-pass", "run-init", "validate gate")
                .with_feature_list(vec![feature("feature-1")]),
        )
        .await
        .expect("initializer should succeed");

    let session = ChatSession::new("integration-no-pass", ProviderId::OpenAi, "gpt-4o-mini");
    let result = harness
        .run_task_iteration(TaskIterationRequest::new(session, "run-code-1"))
        .await
        .expect("task iteration should complete");

    assert!(!result.validated);

    let bootstrap = memory
        .load_bootstrap_state(&SessionId::from("integration-no-pass"))
        .await
        .expect("bootstrap should load");
    assert!(!bootstrap.feature_list[0].passes);
}

#[tokio::test]
async fn fresh_context_window_recovers_state_from_fmemory_and_continues() {
    let memory: Arc<dyn MemoryBackend> = Arc::new(InMemoryMemoryBackend::new());

    let harness_a = Harness::builder(memory.clone())
        .provider(Arc::new(FixedAssistantProvider))
        .build()
        .expect("builder should succeed");

    harness_a
        .run_initializer(
            InitializerRequest::new("integration-fresh", "run-init", "continue across windows")
                .with_feature_list(vec![feature("feature-1"), feature("feature-2")]),
        )
        .await
        .expect("initializer should succeed");

    let session_a = ChatSession::new("integration-fresh", ProviderId::OpenAi, "gpt-4o-mini");
    let first = harness_a
        .run_task_iteration(TaskIterationRequest::new(session_a, "run-code-1"))
        .await
        .expect("first run should succeed");
    assert_eq!(first.selected_feature_id.as_deref(), Some("feature-1"));

    let harness_b = Harness::builder(memory.clone())
        .provider(Arc::new(FixedAssistantProvider))
        .build()
        .expect("new harness should rebuild from same memory");

    let session_b = ChatSession::new("integration-fresh", ProviderId::OpenAi, "gpt-4o-mini");
    let second = harness_b
        .run_task_iteration(TaskIterationRequest::new(session_b, "run-code-2"))
        .await
        .expect("second run should continue from memory state");
    assert_eq!(second.selected_feature_id.as_deref(), Some("feature-2"));

    let bootstrap = memory
        .load_bootstrap_state(&SessionId::from("integration-fresh"))
        .await
        .expect("bootstrap should load");
    assert!(bootstrap.feature_list.iter().all(|f| f.passes));
}

#[tokio::test]
async fn multi_run_completion_requires_all_features_passed() {
    let memory: Arc<dyn MemoryBackend> = Arc::new(InMemoryMemoryBackend::new());
    let harness = Harness::builder(memory)
        .provider(Arc::new(FixedAssistantProvider))
        .build()
        .expect("builder should succeed");

    let session = ChatSession::new("integration-multi", ProviderId::OpenAi, "gpt-4o-mini");

    let init_outcome = harness
        .run(
            RuntimeRunRequest::new(
                session.clone(),
                "run-init",
                "complete all required features",
            )
            .with_feature_list(vec![feature("feature-1"), feature("feature-2")]),
        )
        .await
        .expect("initializer phase should run");
    assert!(matches!(init_outcome, RuntimeRunOutcome::Initializer(_)));

    let first_outcome = harness
        .run(RuntimeRunRequest::new(
            session.clone(),
            "run-code-1",
            "complete all required features",
        ))
        .await
        .expect("first task-iteration phase should run");

    let first = match first_outcome {
        RuntimeRunOutcome::TaskIteration(value) => value,
        RuntimeRunOutcome::Initializer(_) => panic!("expected task-iteration outcome"),
    };
    assert!(first.validated);
    assert!(!first.no_pending_features);

    let second_outcome = harness
        .run(RuntimeRunRequest::new(
            session,
            "run-code-2",
            "complete all required features",
        ))
        .await
        .expect("second task-iteration phase should run");

    let second = match second_outcome {
        RuntimeRunOutcome::TaskIteration(value) => value,
        RuntimeRunOutcome::Initializer(_) => panic!("expected task-iteration outcome"),
    };
    assert!(second.validated);
    assert!(second.no_pending_features);
}
