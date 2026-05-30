use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime};

use chrono::{DateTime, NaiveDateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;
use xxhash_rust::xxh64::xxh64;

use crate::{
    EntryKind, EntryMeta, PeerId, PeerSession, RelPath, RetentionPolicy, Timestamp,
    TransportError,
};

const SUMMARY: &str = "snapshot: SQLite peer history, path hashing, tombstones, and snapshot upload lifecycle.";

const LIVE_DB: &str = ".kitchensync/snapshot.db";
const SWAP_NEW: &str = ".kitchensync/SWAP/snapshot.db/new";
const SWAP_OLD: &str = ".kitchensync/SWAP/snapshot.db/old";

pub struct SnapshotStore {
    peer: PeerId,
    path: PathBuf,
    connection: Connection,
    had_changes: bool,
}

pub struct SnapshotOpen {
    pub store: SnapshotStore,
    pub had_history_at_startup: bool,
}

impl fmt::Debug for SnapshotOpen {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SnapshotOpen")
            .field("peer", &self.store.peer)
            .field("path", &self.store.path)
            .field("had_changes", &self.store.had_changes)
            .field("had_history_at_startup", &self.had_history_at_startup)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotStartupMode {
    Normal,
    DryRun,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotEntryKind {
    File,
    Directory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotRow {
    pub path: RelPath,
    pub kind: SnapshotEntryKind,
    pub mod_time: Timestamp,
    pub byte_size: i64,
    pub last_seen: Option<Timestamp>,
    pub deleted_time: Option<Timestamp>,
}

pub struct SnapshotCleanupScope<'a> {
    pub listed_paths: &'a dyn SnapshotListedPaths,
    pub retention: RetentionPolicy,
}

pub trait SnapshotListedPaths {
    fn contains(&self, path: &RelPath) -> bool;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotError {
    Transport {
        peer: PeerId,
        category: TransportError,
        operation: SnapshotTransportOperation,
    },
    InvalidDatabase {
        peer: PeerId,
        reason: SnapshotDatabaseError,
    },
    LocalIo {
        peer: PeerId,
        operation: SnapshotLocalOperation,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotTransportOperation {
    RecoverSwap,
    DownloadLive,
    UploadNew,
    RenameLiveToOld,
    RenameNewToLive,
    DeleteOld,
    DeleteNew,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotLocalOperation {
    CreateTempDirectory,
    CreateDatabase,
    OpenDatabase,
    FlushDatabase,
    CloseDatabase,
    ReadDatabase,
    WriteDatabase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotDatabaseError {
    OpenFailed,
    SchemaMismatch,
    UnsupportedObjects,
    Corrupt,
}

pub struct ClosedSnapshotStore {
    peer: PeerId,
    path: PathBuf,
}

pub fn summary() -> &'static str {
    SUMMARY
}

pub fn prepare_peer_snapshot(
    peer: &PeerSession,
    tmp_root: &Path,
    mode: SnapshotStartupMode,
) -> Result<SnapshotOpen, SnapshotError> {
    if mode == SnapshotStartupMode::Normal {
        recover_snapshot_swap(peer)?;
    }

    let work_dir = tmp_root.join(Uuid::new_v4().to_string());
    fs::create_dir_all(&work_dir)
        .map_err(|_| local(peer, SnapshotLocalOperation::CreateTempDirectory))?;
    let local_db = work_dir.join("snapshot.db");

    let had_history_at_startup = match download_live_snapshot(peer, &local_db) {
        Ok(()) => true,
        Err(SnapshotError::Transport {
            category: TransportError::NotFound,
            operation: SnapshotTransportOperation::DownloadLive,
            ..
        }) => {
            create_empty_database(peer, &local_db)?;
            false
        }
        Err(error) => return Err(error),
    };

    let connection = open_and_validate(peer.id, &local_db)?;
    Ok(SnapshotOpen {
        store: SnapshotStore {
            peer: peer.id,
            path: local_db,
            connection,
            had_changes: false,
        },
        had_history_at_startup,
    })
}

pub fn fresh_timestamp() -> Timestamp {
    static LAST: OnceLock<Mutex<SystemTime>> = OnceLock::new();
    let lock = LAST.get_or_init(|| Mutex::new(SystemTime::UNIX_EPOCH));
    let mut last = lock.lock().expect("fresh timestamp lock poisoned");
    let mut now = SystemTime::now();
    if now <= *last {
        now = *last + Duration::from_micros(1);
    }
    *last = now;
    Timestamp(format_timestamp(now))
}

impl SnapshotStore {
    pub fn peer(&self) -> PeerId {
        self.peer
    }

    pub fn had_changes(&self) -> bool {
        self.had_changes
    }

    pub fn lookup(&self, path: &RelPath) -> Result<Option<SnapshotRow>, SnapshotError> {
        reject_root_path(self.peer, path)?;
        let id = path_id(path.as_str());
        let stored = self
            .connection
            .query_row(
                "SELECT mod_time, byte_size, last_seen, deleted_time FROM snapshot WHERE id = ?1",
                params![id],
                |row| {
                    let mod_time: String = row.get(0)?;
                    let byte_size: i64 = row.get(1)?;
                    let last_seen: Option<String> = row.get(2)?;
                    let deleted_time: Option<String> = row.get(3)?;
                    Ok((mod_time, byte_size, last_seen, deleted_time))
                },
            )
            .optional()
            .map_err(|_| invalid(self.peer, SnapshotDatabaseError::Corrupt))?;

        Ok(stored.map(|(mod_time, byte_size, last_seen, deleted_time)| SnapshotRow {
            path: path.clone(),
            kind: if byte_size == -1 {
                SnapshotEntryKind::Directory
            } else {
                SnapshotEntryKind::File
            },
            mod_time: Timestamp(mod_time),
            byte_size,
            last_seen: last_seen.map(Timestamp),
            deleted_time: deleted_time.map(Timestamp),
        }))
    }

    pub fn upsert_confirmed_present(
        &mut self,
        path: &RelPath,
        meta: &EntryMeta,
    ) -> Result<Timestamp, SnapshotError> {
        reject_root_path(self.peer, path)?;
        let last_seen = fresh_timestamp();
        self.upsert_row(path, meta, Some(&last_seen))?;
        Ok(last_seen)
    }

    pub fn upsert_intended_copy(
        &mut self,
        path: &RelPath,
        winning_meta: &EntryMeta,
    ) -> Result<(), SnapshotError> {
        reject_root_path(self.peer, path)?;
        let existing_last_seen = self
            .connection
            .query_row(
                "SELECT last_seen FROM snapshot WHERE id = ?1",
                params![path_id(path.as_str())],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map_err(|_| invalid(self.peer, SnapshotDatabaseError::Corrupt))?
            .flatten()
            .map(Timestamp);
        self.upsert_row(path, winning_meta, existing_last_seen.as_ref())
    }

    pub fn mark_copy_complete(&mut self, path: &RelPath) -> Result<Timestamp, SnapshotError> {
        reject_root_path(self.peer, path)?;
        let last_seen = fresh_timestamp();
        self.connection
            .execute(
                "UPDATE snapshot SET last_seen = ?2 WHERE id = ?1",
                params![path_id(path.as_str()), last_seen.0],
            )
            .map_err(|_| invalid(self.peer, SnapshotDatabaseError::Corrupt))?;
        self.had_changes = true;
        Ok(last_seen)
    }

    pub fn mark_absent(&mut self, path: &RelPath) -> Result<(), SnapshotError> {
        reject_root_path(self.peer, path)?;
        self.connection
            .execute(
                "UPDATE snapshot
                 SET deleted_time = last_seen
                 WHERE id = ?1 AND deleted_time IS NULL",
                params![path_id(path.as_str())],
            )
            .map_err(|_| invalid(self.peer, SnapshotDatabaseError::Corrupt))?;
        self.had_changes = true;
        Ok(())
    }

    pub fn mark_displaced(
        &mut self,
        path: &RelPath,
        kind: SnapshotEntryKind,
    ) -> Result<(), SnapshotError> {
        reject_root_path(self.peer, path)?;
        let id = path_id(path.as_str());
        let deletion_estimate = self
            .connection
            .query_row(
                "SELECT last_seen FROM snapshot WHERE id = ?1 AND deleted_time IS NULL",
                params![id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map_err(|_| invalid(self.peer, SnapshotDatabaseError::Corrupt))?
            .flatten();

        self.connection
            .execute(
                "UPDATE snapshot
                 SET deleted_time = last_seen
                 WHERE id = ?1 AND deleted_time IS NULL",
                params![id],
            )
            .map_err(|_| invalid(self.peer, SnapshotDatabaseError::Corrupt))?;

        if kind == SnapshotEntryKind::Directory {
            if let Some(deletion_estimate) = deletion_estimate {
                self.cascade_displaced_directory(&id, &deletion_estimate)?;
            }
        }

        self.had_changes = true;
        Ok(())
    }

    pub fn cleanup_stale_rows(
        &mut self,
        scope: SnapshotCleanupScope<'_>,
    ) -> Result<(), SnapshotError> {
        let cutoff = retention_cutoff(scope.retention.keep_del_days);
        let rows = self.all_rows_for_cleanup()?;
        let mut removed_any = false;

        for row in rows {
            let Some(path) = row.path else {
                continue;
            };

            let remove = match (row.deleted_time.as_deref(), row.last_seen.as_deref()) {
                (Some(deleted), _) => timestamp_before_or_equal(deleted, &cutoff),
                (None, None) => !scope.listed_paths.contains(&path),
                (None, Some(last_seen)) => {
                    !scope.listed_paths.contains(&path)
                        && timestamp_before_or_equal(last_seen, &cutoff)
                }
            };

            if remove {
                self.connection
                    .execute("DELETE FROM snapshot WHERE id = ?1", params![row.id])
                    .map_err(|_| invalid(self.peer, SnapshotDatabaseError::Corrupt))?;
                removed_any = true;
            }
        }

        if removed_any {
            self.had_changes = true;
        }
        Ok(())
    }

    pub fn flush(&mut self) -> Result<(), SnapshotError> {
        self.connection
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .map_err(|_| local_peer(self.peer, SnapshotLocalOperation::FlushDatabase))?;
        Ok(())
    }

    pub fn close(mut self) -> Result<ClosedSnapshotStore, SnapshotError> {
        self.flush()?;
        let peer = self.peer;
        let path = self.path.clone();
        match self.connection.close() {
            Ok(()) => Ok(ClosedSnapshotStore { peer, path }),
            Err((_connection, _error)) => {
                Err(local_peer(peer, SnapshotLocalOperation::CloseDatabase))
            }
        }
    }

    fn upsert_row(
        &mut self,
        path: &RelPath,
        meta: &EntryMeta,
        last_seen: Option<&Timestamp>,
    ) -> Result<(), SnapshotError> {
        let id = path_id(path.as_str());
        let parent_id = parent_id(path.as_str());
        let basename = basename(path.as_str());
        let byte_size = match meta.kind {
            EntryKind::Directory => -1,
            EntryKind::File => meta.byte_size,
        };
        self.connection
            .execute(
                "INSERT INTO snapshot
                    (id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(id) DO UPDATE SET
                    parent_id = excluded.parent_id,
                    basename = excluded.basename,
                    mod_time = excluded.mod_time,
                    byte_size = excluded.byte_size,
                    last_seen = excluded.last_seen,
                    deleted_time = excluded.deleted_time",
                params![
                    id,
                    parent_id,
                    basename,
                    meta.mod_time.0,
                    byte_size,
                    last_seen.map(|timestamp| timestamp.0.as_str()),
                    Option::<&str>::None,
                ],
            )
            .map_err(|_| invalid(self.peer, SnapshotDatabaseError::Corrupt))?;
        self.had_changes = true;
        Ok(())
    }

    fn cascade_displaced_directory(
        &mut self,
        root_id: &str,
        deletion_estimate: &str,
    ) -> Result<(), SnapshotError> {
        let mut stack = vec![root_id.to_string()];

        while let Some(parent) = stack.pop() {
            let children = {
                let mut statement = self
                    .connection
                    .prepare(
                        "SELECT id, byte_size FROM snapshot
                         WHERE parent_id = ?1 AND deleted_time IS NULL",
                    )
                    .map_err(|_| invalid(self.peer, SnapshotDatabaseError::Corrupt))?;
                let rows = statement
                    .query_map(params![parent], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                    })
                    .map_err(|_| invalid(self.peer, SnapshotDatabaseError::Corrupt))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| invalid(self.peer, SnapshotDatabaseError::Corrupt))?;
                rows
            };

            for (child_id, byte_size) in children {
                self.connection
                    .execute(
                        "UPDATE snapshot SET deleted_time = ?2
                         WHERE id = ?1 AND deleted_time IS NULL",
                        params![child_id, deletion_estimate],
                    )
                    .map_err(|_| invalid(self.peer, SnapshotDatabaseError::Corrupt))?;
                if byte_size == -1 {
                    stack.push(child_id);
                }
            }
        }
        Ok(())
    }

    fn all_rows_for_cleanup(&self) -> Result<Vec<CleanupRow>, SnapshotError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, parent_id, basename, last_seen, deleted_time
                 FROM snapshot",
            )
            .map_err(|_| invalid(self.peer, SnapshotDatabaseError::Corrupt))?;
        let raw_rows = statement
            .query_map([], |row| {
                Ok(RawCleanupRow {
                    id: row.get(0)?,
                    parent_id: row.get(1)?,
                    basename: row.get(2)?,
                    last_seen: row.get(3)?,
                    deleted_time: row.get(4)?,
                })
            })
            .map_err(|_| invalid(self.peer, SnapshotDatabaseError::Corrupt))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| invalid(self.peer, SnapshotDatabaseError::Corrupt))?;

        Ok(resolve_cleanup_paths(raw_rows))
    }
}

pub fn upload_peer_snapshot(
    peer: &PeerSession,
    store: ClosedSnapshotStore,
) -> Result<(), SnapshotError> {
    if store.peer != peer.id {
        return Err(invalid(peer.id, SnapshotDatabaseError::SchemaMismatch));
    }

    let mut local = fs::File::open(&store.path)
        .map_err(|_| local(peer, SnapshotLocalOperation::ReadDatabase))?;
    let mut remote = peer
        .transport
        .open_write(&rel(SWAP_NEW))
        .map_err(|category| transport(peer, category, SnapshotTransportOperation::UploadNew))?;
    std::io::copy(&mut local, &mut remote).map_err(|_| {
        transport(
            peer,
            TransportError::IoError,
            SnapshotTransportOperation::UploadNew,
        )
    })?;
    remote
        .close()
        .map_err(|category| transport(peer, category, SnapshotTransportOperation::UploadNew))?;

    if remote_exists(peer, LIVE_DB, SnapshotTransportOperation::RenameLiveToOld)? {
        peer.transport
            .rename_no_overwrite(&rel(LIVE_DB), &rel(SWAP_OLD))
            .map_err(|category| {
                transport(peer, category, SnapshotTransportOperation::RenameLiveToOld)
            })?;
    }

    peer.transport
        .rename_no_overwrite(&rel(SWAP_NEW), &rel(LIVE_DB))
        .map_err(|category| {
            transport(peer, category, SnapshotTransportOperation::RenameNewToLive)
        })?;

    if remote_exists(peer, SWAP_OLD, SnapshotTransportOperation::DeleteOld)? {
        peer.transport
            .delete_file(&rel(SWAP_OLD))
            .map_err(|category| transport(peer, category, SnapshotTransportOperation::DeleteOld))?;
    }

    Ok(())
}

fn recover_snapshot_swap(peer: &PeerSession) -> Result<(), SnapshotError> {
    let live = remote_exists(peer, LIVE_DB, SnapshotTransportOperation::RecoverSwap)?;
    let old = remote_exists(peer, SWAP_OLD, SnapshotTransportOperation::RecoverSwap)?;
    let new = remote_exists(peer, SWAP_NEW, SnapshotTransportOperation::RecoverSwap)?;

    match (old, new, live) {
        (true, true, true) | (true, false, true) => {
            if new {
                delete_if_exists(peer, SWAP_NEW, SnapshotTransportOperation::DeleteNew)?;
            }
            delete_if_exists(peer, SWAP_OLD, SnapshotTransportOperation::DeleteOld)?;
        }
        (true, true, false) => {
            peer.transport
                .rename_no_overwrite(&rel(SWAP_NEW), &rel(LIVE_DB))
                .map_err(|category| {
                    transport(peer, category, SnapshotTransportOperation::RenameNewToLive)
                })?;
            delete_if_exists(peer, SWAP_OLD, SnapshotTransportOperation::DeleteOld)?;
        }
        (true, false, false) => {
            peer.transport
                .rename_no_overwrite(&rel(SWAP_OLD), &rel(LIVE_DB))
                .map_err(|category| {
                    transport(peer, category, SnapshotTransportOperation::RenameNewToLive)
                })?;
        }
        (false, true, true) => {
            delete_if_exists(peer, SWAP_NEW, SnapshotTransportOperation::DeleteNew)?;
        }
        (false, true, false) => {
            peer.transport
                .rename_no_overwrite(&rel(SWAP_NEW), &rel(LIVE_DB))
                .map_err(|category| {
                    transport(peer, category, SnapshotTransportOperation::RenameNewToLive)
                })?;
        }
        (false, false, _) => {}
    }

    Ok(())
}

fn download_live_snapshot(peer: &PeerSession, local_db: &Path) -> Result<(), SnapshotError> {
    let mut remote = peer
        .transport
        .open_read(&rel(LIVE_DB))
        .map_err(|category| transport(peer, category, SnapshotTransportOperation::DownloadLive))?;
    let mut local_file = fs::File::create(local_db)
        .map_err(|_| local(peer, SnapshotLocalOperation::WriteDatabase))?;
    std::io::copy(&mut remote, &mut local_file).map_err(|_| {
        transport(
            peer,
            TransportError::IoError,
            SnapshotTransportOperation::DownloadLive,
        )
    })?;
    local_file
        .flush()
        .map_err(|_| local(peer, SnapshotLocalOperation::WriteDatabase))?;
    Ok(())
}

fn create_empty_database(peer: &PeerSession, path: &Path) -> Result<(), SnapshotError> {
    let connection = Connection::open(path)
        .map_err(|_| local(peer, SnapshotLocalOperation::CreateDatabase))?;
    initialize_schema(&connection)
        .map_err(|_| local(peer, SnapshotLocalOperation::CreateDatabase))?;
    connection
        .close()
        .map_err(|_| local(peer, SnapshotLocalOperation::CloseDatabase))?;
    Ok(())
}

fn open_and_validate(peer: PeerId, path: &Path) -> Result<Connection, SnapshotError> {
    let connection = Connection::open(path)
        .map_err(|_| local_peer(peer, SnapshotLocalOperation::OpenDatabase))?;
    validate_schema(peer, &connection)?;
    Ok(connection)
}

fn initialize_schema(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute_batch(
        "PRAGMA journal_mode = DELETE;
         CREATE TABLE IF NOT EXISTS snapshot (
            id TEXT PRIMARY KEY,
            parent_id TEXT,
            basename TEXT NOT NULL,
            mod_time TEXT NOT NULL,
            byte_size INTEGER NOT NULL,
            last_seen TEXT NULL,
            deleted_time TEXT NULL
         );
         CREATE INDEX IF NOT EXISTS snapshot_parent_id_idx ON snapshot(parent_id);
         CREATE INDEX IF NOT EXISTS snapshot_last_seen_idx ON snapshot(last_seen);
         CREATE INDEX IF NOT EXISTS snapshot_deleted_time_idx ON snapshot(deleted_time);",
    )
}

fn validate_schema(peer: PeerId, connection: &Connection) -> Result<(), SnapshotError> {
    let tables: Vec<String> = connection
        .prepare(
            "SELECT name FROM sqlite_schema
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
        )
        .and_then(|mut statement| {
            statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()
        })
        .map_err(|_| invalid(peer, SnapshotDatabaseError::Corrupt))?;
    if tables.len() != 1 || tables[0] != "snapshot" {
        return Err(invalid(peer, SnapshotDatabaseError::UnsupportedObjects));
    }

    let views: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE type = 'view'",
            [],
            |row| row.get(0),
        )
        .map_err(|_| invalid(peer, SnapshotDatabaseError::Corrupt))?;
    if views != 0 {
        return Err(invalid(peer, SnapshotDatabaseError::UnsupportedObjects));
    }

    let columns: Vec<(String, String, bool, bool)> = connection
        .prepare("PRAGMA table_info(snapshot)")
        .and_then(|mut statement| {
            statement
                .query_map([], |row| {
                    let name: String = row.get(1)?;
                    let ty: String = row.get(2)?;
                    let not_null: i64 = row.get(3)?;
                    let pk: i64 = row.get(5)?;
                    Ok((name, ty.to_ascii_uppercase(), not_null != 0, pk != 0))
                })?
                .collect::<Result<Vec<_>, _>>()
        })
        .map_err(|_| invalid(peer, SnapshotDatabaseError::Corrupt))?;

    let expected = vec![
        ("id".to_string(), "TEXT".to_string(), false, true),
        ("parent_id".to_string(), "TEXT".to_string(), false, false),
        ("basename".to_string(), "TEXT".to_string(), true, false),
        ("mod_time".to_string(), "TEXT".to_string(), true, false),
        ("byte_size".to_string(), "INTEGER".to_string(), true, false),
        ("last_seen".to_string(), "TEXT".to_string(), false, false),
        ("deleted_time".to_string(), "TEXT".to_string(), false, false),
    ];
    if columns != expected {
        return Err(invalid(peer, SnapshotDatabaseError::SchemaMismatch));
    }

    validate_stored_values(peer, connection)?;

    let indexed_columns = indexed_snapshot_columns(peer, connection)?;
    for required in ["parent_id", "last_seen", "deleted_time"] {
        if !indexed_columns.iter().any(|column| column == required) {
            return Err(invalid(peer, SnapshotDatabaseError::SchemaMismatch));
        }
    }

    Ok(())
}

fn validate_stored_values(peer: PeerId, connection: &Connection) -> Result<(), SnapshotError> {
    let mut statement = connection
        .prepare("SELECT mod_time, last_seen, deleted_time, byte_size FROM snapshot")
        .map_err(|_| invalid(peer, SnapshotDatabaseError::Corrupt))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })
        .map_err(|_| invalid(peer, SnapshotDatabaseError::Corrupt))?;

    for row in rows {
        let (mod_time, last_seen, deleted_time, byte_size) =
            row.map_err(|_| invalid(peer, SnapshotDatabaseError::Corrupt))?;
        if parse_timestamp(&mod_time).is_none()
            || last_seen
                .as_deref()
                .is_some_and(|timestamp| parse_timestamp(timestamp).is_none())
            || deleted_time
                .as_deref()
                .is_some_and(|timestamp| parse_timestamp(timestamp).is_none())
            || byte_size < -1
        {
            return Err(invalid(peer, SnapshotDatabaseError::SchemaMismatch));
        }
    }

    Ok(())
}

fn indexed_snapshot_columns(
    peer: PeerId,
    connection: &Connection,
) -> Result<Vec<String>, SnapshotError> {
    let index_names: Vec<String> = connection
        .prepare("PRAGMA index_list(snapshot)")
        .and_then(|mut statement| {
            statement
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<Result<Vec<_>, _>>()
        })
        .map_err(|_| invalid(peer, SnapshotDatabaseError::Corrupt))?;

    let mut columns = Vec::new();
    for index_name in index_names {
        let pragma = format!("PRAGMA index_info({})", quote_sql_identifier(&index_name));
        let index_columns: Vec<String> = connection
            .prepare(&pragma)
            .and_then(|mut statement| {
                statement
                    .query_map([], |row| row.get::<_, String>(2))?
                    .collect::<Result<Vec<_>, _>>()
            })
            .map_err(|_| invalid(peer, SnapshotDatabaseError::Corrupt))?;
        columns.extend(index_columns);
    }
    Ok(columns)
}

fn quote_sql_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn rel(path: &str) -> RelPath {
    RelPath::new(path).expect("snapshot internal path is a valid relative path")
}

fn remote_exists(
    peer: &PeerSession,
    path: &str,
    operation: SnapshotTransportOperation,
) -> Result<bool, SnapshotError> {
    match peer.transport.stat(&rel(path)) {
        Ok(_) => Ok(true),
        Err(TransportError::NotFound) => Ok(false),
        Err(category) => Err(transport(peer, category, operation)),
    }
}

fn delete_if_exists(
    peer: &PeerSession,
    path: &str,
    operation: SnapshotTransportOperation,
) -> Result<(), SnapshotError> {
    if remote_exists(peer, path, operation)? {
        peer.transport
            .delete_file(&rel(path))
            .map_err(|category| transport(peer, category, operation))?;
    }
    Ok(())
}

fn reject_root_path(peer: PeerId, path: &RelPath) -> Result<(), SnapshotError> {
    if path.as_str().is_empty() {
        Err(SnapshotError::InvalidDatabase {
            peer,
            reason: SnapshotDatabaseError::SchemaMismatch,
        })
    } else {
        Ok(())
    }
}

fn path_id(path: &str) -> String {
    base62_11(xxh64(path.as_bytes(), 0))
}

fn parent_id(path: &str) -> String {
    match path.rsplit_once('/') {
        Some((parent, _)) => path_id(parent),
        None => path_id("/"),
    }
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn base62_11(mut value: u64) -> String {
    const DIGITS: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let mut output = [b'0'; 11];
    for slot in output.iter_mut().rev() {
        *slot = DIGITS[(value % 62) as usize];
        value /= 62;
    }
    String::from_utf8(output.to_vec()).expect("base62 is ASCII")
}

fn format_timestamp(time: SystemTime) -> String {
    let datetime: DateTime<Utc> = time.into();
    format!(
        "{}_{:06}Z",
        datetime.format("%Y-%m-%d_%H-%M-%S"),
        datetime.timestamp_subsec_micros()
    )
}

fn parse_timestamp(value: &str) -> Option<SystemTime> {
    NaiveDateTime::parse_from_str(value, "%Y-%m-%d_%H-%M-%S_%fZ")
        .ok()
        .map(|datetime| DateTime::<Utc>::from_naive_utc_and_offset(datetime, Utc))
        .map(Into::into)
}

fn retention_cutoff(days: u32) -> String {
    let cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(days as u64 * 24 * 60 * 60))
        .unwrap_or(SystemTime::UNIX_EPOCH);
    format_timestamp(cutoff)
}

fn timestamp_before_or_equal(value: &str, cutoff: &str) -> bool {
    match (parse_timestamp(value), parse_timestamp(cutoff)) {
        (Some(value), Some(cutoff)) => value <= cutoff,
        _ => value <= cutoff,
    }
}

#[derive(Clone)]
struct RawCleanupRow {
    id: String,
    parent_id: Option<String>,
    basename: String,
    last_seen: Option<String>,
    deleted_time: Option<String>,
}

struct CleanupRow {
    id: String,
    path: Option<RelPath>,
    last_seen: Option<String>,
    deleted_time: Option<String>,
}

fn resolve_cleanup_paths(rows: Vec<RawCleanupRow>) -> Vec<CleanupRow> {
    let mut output = Vec::with_capacity(rows.len());
    for row in &rows {
        output.push(CleanupRow {
            id: row.id.clone(),
            path: resolve_row_path(row, &rows).and_then(|path| RelPath::new(path).ok()),
            last_seen: row.last_seen.clone(),
            deleted_time: row.deleted_time.clone(),
        });
    }
    output
}

fn resolve_row_path(row: &RawCleanupRow, rows: &[RawCleanupRow]) -> Option<String> {
    let root_id = path_id("/");
    let mut names = vec![row.basename.as_str()];
    let mut parent = row.parent_id.as_deref()?;
    let mut guard = 0usize;

    while parent != root_id {
        guard += 1;
        if guard > rows.len() {
            return None;
        }
        let parent_row = rows.iter().find(|candidate| candidate.id == parent)?;
        names.push(parent_row.basename.as_str());
        parent = parent_row.parent_id.as_deref()?;
    }

    names.reverse();
    Some(names.join("/"))
}

fn transport(
    peer: &PeerSession,
    category: TransportError,
    operation: SnapshotTransportOperation,
) -> SnapshotError {
    SnapshotError::Transport {
        peer: peer.id,
        category,
        operation,
    }
}

fn local(peer: &PeerSession, operation: SnapshotLocalOperation) -> SnapshotError {
    local_peer(peer.id, operation)
}

fn local_peer(peer: PeerId, operation: SnapshotLocalOperation) -> SnapshotError {
    SnapshotError::LocalIo { peer, operation }
}

fn invalid(peer: PeerId, reason: SnapshotDatabaseError) -> SnapshotError {
    SnapshotError::InvalidDatabase { peer, reason }
}
