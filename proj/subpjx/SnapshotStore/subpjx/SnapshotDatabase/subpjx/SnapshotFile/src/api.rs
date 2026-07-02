use std::path::{Path, PathBuf};

use rusqlite::Connection;

pub struct SnapshotFileOpenDatabase {
    pub local_snapshot_db_path: PathBuf,
    pub connection: Connection,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotFilePreparedForUpload {
    pub local_snapshot_db_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotFileError {
    pub local_snapshot_db_path: PathBuf,
    pub reason: SnapshotFileErrorReason,
    pub detail: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotFileErrorReason {
    SqliteOpen,
    RollbackJournalSetup,
    SchemaCreation,
    SchemaValidation,
    TransactionFinish,
    StatementFinalization,
    ReaderOrCursorFinish,
    ConnectionClose,
    OwnedResourceStillOpen,
}

pub trait SnapshotFile: Send + Sync {
    /// Creates a new empty local temporary `snapshot.db` file and returns its
    /// open SQLite connection.
    ///
    /// The supplied path is the caller's local temporary snapshot file for one
    /// peer. This operation must open that exact path, force SQLite
    /// rollback-journal behavior instead of WAL behavior, create exactly the
    /// required `snapshot` schema, validate the created schema before
    /// returning, and keep all later reads and writes on the returned
    /// connection pointed at that local temporary file. It must not download,
    /// upload, recover, rename, delete, stage, or redirect to any peer-side
    /// `.kitchensync/snapshot.db` path.
    ///
    /// Success returns only after the database contains exactly one table
    /// named `snapshot`, no views, the seven required columns with their exact
    /// SQLite types, primary-key and nullability rules, and indexes covering
    /// `parent_id`, `last_seen`, and `deleted_time`. Failure reports SQLite
    /// open, rollback-journal setup, schema creation, or schema validation
    /// errors and must not report success for schema drift or for a database
    /// that cannot be forced to rollback-journal behavior.
    fn create_new_snapshot_database(
        &self,
        local_snapshot_db_path: PathBuf,
    ) -> Result<SnapshotFileOpenDatabase, SnapshotFileError>;

    /// Opens an existing local temporary `snapshot.db` file and returns its
    /// open SQLite connection.
    ///
    /// The supplied path is the caller's local temporary snapshot file for one
    /// peer. This operation must open that exact file, force SQLite
    /// rollback-journal behavior instead of WAL behavior, validate the schema
    /// before returning, and keep all later reads and writes on the returned
    /// connection pointed at that local temporary file. It must not download,
    /// upload, recover, rename, delete, stage, or redirect to any peer-side
    /// `.kitchensync/snapshot.db` path.
    ///
    /// Success requires the existing file to already satisfy the required
    /// schema exactly. The operation must reject extra application tables,
    /// alternate table names, views, missing columns, extra columns, wrong
    /// column types, wrong nullability, a missing primary key on `id`, or
    /// missing required indexes. Failure reports SQLite open,
    /// rollback-journal setup, or schema validation errors.
    fn open_existing_snapshot_database(
        &self,
        local_snapshot_db_path: PathBuf,
    ) -> Result<SnapshotFileOpenDatabase, SnapshotFileError>;

    /// Validates the schema of an open local temporary snapshot database.
    ///
    /// Validation is scoped to the supplied open connection and path. It must
    /// not create, modify, adapt, or extend the database. It succeeds only
    /// when the database contains exactly one table named `snapshot`, contains
    /// no views, and has exactly these columns: `id` as `TEXT` and primary
    /// key, `parent_id` as `TEXT`, `basename` as `TEXT NOT NULL`, `mod_time`
    /// as `TEXT NOT NULL`, `byte_size` as `INTEGER NOT NULL`, `last_seen` as
    /// nullable `TEXT`, and `deleted_time` as nullable `TEXT`.
    ///
    /// The `snapshot` table must have indexes that cover `parent_id`,
    /// `last_seen`, and `deleted_time`. Validation must reject schema drift
    /// instead of adapting to it, including extra application tables,
    /// alternate table names, views, missing columns, extra columns, wrong
    /// column types, wrong nullability, a missing primary key on `id`, or
    /// missing required indexes.
    fn validate_snapshot_schema(
        &self,
        local_snapshot_db_path: &Path,
        connection: &Connection,
    ) -> Result<(), SnapshotFileError>;

    /// Closes an open local temporary snapshot database so transport can
    /// upload the filesystem file.
    ///
    /// Before success, this operation must commit or roll back every
    /// transaction SnapshotFile owns for the local file, finalize every
    /// statement, cursor, and reader SnapshotFile owns for the local file, and
    /// close every SQLite connection SnapshotFile owns for the local file.
    /// After success, upload reads the closed `snapshot.db` file from the
    /// filesystem, not from a live SQLite connection, and the file is a
    /// self-contained SQLite database that does not require SQLite sidecar
    /// files for later use.
    ///
    /// The operation must report transaction-finish, statement-finalization,
    /// reader-or-cursor-finish, connection-close, and remaining-owned-resource
    /// errors. It must not report success while any owned SQLite resource for
    /// the file remains open. SnapshotFile does not own SQLite resources
    /// created wholly by a caller outside its boundary; callers must finish
    /// those resources before requesting upload preparation, or this operation
    /// must report that the file cannot be prepared.
    fn prepare_for_upload(
        &self,
        database: SnapshotFileOpenDatabase,
    ) -> Result<SnapshotFilePreparedForUpload, SnapshotFileError>;
}
