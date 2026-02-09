//! Context and harness-state persistence layer with fchat adapter support.

mod adapter;
mod backend;
mod backends;
mod error;
mod types;

pub mod prelude {
    pub use crate::{
        BootstrapState, FeatureRecord, FilesystemMemoryBackend, InMemoryMemoryBackend, InitCommand,
        InitPlan, InitShell, InitShellScript, InitStep, MemoryBackend, MemoryBackendConfig,
        MemoryConversationStore, MemoryError, MemoryErrorKind, ProgressEntry, RunCheckpoint,
        RunStatus, SessionManifest, SqliteMemoryBackend, create_default_memory_backend,
        create_memory_backend,
    };
}

pub use adapter::MemoryConversationStore;
pub use backend::{
    FilesystemMemoryBackend, InMemoryMemoryBackend, MemoryBackend, MemoryBackendConfig,
    SqliteMemoryBackend, create_default_memory_backend, create_memory_backend,
};
pub use error::{MemoryError, MemoryErrorKind};
pub use types::{
    BootstrapState, FeatureRecord, InitCommand, InitPlan, InitShell, InitShellScript, InitStep,
    ProgressEntry, RunCheckpoint, RunStatus, SessionManifest,
};

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use fchat::ConversationStore;
    use fcommon::SessionId;
    use fprovider::{Message, Role};

    use crate::types::{FeatureRecord, ProgressEntry, RunCheckpoint, SessionManifest};
    use crate::{
        FilesystemMemoryBackend, InMemoryMemoryBackend, MemoryBackend, MemoryConversationStore,
        SqliteMemoryBackend,
    };

    fn temp_dir(prefix: &str) -> std::path::PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("fmemory-{prefix}-{unique}"))
    }

    #[tokio::test]
    async fn backend_stores_bootstrap_state_and_transcript() {
        let backend = InMemoryMemoryBackend::new();
        let session_id = SessionId::from("session-a");

        backend
            .save_manifest(
                &session_id,
                SessionManifest::new(session_id.clone(), "feature/harness", "Build initializer"),
            )
            .await
            .expect("manifest should save");

        backend
            .replace_feature_list(
                &session_id,
                vec![FeatureRecord {
                    id: "f-1".to_string(),
                    category: "functional".to_string(),
                    description: "Initializer creates artifacts".to_string(),
                    steps: vec!["run initializer".to_string()],
                    passes: false,
                }],
            )
            .await
            .expect("feature list should save");

        backend
            .append_progress_entry(&session_id, ProgressEntry::new("run-1", "Initialized"))
            .await
            .expect("progress should append");

        backend
            .record_run_checkpoint(&session_id, RunCheckpoint::started("run-1"))
            .await
            .expect("checkpoint should save");

        backend
            .append_transcript_messages(
                &session_id,
                vec![
                    Message::new(Role::User, "hello"),
                    Message::new(Role::Assistant, "hi"),
                ],
            )
            .await
            .expect("transcript should append");

        let bootstrap = backend
            .load_bootstrap_state(&session_id)
            .await
            .expect("bootstrap should load");
        assert!(bootstrap.manifest.is_some());
        assert_eq!(bootstrap.feature_list.len(), 1);
        assert_eq!(bootstrap.recent_progress.len(), 1);
        assert_eq!(bootstrap.checkpoints.len(), 1);

        let transcript = backend
            .load_transcript_messages(&session_id)
            .await
            .expect("transcript should load");
        assert_eq!(transcript.len(), 2);
    }

    #[tokio::test]
    async fn conversation_store_adapter_reads_and_writes_transcript() {
        let backend: Arc<dyn MemoryBackend> = Arc::new(InMemoryMemoryBackend::new());
        let store = MemoryConversationStore::new(backend.clone());
        let session_id = SessionId::from("session-b");

        store
            .append_messages(
                &session_id,
                vec![
                    Message::new(Role::User, "hello"),
                    Message::new(Role::Assistant, "greetings"),
                ],
            )
            .await
            .expect("append should work");

        let loaded = store
            .load_messages(&session_id)
            .await
            .expect("load should work");
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].role, Role::User);
        assert_eq!(loaded[1].role, Role::Assistant);
    }

    #[tokio::test]
    async fn update_feature_pass_fails_for_unknown_feature() {
        let backend = InMemoryMemoryBackend::new();
        let session_id = SessionId::from("session-c");
        let error = backend
            .update_feature_pass(&session_id, "missing", true)
            .await
            .expect_err("update should fail");

        assert_eq!(error.kind, crate::MemoryErrorKind::NotFound);
    }

    #[tokio::test]
    async fn initialize_session_if_missing_is_idempotent() {
        let backend = InMemoryMemoryBackend::new();
        let session_id = SessionId::from("session-init");

        let created = backend
            .initialize_session_if_missing(
                &session_id,
                SessionManifest::new(session_id.clone(), "feature/init", "Initialize harness")
                    .with_harness_version("v0.1.0"),
                vec![FeatureRecord {
                    id: "f-1".to_string(),
                    category: "functional".to_string(),
                    description: "create init artifacts".to_string(),
                    steps: vec!["write files".to_string()],
                    passes: false,
                }],
                Some(ProgressEntry::new("run-1", "initialized")),
                Some(RunCheckpoint::started("run-1")),
            )
            .await
            .expect("init should succeed");
        assert!(created);

        let created_again = backend
            .initialize_session_if_missing(
                &session_id,
                SessionManifest::new(session_id.clone(), "feature/other", "Should not overwrite"),
                vec![FeatureRecord {
                    id: "f-overwrite".to_string(),
                    category: "functional".to_string(),
                    description: "should not appear".to_string(),
                    steps: vec!["none".to_string()],
                    passes: true,
                }],
                Some(ProgressEntry::new("run-2", "should not append")),
                Some(RunCheckpoint::started("run-2")),
            )
            .await
            .expect("second init should return false");
        assert!(!created_again);

        let bootstrap = backend
            .load_bootstrap_state(&session_id)
            .await
            .expect("bootstrap should load");
        let manifest = bootstrap.manifest.expect("manifest should exist");
        assert_eq!(manifest.active_branch, "feature/init");
        assert_eq!(manifest.harness_version, "v0.1.0");
        assert_eq!(bootstrap.feature_list.len(), 1);
        assert_eq!(bootstrap.recent_progress.len(), 1);
        assert_eq!(bootstrap.checkpoints.len(), 1);
    }

    #[tokio::test]
    async fn is_initialized_tracks_manifest_presence() {
        let backend = InMemoryMemoryBackend::new();
        let session_id = SessionId::from("session-ready");

        assert!(
            !backend
                .is_initialized(&session_id)
                .await
                .expect("lookup should work")
        );

        backend
            .save_manifest(
                &session_id,
                SessionManifest::new(session_id.clone(), "feature/init", "Initialize"),
            )
            .await
            .expect("manifest should save");

        assert!(
            backend
                .is_initialized(&session_id)
                .await
                .expect("lookup should work")
        );
    }

    #[tokio::test]
    async fn sqlite_backend_stores_bootstrap_state_and_transcript() {
        let backend =
            SqliteMemoryBackend::new_in_memory().expect("sqlite backend should initialize");
        let session_id = SessionId::from("session-sqlite");

        backend
            .save_manifest(
                &session_id,
                SessionManifest::new(session_id.clone(), "feature/sqlite", "Build sqlite backend"),
            )
            .await
            .expect("manifest should save");

        backend
            .replace_feature_list(
                &session_id,
                vec![FeatureRecord {
                    id: "f-sqlite-1".to_string(),
                    category: "functional".to_string(),
                    description: "SQLite backend persists feature rows".to_string(),
                    steps: vec!["write feature rows".to_string()],
                    passes: false,
                }],
            )
            .await
            .expect("feature list should save");

        backend
            .append_progress_entry(
                &session_id,
                ProgressEntry::new("run-sqlite-1", "SQLite bootstrap created"),
            )
            .await
            .expect("progress should append");

        backend
            .record_run_checkpoint(&session_id, RunCheckpoint::started("run-sqlite-1"))
            .await
            .expect("checkpoint should save");

        backend
            .append_transcript_messages(
                &session_id,
                vec![
                    Message::new(Role::User, "sqlite hello"),
                    Message::new(Role::Assistant, "sqlite hi"),
                ],
            )
            .await
            .expect("transcript should append");

        let bootstrap = backend
            .load_bootstrap_state(&session_id)
            .await
            .expect("bootstrap should load");
        assert!(bootstrap.manifest.is_some());
        assert_eq!(bootstrap.feature_list.len(), 1);
        assert_eq!(bootstrap.recent_progress.len(), 1);
        assert_eq!(bootstrap.checkpoints.len(), 1);

        let transcript = backend
            .load_transcript_messages(&session_id)
            .await
            .expect("transcript should load");
        assert_eq!(transcript.len(), 2);
        assert_eq!(transcript[0].role, Role::User);
        assert_eq!(transcript[1].role, Role::Assistant);
    }

    #[tokio::test]
    async fn filesystem_backend_stores_bootstrap_state_and_transcript() {
        let root = temp_dir("filesystem");
        let backend = FilesystemMemoryBackend::new(&root).expect("fs backend should initialize");
        let session_id = SessionId::from("session-filesystem");

        backend
            .save_manifest(
                &session_id,
                SessionManifest::new(session_id.clone(), "feature/fs", "Build filesystem backend"),
            )
            .await
            .expect("manifest should save");

        backend
            .replace_feature_list(
                &session_id,
                vec![FeatureRecord {
                    id: "f-fs-1".to_string(),
                    category: "functional".to_string(),
                    description: "Filesystem backend persists feature rows".to_string(),
                    steps: vec!["write feature rows".to_string()],
                    passes: false,
                }],
            )
            .await
            .expect("feature list should save");

        backend
            .append_progress_entry(
                &session_id,
                ProgressEntry::new("run-fs-1", "Filesystem bootstrap created"),
            )
            .await
            .expect("progress should append");

        backend
            .record_run_checkpoint(&session_id, RunCheckpoint::started("run-fs-1"))
            .await
            .expect("checkpoint should save");

        backend
            .append_transcript_messages(
                &session_id,
                vec![
                    Message::new(Role::User, "filesystem hello"),
                    Message::new(Role::Assistant, "filesystem hi"),
                ],
            )
            .await
            .expect("transcript should append");

        let bootstrap = backend
            .load_bootstrap_state(&session_id)
            .await
            .expect("bootstrap should load");
        assert!(bootstrap.manifest.is_some());
        assert_eq!(bootstrap.feature_list.len(), 1);
        assert_eq!(bootstrap.recent_progress.len(), 1);
        assert_eq!(bootstrap.checkpoints.len(), 1);

        let transcript = backend
            .load_transcript_messages(&session_id)
            .await
            .expect("transcript should load");
        assert_eq!(transcript.len(), 2);
        assert_eq!(transcript[0].role, Role::User);
        assert_eq!(transcript[1].role, Role::Assistant);

        std::fs::remove_dir_all(&root).expect("temporary directory should be removable");
    }
}
