//! Durable harness and conversation state records.

use std::collections::HashMap;
use std::time::SystemTime;

use fcommon::{MetadataMap, SessionId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureRecord {
    pub id: String,
    pub category: String,
    pub description: String,
    pub steps: Vec<String>,
    pub passes: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressEntry {
    pub run_id: String,
    pub summary: String,
    pub created_at: SystemTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitPlan {
    pub steps: Vec<InitStep>,
}

impl InitPlan {
    pub fn new(steps: Vec<InitStep>) -> Self {
        Self { steps }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InitStep {
    Command(InitCommand),
    Shell(InitShellScript),
}

impl InitStep {
    pub fn command(
        program: impl Into<String>,
        args: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self::Command(InitCommand::new(program, args))
    }

    pub fn shell(shell: InitShell, script: impl Into<String>) -> Self {
        Self::Shell(InitShellScript::new(shell, script))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitCommand {
    pub program: String,
    pub args: Vec<String>,
}

impl InitCommand {
    pub fn new(
        program: impl Into<String>,
        args: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitShellScript {
    pub shell: InitShell,
    pub script: String,
}

impl InitShellScript {
    pub fn new(shell: InitShell, script: impl Into<String>) -> Self {
        Self {
            shell,
            script: script.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitShell {
    Bash,
    Sh,
    Pwsh,
    Cmd,
}

impl ProgressEntry {
    pub fn new(run_id: impl Into<String>, summary: impl Into<String>) -> Self {
        Self {
            run_id: run_id.into(),
            summary: summary.into(),
            created_at: SystemTime::now(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionManifest {
    pub session_id: SessionId,
    pub schema_version: u32,
    pub harness_version: String,
    pub active_branch: String,
    pub current_objective: String,
    pub last_known_good_commit: Option<String>,
    pub init_plan: Option<InitPlan>,
    pub metadata: MetadataMap,
}

impl SessionManifest {
    pub const DEFAULT_SCHEMA_VERSION: u32 = 1;
    pub const DEFAULT_HARNESS_VERSION: &'static str = "v0";

    pub fn new(
        session_id: impl Into<SessionId>,
        active_branch: impl Into<String>,
        current_objective: impl Into<String>,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            schema_version: Self::DEFAULT_SCHEMA_VERSION,
            harness_version: Self::DEFAULT_HARNESS_VERSION.to_string(),
            active_branch: active_branch.into(),
            current_objective: current_objective.into(),
            last_known_good_commit: None,
            init_plan: None,
            metadata: HashMap::new(),
        }
    }

    pub fn with_harness_version(mut self, harness_version: impl Into<String>) -> Self {
        self.harness_version = harness_version.into();
        self
    }

    pub fn with_schema_version(mut self, schema_version: u32) -> Self {
        self.schema_version = schema_version;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStatus {
    InProgress,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunCheckpoint {
    pub run_id: String,
    pub started_at: SystemTime,
    pub completed_at: Option<SystemTime>,
    pub status: RunStatus,
    pub note: Option<String>,
}

impl RunCheckpoint {
    pub fn started(run_id: impl Into<String>) -> Self {
        Self {
            run_id: run_id.into(),
            started_at: SystemTime::now(),
            completed_at: None,
            status: RunStatus::InProgress,
            note: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BootstrapState {
    pub manifest: Option<SessionManifest>,
    pub feature_list: Vec<FeatureRecord>,
    pub recent_progress: Vec<ProgressEntry>,
    pub checkpoints: Vec<RunCheckpoint>,
}
