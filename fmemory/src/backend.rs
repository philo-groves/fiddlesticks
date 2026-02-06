//! Memory backend trait and in-memory implementation for harness phase state.

use std::collections::HashMap;
use std::sync::Mutex;

use fcommon::{BoxFuture, SessionId};
use fprovider::Message;

use crate::error::MemoryError;
use crate::types::{BootstrapState, FeatureRecord, ProgressEntry, RunCheckpoint, SessionManifest};

pub trait MemoryBackend: Send + Sync {
    fn is_initialized<'a>(
        &'a self,
        session_id: &'a SessionId,
    ) -> BoxFuture<'a, Result<bool, MemoryError>>;

    fn initialize_session_if_missing<'a>(
        &'a self,
        session_id: &'a SessionId,
        manifest: SessionManifest,
        feature_list: Vec<FeatureRecord>,
        initial_progress_entry: Option<ProgressEntry>,
        initial_checkpoint: Option<RunCheckpoint>,
    ) -> BoxFuture<'a, Result<bool, MemoryError>>;

    fn load_bootstrap_state<'a>(
        &'a self,
        session_id: &'a SessionId,
    ) -> BoxFuture<'a, Result<BootstrapState, MemoryError>>;

    fn save_manifest<'a>(
        &'a self,
        session_id: &'a SessionId,
        manifest: SessionManifest,
    ) -> BoxFuture<'a, Result<(), MemoryError>>;

    fn append_progress_entry<'a>(
        &'a self,
        session_id: &'a SessionId,
        entry: ProgressEntry,
    ) -> BoxFuture<'a, Result<(), MemoryError>>;

    fn replace_feature_list<'a>(
        &'a self,
        session_id: &'a SessionId,
        features: Vec<FeatureRecord>,
    ) -> BoxFuture<'a, Result<(), MemoryError>>;

    fn update_feature_pass<'a>(
        &'a self,
        session_id: &'a SessionId,
        feature_id: &'a str,
        passes: bool,
    ) -> BoxFuture<'a, Result<(), MemoryError>>;

    fn record_run_checkpoint<'a>(
        &'a self,
        session_id: &'a SessionId,
        checkpoint: RunCheckpoint,
    ) -> BoxFuture<'a, Result<(), MemoryError>>;

    fn load_transcript_messages<'a>(
        &'a self,
        session_id: &'a SessionId,
    ) -> BoxFuture<'a, Result<Vec<Message>, MemoryError>>;

    fn append_transcript_messages<'a>(
        &'a self,
        session_id: &'a SessionId,
        messages: Vec<Message>,
    ) -> BoxFuture<'a, Result<(), MemoryError>>;
}

#[derive(Debug, Default)]
pub struct InMemoryMemoryBackend {
    sessions: Mutex<HashMap<SessionId, SessionState>>,
}

#[derive(Debug, Default, Clone)]
struct SessionState {
    manifest: Option<SessionManifest>,
    feature_list: Vec<FeatureRecord>,
    progress: Vec<ProgressEntry>,
    checkpoints: Vec<RunCheckpoint>,
    transcript: Vec<Message>,
}

impl InMemoryMemoryBackend {
    pub fn new() -> Self {
        Self::default()
    }
}

