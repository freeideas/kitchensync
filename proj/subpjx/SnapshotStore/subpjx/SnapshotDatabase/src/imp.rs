use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use crate::api::*;

struct SnapshotDatabaseImpl {
    snapshotcleanup: std::sync::Arc<dyn snapshotstore_snapshotdatabase_snapshotcleanup::SnapshotCleanup>,
    snapshotfile: std::sync::Arc<dyn snapshotstore_snapshotdatabase_snapshotfile::SnapshotFile>,
    snapshotrows: std::sync::Arc<dyn snapshotstore_snapshotdatabase_snapshotrows::SnapshotRows>,
    open_databases: Mutex<HashMap<PathBuf, snapshotstore_snapshotdatabase_snapshotfile::SnapshotFileOpenDatabase>>,
}

impl SnapshotDatabaseImpl {
    fn error(kind: SnapshotDatabaseErrorKind, message: impl Into<String>) -> SnapshotDatabaseError {
        SnapshotDatabaseError {
            kind,
            message: message.into(),
        }
    }

    fn file_error(error: snapshotstore_snapshotdatabase_snapshotfile::SnapshotFileError) -> SnapshotDatabaseError {
        let kind = match error.reason {
            snapshotstore_snapshotdatabase_snapshotfile::SnapshotFileErrorReason::SqliteOpen => {
                SnapshotDatabaseErrorKind::SqliteOpen
            }
            snapshotstore_snapshotdatabase_snapshotfile::SnapshotFileErrorReason::RollbackJournalSetup => {
                SnapshotDatabaseErrorKind::RollbackJournalMode
            }
            snapshotstore_snapshotdatabase_snapshotfile::SnapshotFileErrorReason::SchemaCreation => {
                SnapshotDatabaseErrorKind::SchemaCreation
            }
            snapshotstore_snapshotdatabase_snapshotfile::SnapshotFileErrorReason::SchemaValidation => {
                SnapshotDatabaseErrorKind::SchemaValidation
            }
            snapshotstore_snapshotdatabase_snapshotfile::SnapshotFileErrorReason::TransactionFinish => {
                SnapshotDatabaseErrorKind::TransactionCompletion
            }
            snapshotstore_snapshotdatabase_snapshotfile::SnapshotFileErrorReason::StatementFinalization => {
                SnapshotDatabaseErrorKind::ResourceFinalization
            }
            snapshotstore_snapshotdatabase_snapshotfile::SnapshotFileErrorReason::ReaderOrCursorFinish => {
                SnapshotDatabaseErrorKind::ResourceFinalization
            }
            snapshotstore_snapshotdatabase_snapshotfile::SnapshotFileErrorReason::ConnectionClose => {
                SnapshotDatabaseErrorKind::ConnectionClose
            }
            snapshotstore_snapshotdatabase_snapshotfile::SnapshotFileErrorReason::OwnedResourceStillOpen => {
                SnapshotDatabaseErrorKind::ResourceFinalization
            }
        };
        Self::error(kind, error.detail)
    }

    fn row_error(error: rusqlite::Error) -> SnapshotDatabaseError {
        let kind = match error {
            rusqlite::Error::InvalidParameterName(_) => SnapshotDatabaseErrorKind::InvalidRowIdentity,
            _ => SnapshotDatabaseErrorKind::RowMutation,
        };
        Self::error(kind, error.to_string())
    }

    fn cleanup_error(error: rusqlite::Error) -> SnapshotDatabaseError {
        Self::error(SnapshotDatabaseErrorKind::Cleanup, error.to_string())
    }

    fn lookup_error(error: rusqlite::Error) -> SnapshotDatabaseError {
        Self::error(SnapshotDatabaseErrorKind::RowLookup, error.to_string())
    }

    fn missing_open_database(path: &Path) -> SnapshotDatabaseError {
        Self::error(
            SnapshotDatabaseErrorKind::SqliteOpen,
            format!("snapshot database is not open: {}", path.display()),
        )
    }

    fn lock_open_databases(
        &self,
        kind: SnapshotDatabaseErrorKind,
    ) -> SnapshotDatabaseResult<std::sync::MutexGuard<'_, HashMap<PathBuf, snapshotstore_snapshotdatabase_snapshotfile::SnapshotFileOpenDatabase>>> {
        self.open_databases
            .lock()
            .map_err(|_| Self::error(kind, "open snapshot database state is poisoned"))
    }
}

fn to_rows_identity(identity: &SnapshotRowIdentity) -> snapshotstore_snapshotdatabase_snapshotrows::SnapshotRowIdentity {
    snapshotstore_snapshotdatabase_snapshotrows::SnapshotRowIdentity {
        id: identity.id.clone(),
        parent_id: identity.parent_id.clone(),
        basename: identity.basename.clone(),
    }
}

