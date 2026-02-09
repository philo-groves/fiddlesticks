//! Stable, facade-owned builder for dynamic agent harness runtimes.

use std::sync::Arc;

use crate::{
    in_memory_backend, AcceptAllValidator, ChatPolicy, ChatService, FeatureSelector,
    FirstPendingFeatureSelector, Harness, HarnessError, HealthChecker, MemoryBackend,
    MemoryConversationStore, NoopHealthChecker, OutcomeValidator, RunPolicy, ToolRuntime,
};
use crate::{ModelProvider, RuntimeBundle};

pub type AgentRuntime = RuntimeBundle;

pub struct AgentHarnessBuilder {
    provider: Arc<dyn ModelProvider>,
    memory: Arc<dyn MemoryBackend>,
    tool_runtime: Option<Arc<dyn ToolRuntime>>,
    chat_policy: ChatPolicy,
    health_checker: Arc<dyn HealthChecker>,
    validator: Arc<dyn OutcomeValidator>,
    feature_selector: Arc<dyn FeatureSelector>,
    run_policy: RunPolicy,
    schema_version: Option<u32>,
    harness_version: Option<String>,
}

impl AgentHarnessBuilder {
    pub fn new(provider: Arc<dyn ModelProvider>) -> Self {
        Self {
            provider,
            memory: in_memory_backend(),
            tool_runtime: None,
            chat_policy: ChatPolicy::default(),
            health_checker: Arc::new(NoopHealthChecker),
            validator: Arc::new(AcceptAllValidator),
            feature_selector: Arc::new(FirstPendingFeatureSelector),
            run_policy: RunPolicy::default(),
            schema_version: None,
            harness_version: None,
        }
    }

    pub fn memory(mut self, memory: Arc<dyn MemoryBackend>) -> Self {
        self.memory = memory;
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
        self.schema_version = Some(schema_version);
        self
    }

    pub fn harness_version(mut self, harness_version: impl Into<String>) -> Self {
        self.harness_version = Some(harness_version.into());
        self
    }

    pub fn build(self) -> Result<AgentRuntime, HarnessError> {
        let store = Arc::new(MemoryConversationStore::new(Arc::clone(&self.memory)));

        let mut chat_builder = ChatService::builder(Arc::clone(&self.provider))
            .store(store)
            .policy(self.chat_policy.clone());

        let mut harness_builder = Harness::builder(Arc::clone(&self.memory))
            .provider(self.provider)
            .chat_policy(self.chat_policy)
            .health_checker(self.health_checker)
            .validator(self.validator)
            .feature_selector(self.feature_selector)
            .run_policy(self.run_policy);

        if let Some(schema_version) = self.schema_version {
            harness_builder = harness_builder.schema_version(schema_version);
        }

        if let Some(harness_version) = self.harness_version {
            harness_builder = harness_builder.harness_version(harness_version);
        }

        if let Some(runtime) = self.tool_runtime {
            chat_builder = chat_builder.tool_runtime(Arc::clone(&runtime));
            harness_builder = harness_builder.tool_runtime(runtime);
        }

        let chat = chat_builder.build();
        let harness = harness_builder.build()?;

        Ok(RuntimeBundle {
            memory: self.memory,
            chat,
            harness,
        })
    }
}
