use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fcommon::SessionId;
use fprovider::{Message, Role};
use serde::{Deserialize, Serialize};

use crate::backend::MemoryBackend;
use crate::error::MemoryError;
use crate::types::{
    BootstrapState, FeatureRecord, InitCommand, InitPlan, InitShell, InitShellScript, InitStep,
    ProgressEntry, RunCheckpoint, RunStatus, SessionManifest,
};

#[derive(Debug)]
pub struct FilesystemMemoryBackend {
    root: PathBuf,
    lock: Mutex<()>,
}

impl FilesystemMemoryBackend {
    pub fn new(root: impl AsRef<Path>) -> Result<Self, MemoryError> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(root.join("sessions")).map_err(|error| {
            MemoryError::storage(format!("failed to create filesystem backend root: {error}"))
        })?;
        Ok(Self {
            root,
            lock: Mutex::new(()),
        })
    }

    fn session_path(&self, session_id: &SessionId) -> PathBuf {
        self.root.join("sessions").join(format!(
            "{}.json",
            hex_encode(session_id.as_str().as_bytes())
        ))
    }

    fn load_state(&self, session_id: &SessionId) -> Result<Option<PersistedState>, MemoryError> {
        let path = self.session_path(session_id);
        if !path.exists() {
            return Ok(None);
        }
        let bytes = fs::read(&path).map_err(|error| {
            MemoryError::storage(format!("failed to read session state file: {error}"))
        })?;
        let state = serde_json::from_slice::<PersistedState>(&bytes).map_err(|error| {
            MemoryError::storage(format!("failed to deserialize session state: {error}"))
        })?;
        Ok(Some(state))
    }

    fn save_state(
        &self,
        session_id: &SessionId,
        state: &PersistedState,
    ) -> Result<(), MemoryError> {
        let path = self.session_path(session_id);
        let bytes = serde_json::to_vec_pretty(state).map_err(|error| {
            MemoryError::storage(format!("failed to serialize session state: {error}"))
        })?;

        write_atomic(&path, &bytes)
    }
}