fn to_rows_facts(facts: &SnapshotRowFacts) -> snapshotstore_snapshotdatabase_snapshotrows::SnapshotRowFacts {
    snapshotstore_snapshotdatabase_snapshotrows::SnapshotRowFacts {
        identity: to_rows_identity(&facts.identity),
        mod_time: facts.mod_time.clone(),
        byte_size: facts.byte_size,
    }
}

fn read_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SnapshotRow> {
    Ok(SnapshotRow {
        identity: SnapshotRowIdentity {
            id: row.get(0)?,
            parent_id: row.get(1)?,
            basename: row.get(2)?,
        },
        mod_time: row.get(3)?,
        byte_size: row.get(4)?,
        last_seen: row.get(5)?,
        deleted_time: row.get(6)?,
    })
}

impl SnapshotDatabase for SnapshotDatabaseImpl {
    fn create_empty( &self, local_path: &Path, ) -> SnapshotDatabaseResult<SnapshotDatabaseHandle> {
        let database = self
            .snapshotfile
            .create_new_snapshot_database(local_path.to_path_buf())
            .map_err(Self::file_error)?;
        let handle = SnapshotDatabaseHandle {
            local_path: database.local_snapshot_db_path.clone(),
        };
        self.lock_open_databases(SnapshotDatabaseErrorKind::SqliteOpen)?
            .insert(handle.local_path.clone(), database);
        Ok(handle)
    }
    fn open_existing( &self, local_path: &Path, ) -> SnapshotDatabaseResult<SnapshotDatabaseHandle> {
        let database = self
            .snapshotfile
            .open_existing_snapshot_database(local_path.to_path_buf())
            .map_err(Self::file_error)?;
        let handle = SnapshotDatabaseHandle {
            local_path: database.local_snapshot_db_path.clone(),
        };
        self.lock_open_databases(SnapshotDatabaseErrorKind::SqliteOpen)?
            .insert(handle.local_path.clone(), database);
        Ok(handle)
    }
    fn lookup_row( &self, database: &SnapshotDatabaseHandle, id: &str, ) -> SnapshotDatabaseResult<Option<SnapshotRow>> {
        let mut open_databases = self.lock_open_databases(SnapshotDatabaseErrorKind::RowLookup)?;
        let open_database = open_databases
            .get_mut(&database.local_path)
            .ok_or_else(|| Self::missing_open_database(&database.local_path))?;

        let mut statement = open_database
            .connection
            .prepare(
                "SELECT id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time
                 FROM snapshot
                 WHERE id = ?1",
            )
            .map_err(Self::lookup_error)?;
        let mut rows = statement.query([id]).map_err(Self::lookup_error)?;
        match rows.next().map_err(Self::lookup_error)? {
            Some(row) => read_row(row).map(Some).map_err(Self::lookup_error),
            None => Ok(None),
        }
    }
    fn list_child_rows( &self, database: &SnapshotDatabaseHandle, parent_id: &str, ) -> SnapshotDatabaseResult<Vec<SnapshotRow>> {
        let mut open_databases = self.lock_open_databases(SnapshotDatabaseErrorKind::RowLookup)?;
        let open_database = open_databases
            .get_mut(&database.local_path)
            .ok_or_else(|| Self::missing_open_database(&database.local_path))?;

        let mut statement = open_database
            .connection
            .prepare(
                "SELECT id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time
                 FROM snapshot
                 WHERE parent_id = ?1
                 ORDER BY basename, id",
            )
            .map_err(Self::lookup_error)?;
        let rows = statement
            .query_map([parent_id], read_row)
            .map_err(Self::lookup_error)?;
        let mut found = Vec::new();
        for row in rows {
            found.push(row.map_err(Self::lookup_error)?);
        }
        Ok(found)
    }
    fn confirm_present( &self, database: &SnapshotDatabaseHandle, facts: &SnapshotRowFacts, last_seen: &str, ) -> SnapshotDatabaseResult<()> {
        let mut open_databases = self.lock_open_databases(SnapshotDatabaseErrorKind::RowMutation)?;
        let open_database = open_databases
            .get_mut(&database.local_path)
            .ok_or_else(|| Self::missing_open_database(&database.local_path))?;
        self.snapshotrows
            .confirm_present(&mut open_database.connection, &to_rows_facts(facts), last_seen)
            .map_err(Self::row_error)
    }
    fn confirm_absent( &self, database: &SnapshotDatabaseHandle, identity: &SnapshotRowIdentity, ) -> SnapshotDatabaseResult<()> {
        let mut open_databases = self.lock_open_databases(SnapshotDatabaseErrorKind::RowMutation)?;
        let open_database = open_databases
            .get_mut(&database.local_path)
            .ok_or_else(|| Self::missing_open_database(&database.local_path))?;
        self.snapshotrows
            .confirm_absent(&mut open_database.connection, &to_rows_identity(identity))
            .map_err(Self::row_error)
    }
    fn record_intended_file_copy( &self, database: &SnapshotDatabaseHandle, facts: &SnapshotRowFacts, ) -> SnapshotDatabaseResult<()> {
        let mut open_databases = self.lock_open_databases(SnapshotDatabaseErrorKind::RowMutation)?;
        let open_database = open_databases
            .get_mut(&database.local_path)
            .ok_or_else(|| Self::missing_open_database(&database.local_path))?;
        self.snapshotrows
            .record_intended_file_copy(&mut open_database.connection, &to_rows_facts(facts))
            .map_err(Self::row_error)
    }
    fn complete_file_copy( &self, database: &SnapshotDatabaseHandle, identity: &SnapshotRowIdentity, last_seen: &str, ) -> SnapshotDatabaseResult<()> {
        let mut open_databases = self.lock_open_databases(SnapshotDatabaseErrorKind::RowMutation)?;
        let open_database = open_databases
            .get_mut(&database.local_path)
            .ok_or_else(|| Self::missing_open_database(&database.local_path))?;
        self.snapshotrows
            .complete_file_copy(&mut open_database.connection, &to_rows_identity(identity), last_seen)
            .map_err(Self::row_error)
    }
    fn complete_directory_creation( &self, database: &SnapshotDatabaseHandle, identity: &SnapshotRowIdentity, mod_time: &str, last_seen: &str, ) -> SnapshotDatabaseResult<()> {
        let mut open_databases = self.lock_open_databases(SnapshotDatabaseErrorKind::RowMutation)?;
        let open_database = open_databases
            .get_mut(&database.local_path)
            .ok_or_else(|| Self::missing_open_database(&database.local_path))?;
        self.snapshotrows
            .complete_directory_creation(
                &mut open_database.connection,
                &to_rows_identity(identity),
                mod_time,
                last_seen,
            )
            .map_err(Self::row_error)
    }
    fn complete_displacement( &self, database: &SnapshotDatabaseHandle, identity: &SnapshotRowIdentity, ) -> SnapshotDatabaseResult<()> {
        let mut open_databases = self.lock_open_databases(SnapshotDatabaseErrorKind::RowMutation)?;
        let open_database = open_databases
            .get_mut(&database.local_path)
            .ok_or_else(|| Self::missing_open_database(&database.local_path))?;
        self.snapshotrows
            .complete_displacement(&mut open_database.connection, &to_rows_identity(identity))
            .map_err(Self::row_error)
    }
    fn complete_directory_displacement_cascade( &self, database: &SnapshotDatabaseHandle, identity: &SnapshotRowIdentity, ) -> SnapshotDatabaseResult<()> {
        let mut open_databases = self.lock_open_databases(SnapshotDatabaseErrorKind::RowMutation)?;
        let open_database = open_databases
            .get_mut(&database.local_path)
            .ok_or_else(|| Self::missing_open_database(&database.local_path))?;
        self.snapshotrows
            .complete_directory_displacement_cascade(
                &mut open_database.connection,
                &to_rows_identity(identity),
            )
            .map_err(Self::row_error)
    }
    fn cleanup_old_rows( &self, database: &SnapshotDatabaseHandle, cutoff: &str, ) -> SnapshotDatabaseResult<()> {
        let mut open_databases = self.lock_open_databases(SnapshotDatabaseErrorKind::Cleanup)?;
        let open_database = open_databases
            .get_mut(&database.local_path)
            .ok_or_else(|| Self::missing_open_database(&database.local_path))?;
        self.snapshotcleanup
            .cleanup_snapshot(&mut open_database.connection, cutoff)
            .map_err(Self::cleanup_error)
    }
    fn prepare_for_upload( &self, database: SnapshotDatabaseHandle, ) -> SnapshotDatabaseResult<PathBuf> {
        let open_database = self
            .lock_open_databases(SnapshotDatabaseErrorKind::ConnectionClose)?
            .remove(&database.local_path)
            .ok_or_else(|| Self::missing_open_database(&database.local_path))?;
        self.snapshotfile
            .prepare_for_upload(open_database)
            .map(|prepared| prepared.local_snapshot_db_path)
            .map_err(Self::file_error)
    }
}

pub fn new(snapshotcleanup: std::sync::Arc<dyn snapshotstore_snapshotdatabase_snapshotcleanup::SnapshotCleanup>, snapshotfile: std::sync::Arc<dyn snapshotstore_snapshotdatabase_snapshotfile::SnapshotFile>, snapshotrows: std::sync::Arc<dyn snapshotstore_snapshotdatabase_snapshotrows::SnapshotRows>) -> std::sync::Arc<dyn SnapshotDatabase> {
    Arc::new(SnapshotDatabaseImpl {
        snapshotcleanup,
        snapshotfile,
        snapshotrows,
        open_databases: Mutex::new(HashMap::new()),
    })
}
