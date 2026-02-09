use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fcommon::{BoxFuture, SessionId};
use fprovider::{Message, Role};
use rusqlite::{Connection, OptionalExtension, params};

use crate::backend::MemoryBackend;
use crate::error::MemoryError;
use crate::types::{
    BootstrapState, FeatureRecord, InitCommand, InitPlan, InitShell, InitShellScript, InitStep,
    ProgressEntry, RunCheckpoint, RunStatus, SessionManifest,
};

#[derive(Debug)]
pub struct SqliteMemoryBackend {
    connection: Mutex<Connection>,
}

impl SqliteMemoryBackend {
    pub fn new(path: impl AsRef<Path>) -> Result<Self, MemoryError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|error| {
                MemoryError::storage(format!(
                    "failed to create sqlite parent directory: {error}"
                ))
            })?;
        }

        let connection = Connection::open(path).map_err(|error| {
            MemoryError::storage(format!("failed to open sqlite database: {error}"))
        })?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(|error| {
                MemoryError::storage(format!("failed to configure sqlite busy timeout: {error}"))
            })?;
        let backend = Self {
            connection: Mutex::new(connection),
        };
        backend.initialize_schema()?;
        Ok(backend)
    }

    pub fn new_in_memory() -> Result<Self, MemoryError> {
        let connection = Connection::open_in_memory().map_err(|error| {
            MemoryError::storage(format!("failed to open in-memory sqlite database: {error}"))
        })?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(|error| {
                MemoryError::storage(format!("failed to configure sqlite busy timeout: {error}"))
            })?;
        let backend = Self {
            connection: Mutex::new(connection),
        };
        backend.initialize_schema()?;
        Ok(backend)
    }

    fn connection(&self) -> Result<std::sync::MutexGuard<'_, Connection>, MemoryError> {
        self.connection
            .lock()
            .map_err(|_| MemoryError::storage("sqlite backend lock poisoned"))
    }

    fn initialize_schema(&self) -> Result<(), MemoryError> {
        let conn = self.connection()?;
        conn.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;

            CREATE TABLE IF NOT EXISTS sessions (
                session_id TEXT PRIMARY KEY,
                schema_version INTEGER NOT NULL,
                harness_version TEXT NOT NULL,
                active_branch TEXT NOT NULL,
                current_objective TEXT NOT NULL,
                last_known_good_commit TEXT
            );

            CREATE TABLE IF NOT EXISTS session_metadata (
                session_id TEXT NOT NULL,
                key TEXT NOT NULL,
                value TEXT NOT NULL,
                PRIMARY KEY (session_id, key)
            );

            CREATE TABLE IF NOT EXISTS init_plan_steps (
                session_id TEXT NOT NULL,
                step_index INTEGER NOT NULL,
                step_kind TEXT NOT NULL,
                command_program TEXT,
                command_args_json TEXT,
                shell_name TEXT,
                shell_script TEXT,
                PRIMARY KEY (session_id, step_index)
            );

            CREATE TABLE IF NOT EXISTS features (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                position INTEGER NOT NULL,
                feature_id TEXT NOT NULL,
                category TEXT NOT NULL,
                description TEXT NOT NULL,
                steps_json TEXT NOT NULL,
                passes INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_features_session_position
            ON features(session_id, position);

            CREATE TABLE IF NOT EXISTS progress_entries (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                run_id TEXT NOT NULL,
                summary TEXT NOT NULL,
                created_at_secs INTEGER NOT NULL,
                created_at_nanos INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_progress_session_id
            ON progress_entries(session_id, id);

            CREATE TABLE IF NOT EXISTS run_checkpoints (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                run_id TEXT NOT NULL,
                started_at_secs INTEGER NOT NULL,
                started_at_nanos INTEGER NOT NULL,
                completed_at_secs INTEGER,
                completed_at_nanos INTEGER,
                status TEXT NOT NULL,
                note TEXT
            );

            CREATE INDEX IF NOT EXISTS idx_checkpoints_session_id
            ON run_checkpoints(session_id, id);

            CREATE TABLE IF NOT EXISTS transcript_messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_transcript_session_id
            ON transcript_messages(session_id, id);
            ",
        )
        .map_err(|error| {
            MemoryError::storage(format!("failed to initialize sqlite schema: {error}"))
        })?;

        Ok(())
    }

    fn save_manifest_rows(
        conn: &Connection,
        session_id: &SessionId,
        manifest: &SessionManifest,
    ) -> Result<(), MemoryError> {
        conn.execute(
            "
            INSERT INTO sessions (
                session_id,
                schema_version,
                harness_version,
                active_branch,
                current_objective,
                last_known_good_commit
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(session_id) DO UPDATE SET
                schema_version = excluded.schema_version,
                harness_version = excluded.harness_version,
                active_branch = excluded.active_branch,
                current_objective = excluded.current_objective,
                last_known_good_commit = excluded.last_known_good_commit
            ",
            params![
                session_id.as_str(),
                i64::from(manifest.schema_version),
                &manifest.harness_version,
                &manifest.active_branch,
                &manifest.current_objective,
                manifest.last_known_good_commit.as_deref(),
            ],
        )
        .map_err(|error| {
            MemoryError::storage(format!("failed to upsert session manifest: {error}"))
        })?;

        conn.execute(
            "DELETE FROM session_metadata WHERE session_id = ?1",
            params![session_id.as_str()],
        )
        .map_err(|error| {
            MemoryError::storage(format!("failed to clear session metadata: {error}"))
        })?;

        for (key, value) in &manifest.metadata {
            conn.execute(
                "
                INSERT INTO session_metadata (session_id, key, value)
                VALUES (?1, ?2, ?3)
                ",
                params![session_id.as_str(), key, value],
            )
            .map_err(|error| {
                MemoryError::storage(format!("failed to write session metadata row: {error}"))
            })?;
        }

        conn.execute(
            "DELETE FROM init_plan_steps WHERE session_id = ?1",
            params![session_id.as_str()],
        )
        .map_err(|error| {
            MemoryError::storage(format!("failed to clear init plan steps: {error}"))
        })?;

        if let Some(plan) = &manifest.init_plan {
            for (index, step) in plan.steps.iter().enumerate() {
                let (step_kind, command_program, command_args_json, shell_name, shell_script) =
                    match step {
                        InitStep::Command(InitCommand { program, args }) => (
                            "command",
                            Some(program.as_str()),
                            Some(serde_json::to_string(args).map_err(|error| {
                                MemoryError::storage(format!(
                                    "failed to serialize init command arguments: {error}"
                                ))
                            })?),
                            None,
                            None,
                        ),
                        InitStep::Shell(InitShellScript { shell, script }) => (
                            "shell",
                            None,
                            None,
                            Some(init_shell_to_str(*shell)),
                            Some(script.as_str()),
                        ),
                    };

                conn.execute(
                    "
                    INSERT INTO init_plan_steps (
                        session_id,
                        step_index,
                        step_kind,
                        command_program,
                        command_args_json,
                        shell_name,
                        shell_script
                    )
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                    ",
                    params![
                        session_id.as_str(),
                        index as i64,
                        step_kind,
                        command_program,
                        command_args_json,
                        shell_name,
                        shell_script,
                    ],
                )
                .map_err(|error| {
                    MemoryError::storage(format!("failed to write init plan step: {error}"))
                })?;
            }
        }

        Ok(())
    }

    fn replace_features(
        conn: &Connection,
        session_id: &SessionId,
        features: &[FeatureRecord],
    ) -> Result<(), MemoryError> {
        conn.execute(
            "DELETE FROM features WHERE session_id = ?1",
            params![session_id.as_str()],
        )
        .map_err(|error| MemoryError::storage(format!("failed to clear feature rows: {error}")))?;

        for (position, feature) in features.iter().enumerate() {
            let steps_json = serde_json::to_string(&feature.steps).map_err(|error| {
                MemoryError::storage(format!("failed to serialize feature steps: {error}"))
            })?;
            conn.execute(
                "
                INSERT INTO features (
                    session_id,
                    position,
                    feature_id,
                    category,
                    description,
                    steps_json,
                    passes
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                ",
                params![
                    session_id.as_str(),
                    position as i64,
                    &feature.id,
                    &feature.category,
                    &feature.description,
                    steps_json,
                    if feature.passes { 1_i64 } else { 0_i64 },
                ],
            )
            .map_err(|error| {
                MemoryError::storage(format!("failed to write feature row: {error}"))
            })?;
        }

        Ok(())
    }
}

impl MemoryBackend for SqliteMemoryBackend {
    fn is_initialized<'a>(
        &'a self,
        session_id: &'a SessionId,
    ) -> BoxFuture<'a, Result<bool, MemoryError>> {
        Box::pin(async move {
            let conn = self.connection()?;
            let exists = conn
                .query_row(
                    "SELECT 1 FROM sessions WHERE session_id = ?1 LIMIT 1",
                    params![session_id.as_str()],
                    |_| Ok(true),
                )
                .optional()
                .map_err(|error| {
                    MemoryError::storage(format!("failed to check session initialization: {error}"))
                })?
                .unwrap_or(false);
            Ok(exists)
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
            let conn = self.connection()?;
            let inserted = conn
                .execute(
                    "
                    INSERT OR IGNORE INTO sessions (
                        session_id,
                        schema_version,
                        harness_version,
                        active_branch,
                        current_objective,
                        last_known_good_commit
                    )
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                    ",
                    params![
                        session_id.as_str(),
                        i64::from(manifest.schema_version),
                        &manifest.harness_version,
                        &manifest.active_branch,
                        &manifest.current_objective,
                        manifest.last_known_good_commit.as_deref(),
                    ],
                )
                .map_err(|error| {
                    MemoryError::storage(format!("failed to initialize session manifest: {error}"))
                })?;

            if inserted == 0 {
                return Ok(false);
            }

            Self::save_manifest_rows(&conn, session_id, &manifest)?;
            Self::replace_features(&conn, session_id, &feature_list)?;

            if let Some(entry) = initial_progress_entry {
                let (secs, nanos) = encode_system_time(entry.created_at)?;
                conn.execute(
                    "
                    INSERT INTO progress_entries (
                        session_id,
                        run_id,
                        summary,
                        created_at_secs,
                        created_at_nanos
                    )
                    VALUES (?1, ?2, ?3, ?4, ?5)
                    ",
                    params![
                        session_id.as_str(),
                        entry.run_id,
                        entry.summary,
                        secs,
                        nanos
                    ],
                )
                .map_err(|error| {
                    MemoryError::storage(format!("failed to write initial progress entry: {error}"))
                })?;
            }

            if let Some(checkpoint) = initial_checkpoint {
                let (started_secs, started_nanos) = encode_system_time(checkpoint.started_at)?;
                let (completed_secs, completed_nanos) = match checkpoint.completed_at {
                    Some(completed_at) => {
                        let (secs, nanos) = encode_system_time(completed_at)?;
                        (Some(secs), Some(nanos))
                    }
                    None => (None, None),
                };
                conn.execute(
                    "
                    INSERT INTO run_checkpoints (
                        session_id,
                        run_id,
                        started_at_secs,
                        started_at_nanos,
                        completed_at_secs,
                        completed_at_nanos,
                        status,
                        note
                    )
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                    ",
                    params![
                        session_id.as_str(),
                        checkpoint.run_id,
                        started_secs,
                        started_nanos,
                        completed_secs,
                        completed_nanos,
                        run_status_to_str(checkpoint.status),
                        checkpoint.note,
                    ],
                )
                .map_err(|error| {
                    MemoryError::storage(format!("failed to write initial checkpoint: {error}"))
                })?;
            }

            Ok(true)
        })
    }

    fn load_bootstrap_state<'a>(
        &'a self,
        session_id: &'a SessionId,
    ) -> BoxFuture<'a, Result<BootstrapState, MemoryError>> {
        Box::pin(async move {
            let conn = self.connection()?;

            let mut manifest = conn
                .query_row(
                    "
                    SELECT
                        schema_version,
                        harness_version,
                        active_branch,
                        current_objective,
                        last_known_good_commit
                    FROM sessions
                    WHERE session_id = ?1
                    ",
                    params![session_id.as_str()],
                    |row| {
                        let schema_version = row.get::<_, i64>(0)?;
                        let harness_version = row.get::<_, String>(1)?;
                        let active_branch = row.get::<_, String>(2)?;
                        let current_objective = row.get::<_, String>(3)?;
                        let last_known_good_commit = row.get::<_, Option<String>>(4)?;
                        Ok((
                            schema_version,
                            harness_version,
                            active_branch,
                            current_objective,
                            last_known_good_commit,
                        ))
                    },
                )
                .optional()
                .map_err(|error| {
                    MemoryError::storage(format!("failed to load session manifest row: {error}"))
                })?
                .map(
                    |(
                        schema_version,
                        harness_version,
                        active_branch,
                        current_objective,
                        last_known_good_commit,
                    )| SessionManifest {
                        session_id: session_id.clone(),
                        schema_version: schema_version as u32,
                        harness_version,
                        active_branch,
                        current_objective,
                        last_known_good_commit,
                        init_plan: None,
                        metadata: HashMap::new(),
                    },
                );

            if let Some(manifest_ref) = manifest.as_mut() {
                let mut metadata_stmt = conn
                    .prepare(
                        "
                        SELECT key, value
                        FROM session_metadata
                        WHERE session_id = ?1
                        ORDER BY key ASC
                        ",
                    )
                    .map_err(|error| {
                        MemoryError::storage(format!("failed to prepare metadata query: {error}"))
                    })?;
                let metadata_rows = metadata_stmt
                    .query_map(params![session_id.as_str()], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                    })
                    .map_err(|error| {
                        MemoryError::storage(format!("failed to query metadata rows: {error}"))
                    })?;
                for pair in metadata_rows {
                    let (key, value) = pair.map_err(|error| {
                        MemoryError::storage(format!("failed to read metadata row: {error}"))
                    })?;
                    manifest_ref.metadata.insert(key, value);
                }

                let mut plan_stmt = conn
                    .prepare(
                        "
                        SELECT
                            step_kind,
                            command_program,
                            command_args_json,
                            shell_name,
                            shell_script
                        FROM init_plan_steps
                        WHERE session_id = ?1
                        ORDER BY step_index ASC
                        ",
                    )
                    .map_err(|error| {
                        MemoryError::storage(format!("failed to prepare init plan query: {error}"))
                    })?;
                let plan_rows = plan_stmt
                    .query_map(params![session_id.as_str()], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, Option<String>>(1)?,
                            row.get::<_, Option<String>>(2)?,
                            row.get::<_, Option<String>>(3)?,
                            row.get::<_, Option<String>>(4)?,
                        ))
                    })
                    .map_err(|error| {
                        MemoryError::storage(format!("failed to query init plan rows: {error}"))
                    })?;

                let mut steps = Vec::new();
                for row in plan_rows {
                    let (step_kind, command_program, command_args_json, shell_name, shell_script) =
                        row.map_err(|error| {
                            MemoryError::storage(format!("failed to read init plan row: {error}"))
                        })?;

                    let step = match step_kind.as_str() {
                        "command" => {
                            let program = command_program.ok_or_else(|| {
                                MemoryError::storage("init plan command step missing program")
                            })?;
                            let args_json = command_args_json.ok_or_else(|| {
                                MemoryError::storage("init plan command step missing args")
                            })?;
                            let args: Vec<String> =
                                serde_json::from_str(&args_json).map_err(|error| {
                                    MemoryError::storage(format!(
                                        "failed to decode init command args JSON: {error}"
                                    ))
                                })?;
                            InitStep::Command(InitCommand { program, args })
                        }
                        "shell" => {
                            let shell =
                                init_shell_from_str(shell_name.as_deref().ok_or_else(|| {
                                    MemoryError::storage("init plan shell step missing shell name")
                                })?)?;
                            let script = shell_script.ok_or_else(|| {
                                MemoryError::storage("init plan shell step missing script")
                            })?;
                            InitStep::Shell(InitShellScript { shell, script })
                        }
                        _ => {
                            return Err(MemoryError::storage(format!(
                                "unsupported init plan step kind '{step_kind}'"
                            )));
                        }
                    };
                    steps.push(step);
                }

                if !steps.is_empty() {
                    manifest_ref.init_plan = Some(InitPlan { steps });
                }
            }

            let mut feature_stmt = conn
                .prepare(
                    "
                    SELECT feature_id, category, description, steps_json, passes
                    FROM features
                    WHERE session_id = ?1
                    ORDER BY position ASC, id ASC
                    ",
                )
                .map_err(|error| {
                    MemoryError::storage(format!("failed to prepare feature query: {error}"))
                })?;
            let feature_rows = feature_stmt
                .query_map(params![session_id.as_str()], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                })
                .map_err(|error| {
                    MemoryError::storage(format!("failed to query feature rows: {error}"))
                })?;

            let mut feature_list = Vec::new();
            for row in feature_rows {
                let (id, category, description, steps_json, passes_int) = row.map_err(|error| {
                    MemoryError::storage(format!("failed to read feature row: {error}"))
                })?;
                let steps: Vec<String> = serde_json::from_str(&steps_json).map_err(|error| {
                    MemoryError::storage(format!("failed to decode feature steps JSON: {error}"))
                })?;
                feature_list.push(FeatureRecord {
                    id,
                    category,
                    description,
                    steps,
                    passes: passes_int != 0,
                });
            }

            let mut progress_stmt = conn
                .prepare(
                    "
                    SELECT run_id, summary, created_at_secs, created_at_nanos
                    FROM progress_entries
                    WHERE session_id = ?1
                    ORDER BY id ASC
                    ",
                )
                .map_err(|error| {
                    MemoryError::storage(format!("failed to prepare progress query: {error}"))
                })?;
            let progress_rows = progress_stmt
                .query_map(params![session_id.as_str()], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                })
                .map_err(|error| {
                    MemoryError::storage(format!("failed to query progress rows: {error}"))
                })?;

            let mut recent_progress = Vec::new();
            for row in progress_rows {
                let (run_id, summary, created_at_secs, created_at_nanos) =
                    row.map_err(|error| {
                        MemoryError::storage(format!("failed to read progress row: {error}"))
                    })?;
                recent_progress.push(ProgressEntry {
                    run_id,
                    summary,
                    created_at: decode_system_time(created_at_secs, created_at_nanos)?,
                });
            }

            let mut checkpoint_stmt = conn
                .prepare(
                    "
                    SELECT
                        run_id,
                        started_at_secs,
                        started_at_nanos,
                        completed_at_secs,
                        completed_at_nanos,
                        status,
                        note
                    FROM run_checkpoints
                    WHERE session_id = ?1
                    ORDER BY id ASC
                    ",
                )
                .map_err(|error| {
                    MemoryError::storage(format!("failed to prepare checkpoint query: {error}"))
                })?;
            let checkpoint_rows = checkpoint_stmt
                .query_map(params![session_id.as_str()], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                        row.get::<_, Option<i64>>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, Option<String>>(6)?,
                    ))
                })
                .map_err(|error| {
                    MemoryError::storage(format!("failed to query checkpoint rows: {error}"))
                })?;

            let mut checkpoints = Vec::new();
            for row in checkpoint_rows {
                let (
                    run_id,
                    started_at_secs,
                    started_at_nanos,
                    completed_at_secs,
                    completed_at_nanos,
                    status,
                    note,
                ) = row.map_err(|error| {
                    MemoryError::storage(format!("failed to read checkpoint row: {error}"))
                })?;

                let completed_at = match (completed_at_secs, completed_at_nanos) {
                    (Some(secs), Some(nanos)) => Some(decode_system_time(secs, nanos)?),
                    (None, None) => None,
                    _ => {
                        return Err(MemoryError::storage(
                            "checkpoint completed timestamp must include both seconds and nanos",
                        ));
                    }
                };

                checkpoints.push(RunCheckpoint {
                    run_id,
                    started_at: decode_system_time(started_at_secs, started_at_nanos)?,
                    completed_at,
                    status: run_status_from_str(&status)?,
                    note,
                });
            }

            Ok(BootstrapState {
                manifest,
                feature_list,
                recent_progress,
                checkpoints,
            })
        })
    }

    fn save_manifest<'a>(
        &'a self,
        session_id: &'a SessionId,
        manifest: SessionManifest,
    ) -> BoxFuture<'a, Result<(), MemoryError>> {
        Box::pin(async move {
            let conn = self.connection()?;
            Self::save_manifest_rows(&conn, session_id, &manifest)
        })
    }

    fn append_progress_entry<'a>(
        &'a self,
        session_id: &'a SessionId,
        entry: ProgressEntry,
    ) -> BoxFuture<'a, Result<(), MemoryError>> {
        Box::pin(async move {
            let conn = self.connection()?;
            let (created_at_secs, created_at_nanos) = encode_system_time(entry.created_at)?;
            conn.execute(
                "
                INSERT INTO progress_entries (
                    session_id,
                    run_id,
                    summary,
                    created_at_secs,
                    created_at_nanos
                )
                VALUES (?1, ?2, ?3, ?4, ?5)
                ",
                params![
                    session_id.as_str(),
                    entry.run_id,
                    entry.summary,
                    created_at_secs,
                    created_at_nanos,
                ],
            )
            .map_err(|error| {
                MemoryError::storage(format!("failed to append progress entry: {error}"))
            })?;
            Ok(())
        })
    }

    fn replace_feature_list<'a>(
        &'a self,
        session_id: &'a SessionId,
        features: Vec<FeatureRecord>,
    ) -> BoxFuture<'a, Result<(), MemoryError>> {
        Box::pin(async move {
            let conn = self.connection()?;
            Self::replace_features(&conn, session_id, &features)
        })
    }

    fn update_feature_pass<'a>(
        &'a self,
        session_id: &'a SessionId,
        feature_id: &'a str,
        passes: bool,
    ) -> BoxFuture<'a, Result<(), MemoryError>> {
        Box::pin(async move {
            let conn = self.connection()?;
            let row_id = conn
                .query_row(
                    "
                    SELECT id
                    FROM features
                    WHERE session_id = ?1 AND feature_id = ?2
                    ORDER BY position ASC, id ASC
                    LIMIT 1
                    ",
                    params![session_id.as_str(), feature_id],
                    |row| row.get::<_, i64>(0),
                )
                .optional()
                .map_err(|error| {
                    MemoryError::storage(format!("failed to query feature update target: {error}"))
                })?;

            let Some(row_id) = row_id else {
                return Err(MemoryError::not_found(format!(
                    "feature '{feature_id}' not found"
                )));
            };

            conn.execute(
                "UPDATE features SET passes = ?1 WHERE id = ?2",
                params![if passes { 1_i64 } else { 0_i64 }, row_id],
            )
            .map_err(|error| {
                MemoryError::storage(format!("failed to update feature pass status: {error}"))
            })?;
            Ok(())
        })
    }

    fn record_run_checkpoint<'a>(
        &'a self,
        session_id: &'a SessionId,
        checkpoint: RunCheckpoint,
    ) -> BoxFuture<'a, Result<(), MemoryError>> {
        Box::pin(async move {
            let conn = self.connection()?;
            let (started_secs, started_nanos) = encode_system_time(checkpoint.started_at)?;
            let (completed_secs, completed_nanos) = match checkpoint.completed_at {
                Some(completed_at) => {
                    let (secs, nanos) = encode_system_time(completed_at)?;
                    (Some(secs), Some(nanos))
                }
                None => (None, None),
            };
            conn.execute(
                "
                INSERT INTO run_checkpoints (
                    session_id,
                    run_id,
                    started_at_secs,
                    started_at_nanos,
                    completed_at_secs,
                    completed_at_nanos,
                    status,
                    note
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                ",
                params![
                    session_id.as_str(),
                    checkpoint.run_id,
                    started_secs,
                    started_nanos,
                    completed_secs,
                    completed_nanos,
                    run_status_to_str(checkpoint.status),
                    checkpoint.note,
                ],
            )
            .map_err(|error| {
                MemoryError::storage(format!("failed to record checkpoint: {error}"))
            })?;
            Ok(())
        })
    }

    fn load_transcript_messages<'a>(
        &'a self,
        session_id: &'a SessionId,
    ) -> BoxFuture<'a, Result<Vec<Message>, MemoryError>> {
        Box::pin(async move {
            let conn = self.connection()?;
            let mut stmt = conn
                .prepare(
                    "
                    SELECT role, content
                    FROM transcript_messages
                    WHERE session_id = ?1
                    ORDER BY id ASC
                    ",
                )
                .map_err(|error| {
                    MemoryError::storage(format!("failed to prepare transcript query: {error}"))
                })?;
            let rows = stmt
                .query_map(params![session_id.as_str()], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|error| {
                    MemoryError::storage(format!("failed to query transcript rows: {error}"))
                })?;
            let mut messages = Vec::new();
            for row in rows {
                let (role, content) = row.map_err(|error| {
                    MemoryError::storage(format!("failed to read transcript row: {error}"))
                })?;
                messages.push(Message {
                    role: role_from_str(&role)?,
                    content,
                });
            }
            Ok(messages)
        })
    }

    fn append_transcript_messages<'a>(
        &'a self,
        session_id: &'a SessionId,
        messages: Vec<Message>,
    ) -> BoxFuture<'a, Result<(), MemoryError>> {
        Box::pin(async move {
            let conn = self.connection()?;
            for message in messages {
                conn.execute(
                    "
                    INSERT INTO transcript_messages (session_id, role, content)
                    VALUES (?1, ?2, ?3)
                    ",
                    params![
                        session_id.as_str(),
                        role_to_str(message.role),
                        message.content
                    ],
                )
                .map_err(|error| {
                    MemoryError::storage(format!("failed to append transcript message: {error}"))
                })?;
            }
            Ok(())
        })
    }
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

fn run_status_to_str(status: RunStatus) -> &'static str {
    match status {
        RunStatus::InProgress => "in_progress",
        RunStatus::Succeeded => "succeeded",
        RunStatus::Failed => "failed",
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

fn init_shell_to_str(shell: InitShell) -> &'static str {
    match shell {
        InitShell::Bash => "bash",
        InitShell::Sh => "sh",
        InitShell::Pwsh => "pwsh",
        InitShell::Cmd => "cmd",
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

fn role_to_str(role: Role) -> &'static str {
    match role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
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

pub(crate) fn default_sqlite_path() -> PathBuf {
    if let Some(explicit) = std::env::var_os("FMEMORY_SQLITE_PATH") {
        return PathBuf::from(explicit);
    }

    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        return PathBuf::from(home)
            .join(".fiddlesticks")
            .join("fmemory.sqlite3");
    }

    PathBuf::from("fmemory.sqlite3")
}