impl MemoryBackend for FilesystemMemoryBackend {
    fn is_initialized<'a>(
        &'a self,
        session_id: &'a SessionId,
    ) -> fcommon::BoxFuture<'a, Result<bool, MemoryError>> {
        Box::pin(async move {
            let _guard = self
                .lock
                .lock()
                .map_err(|_| MemoryError::storage("filesystem backend lock poisoned"))?;
            Ok(self
                .load_state(session_id)?
                .and_then(|state| state.manifest)
                .is_some())
        })
    }

    fn initialize_session_if_missing<'a>(
        &'a self,
        session_id: &'a SessionId,
        manifest: SessionManifest,
        feature_list: Vec<FeatureRecord>,
        initial_progress_entry: Option<ProgressEntry>,
        initial_checkpoint: Option<RunCheckpoint>,
    ) -> fcommon::BoxFuture<'a, Result<bool, MemoryError>> {
        Box::pin(async move {
            let _guard = self
                .lock
                .lock()
                .map_err(|_| MemoryError::storage("filesystem backend lock poisoned"))?;
            let mut state = self.load_state(session_id)?.unwrap_or_default();
            if state.manifest.is_some() {
                return Ok(false);
            }

            state.manifest = Some(PersistedManifest::from_manifest(manifest)?);
            state.feature_list = feature_list
                .into_iter()
                .map(PersistedFeatureRecord::from_feature_record)
                .collect();
            if let Some(entry) = initial_progress_entry {
                state
                    .recent_progress
                    .push(PersistedProgressEntry::from_entry(entry)?);
            }
            if let Some(checkpoint) = initial_checkpoint {
                state
                    .checkpoints
                    .push(PersistedRunCheckpoint::from_checkpoint(checkpoint)?);
            }
            self.save_state(session_id, &state)?;
            Ok(true)
        })
    }

    fn load_bootstrap_state<'a>(
        &'a self,
        session_id: &'a SessionId,
    ) -> fcommon::BoxFuture<'a, Result<BootstrapState, MemoryError>> {
        Box::pin(async move {
            let _guard = self
                .lock
                .lock()
                .map_err(|_| MemoryError::storage("filesystem backend lock poisoned"))?;
            let Some(state) = self.load_state(session_id)? else {
                return Ok(BootstrapState::default());
            };
            Ok(state.into_bootstrap_state(session_id)?)
        })
    }

    fn save_manifest<'a>(
        &'a self,
        session_id: &'a SessionId,
        manifest: SessionManifest,
    ) -> fcommon::BoxFuture<'a, Result<(), MemoryError>> {
        Box::pin(async move {
            let _guard = self
                .lock
                .lock()
                .map_err(|_| MemoryError::storage("filesystem backend lock poisoned"))?;
            let mut state = self.load_state(session_id)?.unwrap_or_default();
            state.manifest = Some(PersistedManifest::from_manifest(manifest)?);
            self.save_state(session_id, &state)
        })
    }

    fn append_progress_entry<'a>(
        &'a self,
        session_id: &'a SessionId,
        entry: ProgressEntry,
    ) -> fcommon::BoxFuture<'a, Result<(), MemoryError>> {
        Box::pin(async move {
            let _guard = self
                .lock
                .lock()
                .map_err(|_| MemoryError::storage("filesystem backend lock poisoned"))?;
            let mut state = self.load_state(session_id)?.unwrap_or_default();
            state
                .recent_progress
                .push(PersistedProgressEntry::from_entry(entry)?);
            self.save_state(session_id, &state)
        })
    }

    fn replace_feature_list<'a>(
        &'a self,
        session_id: &'a SessionId,
        features: Vec<FeatureRecord>,
    ) -> fcommon::BoxFuture<'a, Result<(), MemoryError>> {
        Box::pin(async move {
            let _guard = self
                .lock
                .lock()
                .map_err(|_| MemoryError::storage("filesystem backend lock poisoned"))?;
            let mut state = self.load_state(session_id)?.unwrap_or_default();
            state.feature_list = features
                .into_iter()
                .map(PersistedFeatureRecord::from_feature_record)
                .collect();
            self.save_state(session_id, &state)
        })
    }

    fn update_feature_pass<'a>(
        &'a self,
        session_id: &'a SessionId,
        feature_id: &'a str,
        passes: bool,
    ) -> fcommon::BoxFuture<'a, Result<(), MemoryError>> {
        Box::pin(async move {
            let _guard = self
                .lock
                .lock()
                .map_err(|_| MemoryError::storage("filesystem backend lock poisoned"))?;
            let mut state = self.load_state(session_id)?.unwrap_or_default();
            if let Some(feature) = state.feature_list.iter_mut().find(|f| f.id == feature_id) {
                feature.passes = passes;
                return self.save_state(session_id, &state);
            }
            Err(MemoryError::not_found(format!(
                "feature '{feature_id}' not found"
            )))
        })
    }

    fn record_run_checkpoint<'a>(
        &'a self,
        session_id: &'a SessionId,
        checkpoint: RunCheckpoint,
    ) -> fcommon::BoxFuture<'a, Result<(), MemoryError>> {
        Box::pin(async move {
            let _guard = self
                .lock
                .lock()
                .map_err(|_| MemoryError::storage("filesystem backend lock poisoned"))?;
            let mut state = self.load_state(session_id)?.unwrap_or_default();
            state
                .checkpoints
                .push(PersistedRunCheckpoint::from_checkpoint(checkpoint)?);
            self.save_state(session_id, &state)
        })
    }

    fn load_transcript_messages<'a>(
        &'a self,
        session_id: &'a SessionId,
    ) -> fcommon::BoxFuture<'a, Result<Vec<Message>, MemoryError>> {
        Box::pin(async move {
            let _guard = self
                .lock
                .lock()
                .map_err(|_| MemoryError::storage("filesystem backend lock poisoned"))?;
            let Some(state) = self.load_state(session_id)? else {
                return Ok(Vec::new());
            };
            state
                .transcript
                .into_iter()
                .map(PersistedMessage::into_message)
                .collect()
        })
    }

    fn append_transcript_messages<'a>(
        &'a self,
        session_id: &'a SessionId,
        messages: Vec<Message>,
    ) -> fcommon::BoxFuture<'a, Result<(), MemoryError>> {
        Box::pin(async move {
            let _guard = self
                .lock
                .lock()
                .map_err(|_| MemoryError::storage("filesystem backend lock poisoned"))?;
            let mut state = self.load_state(session_id)?.unwrap_or_default();
            let persisted: Vec<PersistedMessage> = messages
                .into_iter()
                .map(PersistedMessage::from_message)
                .collect();
            state.transcript.extend(persisted);
            self.save_state(session_id, &state)
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PersistedState {
    manifest: Option<PersistedManifest>,
    feature_list: Vec<PersistedFeatureRecord>,
    recent_progress: Vec<PersistedProgressEntry>,
    checkpoints: Vec<PersistedRunCheckpoint>,
    transcript: Vec<PersistedMessage>,
}

impl PersistedState {
    fn into_bootstrap_state(self, session_id: &SessionId) -> Result<BootstrapState, MemoryError> {
        let manifest = self
            .manifest
            .map(|manifest| manifest.into_manifest(session_id))
            .transpose()?;

        let recent_progress = self
            .recent_progress
            .into_iter()
            .map(PersistedProgressEntry::into_entry)
            .collect::<Result<Vec<_>, _>>()?;

        let checkpoints = self
            .checkpoints
            .into_iter()
            .map(PersistedRunCheckpoint::into_checkpoint)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(BootstrapState {
            manifest,
            feature_list: self
                .feature_list
                .into_iter()
                .map(PersistedFeatureRecord::into_feature_record)
                .collect(),
            recent_progress,
            checkpoints,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedFeatureRecord {
    id: String,
    category: String,
    description: String,
    steps: Vec<String>,
    passes: bool,
}

impl PersistedFeatureRecord {
    fn from_feature_record(feature: FeatureRecord) -> Self {
        Self {
            id: feature.id,
            category: feature.category,
            description: feature.description,
            steps: feature.steps,
            passes: feature.passes,
        }
    }

    fn into_feature_record(self) -> FeatureRecord {
        FeatureRecord {
            id: self.id,
            category: self.category,
            description: self.description,
            steps: self.steps,
            passes: self.passes,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedManifest {
    schema_version: u32,
    harness_version: String,
    active_branch: String,
    current_objective: String,
    last_known_good_commit: Option<String>,
    init_plan: Option<PersistedInitPlan>,
    metadata: HashMap<String, String>,
}

impl PersistedManifest {
    fn from_manifest(manifest: SessionManifest) -> Result<Self, MemoryError> {
        Ok(Self {
            schema_version: manifest.schema_version,
            harness_version: manifest.harness_version,
            active_branch: manifest.active_branch,
            current_objective: manifest.current_objective,
            last_known_good_commit: manifest.last_known_good_commit,
            init_plan: manifest
                .init_plan
                .map(PersistedInitPlan::from_init_plan)
                .transpose()?,
            metadata: manifest.metadata,
        })
    }

    fn into_manifest(self, session_id: &SessionId) -> Result<SessionManifest, MemoryError> {
        Ok(SessionManifest {
            session_id: session_id.clone(),
            schema_version: self.schema_version,
            harness_version: self.harness_version,
            active_branch: self.active_branch,
            current_objective: self.current_objective,
            last_known_good_commit: self.last_known_good_commit,
            init_plan: self
                .init_plan
                .map(PersistedInitPlan::into_init_plan)
                .transpose()?,
            metadata: self.metadata,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedInitPlan {
    steps: Vec<PersistedInitStep>,
}

impl PersistedInitPlan {
    fn from_init_plan(plan: InitPlan) -> Result<Self, MemoryError> {
        Ok(Self {
            steps: plan
                .steps
                .into_iter()
                .map(PersistedInitStep::from_init_step)
                .collect::<Result<Vec<_>, _>>()?,
        })
    }

    fn into_init_plan(self) -> Result<InitPlan, MemoryError> {
        Ok(InitPlan {
            steps: self
                .steps
                .into_iter()
                .map(PersistedInitStep::into_init_step)
                .collect::<Result<Vec<_>, _>>()?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum PersistedInitStep {
    Command { program: String, args: Vec<String> },
    Shell { shell: String, script: String },
}

impl PersistedInitStep {
    fn from_init_step(step: InitStep) -> Result<Self, MemoryError> {
        Ok(match step {
            InitStep::Command(InitCommand { program, args }) => Self::Command { program, args },
            InitStep::Shell(InitShellScript { shell, script }) => Self::Shell {
                shell: init_shell_to_string(shell),
                script,
            },
        })
    }

    fn into_init_step(self) -> Result<InitStep, MemoryError> {
        Ok(match self {
            Self::Command { program, args } => InitStep::Command(InitCommand { program, args }),
            Self::Shell { shell, script } => InitStep::Shell(InitShellScript {
                shell: init_shell_from_str(&shell)?,
                script,
            }),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedProgressEntry {
    run_id: String,
    summary: String,
    created_at_secs: i64,
    created_at_nanos: i64,
}

impl PersistedProgressEntry {
    fn from_entry(entry: ProgressEntry) -> Result<Self, MemoryError> {
        let (secs, nanos) = encode_system_time(entry.created_at)?;
        Ok(Self {
            run_id: entry.run_id,
            summary: entry.summary,
            created_at_secs: secs,
            created_at_nanos: nanos,
        })
    }

    fn into_entry(self) -> Result<ProgressEntry, MemoryError> {
        Ok(ProgressEntry {
            run_id: self.run_id,
            summary: self.summary,
            created_at: decode_system_time(self.created_at_secs, self.created_at_nanos)?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedRunCheckpoint {
    run_id: String,
    started_at_secs: i64,
    started_at_nanos: i64,
    completed_at_secs: Option<i64>,
    completed_at_nanos: Option<i64>,
    status: String,
    note: Option<String>,
}

impl PersistedRunCheckpoint {
    fn from_checkpoint(checkpoint: RunCheckpoint) -> Result<Self, MemoryError> {
        let (started_at_secs, started_at_nanos) = encode_system_time(checkpoint.started_at)?;
        let (completed_at_secs, completed_at_nanos) = match checkpoint.completed_at {
            Some(completed_at) => {
                let (secs, nanos) = encode_system_time(completed_at)?;
                (Some(secs), Some(nanos))
            }
            None => (None, None),
        };

        Ok(Self {
            run_id: checkpoint.run_id,
            started_at_secs,
            started_at_nanos,
            completed_at_secs,
            completed_at_nanos,
            status: run_status_to_string(checkpoint.status),
            note: checkpoint.note,
        })
    }

    fn into_checkpoint(self) -> Result<RunCheckpoint, MemoryError> {
        let completed_at = match (self.completed_at_secs, self.completed_at_nanos) {
            (Some(secs), Some(nanos)) => Some(decode_system_time(secs, nanos)?),
            (None, None) => None,
            _ => {
                return Err(MemoryError::storage(
                    "checkpoint completed timestamp must include both seconds and nanos",
                ));
            }
        };

        Ok(RunCheckpoint {
            run_id: self.run_id,
            started_at: decode_system_time(self.started_at_secs, self.started_at_nanos)?,
            completed_at,
            status: run_status_from_str(&self.status)?,
            note: self.note,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedMessage {
    role: String,
    content: String,
}

impl PersistedMessage {
    fn from_message(message: Message) -> Self {
        Self {
            role: role_to_string(message.role),
            content: message.content,
        }
    }

    fn into_message(self) -> Result<Message, MemoryError> {
        Ok(Message {
            role: role_from_str(&self.role)?,
            content: self.content,
        })
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), MemoryError> {
    let Some(parent) = path.parent() else {
        return Err(MemoryError::storage(
            "session state file missing parent directory",
        ));
    };
    fs::create_dir_all(parent).map_err(|error| {
        MemoryError::storage(format!("failed to create parent directory: {error}"))
    })?;

    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, bytes).map_err(|error| {
        MemoryError::storage(format!("failed to write temporary state file: {error}"))
    })?;

    if path.exists() {
        fs::remove_file(path).map_err(|error| {
            MemoryError::storage(format!("failed to replace existing state file: {error}"))
        })?;
    }
    fs::rename(&tmp, path)
        .map_err(|error| MemoryError::storage(format!("failed to finalize state file: {error}")))
}

fn encode_system_time(value: SystemTime) -> Result<(i64, i64), MemoryError> {
    let duration = value.duration_since(UNIX_EPOCH).map_err(|error| {
        MemoryError::invalid_request(format!("timestamp predates unix epoch: {error}"))
    })?;
    Ok((
        duration.as_secs() as i64,
        i64::from(duration.subsec_nanos()),
    ))
}

fn decode_system_time(seconds: i64, nanos: i64) -> Result<SystemTime, MemoryError> {
    if seconds < 0 {
        return Err(MemoryError::storage(format!(
            "timestamp seconds must be non-negative, got {seconds}"
        )));
    }
    if !(0..1_000_000_000).contains(&nanos) {
        return Err(MemoryError::storage(format!(
            "timestamp nanos must be in [0, 1_000_000_000), got {nanos}"
        )));
    }
    Ok(UNIX_EPOCH + Duration::new(seconds as u64, nanos as u32))
}

fn run_status_to_string(status: RunStatus) -> String {
    match status {
        RunStatus::InProgress => "in_progress".to_string(),
        RunStatus::Succeeded => "succeeded".to_string(),
        RunStatus::Failed => "failed".to_string(),
    }
}

fn run_status_from_str(value: &str) -> Result<RunStatus, MemoryError> {
    match value {
        "in_progress" => Ok(RunStatus::InProgress),
        "succeeded" => Ok(RunStatus::Succeeded),
        "failed" => Ok(RunStatus::Failed),
        _ => Err(MemoryError::storage(format!(
            "unknown run status value '{value}'"
        ))),
    }
}

fn init_shell_to_string(shell: InitShell) -> String {
    match shell {
        InitShell::Bash => "bash".to_string(),
        InitShell::Sh => "sh".to_string(),
        InitShell::Pwsh => "pwsh".to_string(),
        InitShell::Cmd => "cmd".to_string(),
    }
}

fn init_shell_from_str(value: &str) -> Result<InitShell, MemoryError> {
    match value {
        "bash" => Ok(InitShell::Bash),
        "sh" => Ok(InitShell::Sh),
        "pwsh" => Ok(InitShell::Pwsh),
        "cmd" => Ok(InitShell::Cmd),
        _ => Err(MemoryError::storage(format!(
            "unknown init shell value '{value}'"
        ))),
    }
}

fn role_to_string(role: Role) -> String {
    match role {
        Role::System => "system".to_string(),
        Role::User => "user".to_string(),
        Role::Assistant => "assistant".to_string(),
        Role::Tool => "tool".to_string(),
    }
}

fn role_from_str(value: &str) -> Result<Role, MemoryError> {
    match value {
        "system" => Ok(Role::System),
        "user" => Ok(Role::User),
        "assistant" => Ok(Role::Assistant),
        "tool" => Ok(Role::Tool),
        _ => Err(MemoryError::storage(format!(
            "unknown transcript role value '{value}'"
        ))),
    }
}

fn hex_encode(input: &[u8]) -> String {
    let mut output = String::with_capacity(input.len() * 2);
    for byte in input {
        output.push(nibble_to_hex(byte >> 4));
        output.push(nibble_to_hex(byte & 0x0f));
    }
    output
}

fn nibble_to_hex(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        10..=15 => (b'a' + (nibble - 10)) as char,
        _ => '0',
    }
}