impl MemoryBackend for InMemoryMemoryBackend {
    fn is_initialized<'a>(
        &'a self,
        session_id: &'a SessionId,
    ) -> BoxFuture<'a, Result<bool, MemoryError>> {
        Box::pin(async move {
            let sessions = self
                .sessions
                .lock()
                .map_err(|_| MemoryError::storage("memory backend lock poisoned"))?;

            Ok(sessions
                .get(session_id)
                .and_then(|state| state.manifest.as_ref())
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
    ) -> BoxFuture<'a, Result<bool, MemoryError>> {
        Box::pin(async move {
            let mut sessions = self
                .sessions
                .lock()
                .map_err(|_| MemoryError::storage("memory backend lock poisoned"))?;

            let state = sessions.entry(session_id.clone()).or_default();
            if state.manifest.is_some() {
                return Ok(false);
            }

            state.manifest = Some(manifest);
            state.feature_list = feature_list;

            if let Some(progress_entry) = initial_progress_entry {
                state.progress.push(progress_entry);
            }

            if let Some(checkpoint) = initial_checkpoint {
                state.checkpoints.push(checkpoint);
            }

            Ok(true)
        })
    }

    fn load_bootstrap_state<'a>(
        &'a self,
        session_id: &'a SessionId,
    ) -> BoxFuture<'a, Result<BootstrapState, MemoryError>> {
        Box::pin(async move {
            let sessions = self
                .sessions
                .lock()
                .map_err(|_| MemoryError::storage("memory backend lock poisoned"))?;

            if let Some(state) = sessions.get(session_id) {
                return Ok(BootstrapState {
                    manifest: state.manifest.clone(),
                    feature_list: state.feature_list.clone(),
                    recent_progress: state.progress.clone(),
                    checkpoints: state.checkpoints.clone(),
                });
            }

            Ok(BootstrapState::default())
        })
    }

    fn save_manifest<'a>(
        &'a self,
        session_id: &'a SessionId,
        manifest: SessionManifest,
    ) -> BoxFuture<'a, Result<(), MemoryError>> {
        Box::pin(async move {
            let mut sessions = self
                .sessions
                .lock()
                .map_err(|_| MemoryError::storage("memory backend lock poisoned"))?;
            sessions.entry(session_id.clone()).or_default().manifest = Some(manifest);
            Ok(())
        })
    }

    fn append_progress_entry<'a>(
        &'a self,
        session_id: &'a SessionId,
        entry: ProgressEntry,
    ) -> BoxFuture<'a, Result<(), MemoryError>> {
        Box::pin(async move {
            let mut sessions = self
                .sessions
                .lock()
                .map_err(|_| MemoryError::storage("memory backend lock poisoned"))?;
            sessions
                .entry(session_id.clone())
                .or_default()
                .progress
                .push(entry);
            Ok(())
        })
    }

    fn replace_feature_list<'a>(
        &'a self,
        session_id: &'a SessionId,
        features: Vec<FeatureRecord>,
    ) -> BoxFuture<'a, Result<(), MemoryError>> {
        Box::pin(async move {
            let mut sessions = self
                .sessions
                .lock()
                .map_err(|_| MemoryError::storage("memory backend lock poisoned"))?;
            sessions.entry(session_id.clone()).or_default().feature_list = features;
            Ok(())
        })
    }

    fn update_feature_pass<'a>(
        &'a self,
        session_id: &'a SessionId,
        feature_id: &'a str,
        passes: bool,
    ) -> BoxFuture<'a, Result<(), MemoryError>> {
        Box::pin(async move {
            let mut sessions = self
                .sessions
                .lock()
                .map_err(|_| MemoryError::storage("memory backend lock poisoned"))?;
            let state = sessions.entry(session_id.clone()).or_default();

            if let Some(feature) = state.feature_list.iter_mut().find(|f| f.id == feature_id) {
                feature.passes = passes;
                return Ok(());
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
    ) -> BoxFuture<'a, Result<(), MemoryError>> {
        Box::pin(async move {
            let mut sessions = self
                .sessions
                .lock()
                .map_err(|_| MemoryError::storage("memory backend lock poisoned"))?;
            sessions
                .entry(session_id.clone())
                .or_default()
                .checkpoints
                .push(checkpoint);
            Ok(())
        })
    }

    fn load_transcript_messages<'a>(
        &'a self,
        session_id: &'a SessionId,
    ) -> BoxFuture<'a, Result<Vec<Message>, MemoryError>> {
        Box::pin(async move {
            let sessions = self
                .sessions
                .lock()
                .map_err(|_| MemoryError::storage("memory backend lock poisoned"))?;

            Ok(sessions
                .get(session_id)
                .map(|state| state.transcript.clone())
                .unwrap_or_default())
        })
    }

    fn append_transcript_messages<'a>(
        &'a self,
        session_id: &'a SessionId,
        messages: Vec<Message>,
    ) -> BoxFuture<'a, Result<(), MemoryError>> {
        Box::pin(async move {
            let mut sessions = self
                .sessions
                .lock()
                .map_err(|_| MemoryError::storage("memory backend lock poisoned"))?;

            sessions
                .entry(session_id.clone())
                .or_default()
                .transcript
                .extend(messages);

            Ok(())
        })
    }
}
