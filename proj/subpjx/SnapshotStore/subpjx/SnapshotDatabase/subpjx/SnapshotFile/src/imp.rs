use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rusqlite::{Connection, OpenFlags};

use crate::api::*;

struct SnapshotFileImpl;

#[derive(Debug, PartialEq, Eq)]
struct ColumnShape {
    name: String,
    sqlite_type: String,
    not_null: bool,
    primary_key: bool,
}

impl SnapshotFileImpl {
    fn error(
        local_snapshot_db_path: &Path,
        reason: SnapshotFileErrorReason,
        detail: impl Into<String>,
    ) -> SnapshotFileError {
        SnapshotFileError {
            local_snapshot_db_path: local_snapshot_db_path.to_path_buf(),
            reason,
            detail: detail.into(),
        }
    }

    fn setup_rollback_journal(
        local_snapshot_db_path: &Path,
        connection: &Connection,
    ) -> Result<(), SnapshotFileError> {
        let mode: String = connection
            .query_row("PRAGMA journal_mode=DELETE", [], |row| row.get(0))
            .map_err(|error| {
                Self::error(
                    local_snapshot_db_path,
                    SnapshotFileErrorReason::RollbackJournalSetup,
                    error.to_string(),
                )
            })?;

        if mode.eq_ignore_ascii_case("delete") {
            Ok(())
        } else {
            Err(Self::error(
                local_snapshot_db_path,
                SnapshotFileErrorReason::RollbackJournalSetup,
                format!("SQLite journal_mode remained {mode}"),
            ))
        }
    }

    fn create_schema(
        local_snapshot_db_path: &Path,
        connection: &Connection,
    ) -> Result<(), SnapshotFileError> {
        connection
            .execute_batch(
                "CREATE TABLE snapshot (
                    id TEXT PRIMARY KEY,
                    parent_id TEXT,
                    basename TEXT NOT NULL,
                    mod_time TEXT NOT NULL,
                    byte_size INTEGER NOT NULL,
                    last_seen TEXT,
                    deleted_time TEXT
                );
                CREATE INDEX snapshot_parent_id ON snapshot(parent_id);
                CREATE INDEX snapshot_last_seen ON snapshot(last_seen);
                CREATE INDEX snapshot_deleted_time ON snapshot(deleted_time);",
            )
            .map_err(|error| {
                Self::error(
                    local_snapshot_db_path,
                    SnapshotFileErrorReason::SchemaCreation,
                    error.to_string(),
                )
            })
    }

    fn validate_tables_and_views(
        local_snapshot_db_path: &Path,
        connection: &Connection,
    ) -> Result<(), SnapshotFileError> {
        let tables = Self::schema_names(
            connection,
            "table",
            SnapshotFileErrorReason::SchemaValidation,
            local_snapshot_db_path,
        )?;
        if tables != vec!["snapshot".to_string()] {
            return Err(Self::error(
                local_snapshot_db_path,
                SnapshotFileErrorReason::SchemaValidation,
                format!("expected only table snapshot, found {}", tables.join(", ")),
            ));
        }

        let views = Self::schema_names(
            connection,
            "view",
            SnapshotFileErrorReason::SchemaValidation,
            local_snapshot_db_path,
        )?;
        if !views.is_empty() {
            return Err(Self::error(
                local_snapshot_db_path,
                SnapshotFileErrorReason::SchemaValidation,
                format!("expected no views, found {}", views.join(", ")),
            ));
        }

        Ok(())
    }

    fn schema_names(
        connection: &Connection,
        schema_type: &str,
        reason: SnapshotFileErrorReason,
        local_snapshot_db_path: &Path,
    ) -> Result<Vec<String>, SnapshotFileError> {
        let mut statement = connection
            .prepare(
                "SELECT name
                 FROM sqlite_schema
                 WHERE type = ?1
                 ORDER BY name",
            )
            .map_err(|error| Self::error(local_snapshot_db_path, reason, error.to_string()))?;
        let rows = statement
            .query_map([schema_type], |row| row.get::<_, String>(0))
            .map_err(|error| Self::error(local_snapshot_db_path, reason, error.to_string()))?;

        let mut names = Vec::new();
        for row in rows {
            names.push(row.map_err(|error| {
                Self::error(local_snapshot_db_path, reason, error.to_string())
            })?);
        }
        Ok(names)
    }

    fn validate_columns(
        local_snapshot_db_path: &Path,
        connection: &Connection,
    ) -> Result<(), SnapshotFileError> {
        let mut statement = connection
            .prepare("PRAGMA table_info(snapshot)")
            .map_err(|error| {
                Self::error(
                    local_snapshot_db_path,
                    SnapshotFileErrorReason::SchemaValidation,
                    error.to_string(),
                )
            })?;
        let rows = statement
            .query_map([], |row| {
                let sqlite_type: String = row.get(2)?;
                let not_null: i32 = row.get(3)?;
                let primary_key: i32 = row.get(5)?;
                Ok(ColumnShape {
                    name: row.get(1)?,
                    sqlite_type: sqlite_type.to_ascii_uppercase(),
                    not_null: not_null != 0,
                    primary_key: primary_key != 0,
                })
            })
            .map_err(|error| {
                Self::error(
                    local_snapshot_db_path,
                    SnapshotFileErrorReason::SchemaValidation,
                    error.to_string(),
                )
            })?;

        let mut columns = Vec::new();
        for row in rows {
            columns.push(row.map_err(|error| {
                Self::error(
                    local_snapshot_db_path,
                    SnapshotFileErrorReason::SchemaValidation,
                    error.to_string(),
                )
            })?);
        }

        let expected = vec![
            ColumnShape {
                name: "id".to_string(),
                sqlite_type: "TEXT".to_string(),
                not_null: false,
                primary_key: true,
            },
            ColumnShape {
                name: "parent_id".to_string(),
                sqlite_type: "TEXT".to_string(),
                not_null: false,
                primary_key: false,
            },
            ColumnShape {
                name: "basename".to_string(),
                sqlite_type: "TEXT".to_string(),
                not_null: true,
                primary_key: false,
            },
            ColumnShape {
                name: "mod_time".to_string(),
                sqlite_type: "TEXT".to_string(),
                not_null: true,
                primary_key: false,
            },
            ColumnShape {
                name: "byte_size".to_string(),
                sqlite_type: "INTEGER".to_string(),
                not_null: true,
                primary_key: false,
            },
            ColumnShape {
                name: "last_seen".to_string(),
                sqlite_type: "TEXT".to_string(),
                not_null: false,
                primary_key: false,
            },
            ColumnShape {
                name: "deleted_time".to_string(),
                sqlite_type: "TEXT".to_string(),
                not_null: false,
                primary_key: false,
            },
        ];

        if columns == expected {
            Ok(())
        } else {
            Err(Self::error(
                local_snapshot_db_path,
                SnapshotFileErrorReason::SchemaValidation,
                "snapshot table columns do not match the required schema",
            ))
        }
    }

    fn validate_indexes(
        local_snapshot_db_path: &Path,
        connection: &Connection,
    ) -> Result<(), SnapshotFileError> {
        let mut statement = connection
            .prepare("PRAGMA index_list(snapshot)")
            .map_err(|error| {
                Self::error(
                    local_snapshot_db_path,
                    SnapshotFileErrorReason::SchemaValidation,
                    error.to_string(),
                )
            })?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(|error| {
                Self::error(
                    local_snapshot_db_path,
                    SnapshotFileErrorReason::SchemaValidation,
                    error.to_string(),
                )
            })?;

        let mut single_column_indexes = BTreeSet::new();
        for row in rows {
            let index_name = row.map_err(|error| {
                Self::error(
                    local_snapshot_db_path,
                    SnapshotFileErrorReason::SchemaValidation,
                    error.to_string(),
                )
            })?;
            let columns = Self::index_columns(local_snapshot_db_path, connection, &index_name)?;
            if let [column_name] = columns.as_slice() {
                single_column_indexes.insert(column_name.clone());
            }
        }

        for required_column in ["parent_id", "last_seen", "deleted_time"] {
            if !single_column_indexes.contains(required_column) {
                return Err(Self::error(
                    local_snapshot_db_path,
                    SnapshotFileErrorReason::SchemaValidation,
                    format!("missing index covering {required_column}"),
                ));
            }
        }

        Ok(())
    }

    fn index_columns(
        local_snapshot_db_path: &Path,
        connection: &Connection,
        index_name: &str,
    ) -> Result<Vec<String>, SnapshotFileError> {
        let mut statement = connection
            .prepare(&format!("PRAGMA index_info({})", quote_identifier(index_name)))
            .map_err(|error| {
                Self::error(
                    local_snapshot_db_path,
                    SnapshotFileErrorReason::SchemaValidation,
                    error.to_string(),
                )
            })?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(2))
            .map_err(|error| {
                Self::error(
                    local_snapshot_db_path,
                    SnapshotFileErrorReason::SchemaValidation,
                    error.to_string(),
                )
            })?;

        let mut columns = Vec::new();
        for row in rows {
            columns.push(row.map_err(|error| {
                Self::error(
                    local_snapshot_db_path,
                    SnapshotFileErrorReason::SchemaValidation,
                    error.to_string(),
                )
            })?);
        }
        Ok(columns)
    }
}

fn quote_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

impl SnapshotFile for SnapshotFileImpl {
    fn create_new_snapshot_database(
        &self,
        local_snapshot_db_path: PathBuf,
    ) -> Result<SnapshotFileOpenDatabase, SnapshotFileError> {
        let connection = Connection::open(&local_snapshot_db_path).map_err(|error| {
            SnapshotFileImpl::error(
                &local_snapshot_db_path,
                SnapshotFileErrorReason::SqliteOpen,
                error.to_string(),
            )
        })?;

        SnapshotFileImpl::setup_rollback_journal(&local_snapshot_db_path, &connection)?;
        SnapshotFileImpl::create_schema(&local_snapshot_db_path, &connection)?;
        self.validate_snapshot_schema(&local_snapshot_db_path, &connection)?;

        Ok(SnapshotFileOpenDatabase {
            local_snapshot_db_path,
            connection,
        })
    }

    fn open_existing_snapshot_database(
        &self,
        local_snapshot_db_path: PathBuf,
    ) -> Result<SnapshotFileOpenDatabase, SnapshotFileError> {
        let connection = Connection::open_with_flags(
            &local_snapshot_db_path,
            OpenFlags::SQLITE_OPEN_READ_WRITE,
        )
        .map_err(|error| {
            SnapshotFileImpl::error(
                &local_snapshot_db_path,
                SnapshotFileErrorReason::SqliteOpen,
                error.to_string(),
            )
        })?;

        SnapshotFileImpl::setup_rollback_journal(&local_snapshot_db_path, &connection)?;
        self.validate_snapshot_schema(&local_snapshot_db_path, &connection)?;

        Ok(SnapshotFileOpenDatabase {
            local_snapshot_db_path,
            connection,
        })
    }

    fn validate_snapshot_schema(
        &self,
        local_snapshot_db_path: &Path,
        connection: &Connection,
    ) -> Result<(), SnapshotFileError> {
        SnapshotFileImpl::validate_tables_and_views(local_snapshot_db_path, connection)?;
        SnapshotFileImpl::validate_columns(local_snapshot_db_path, connection)?;
        SnapshotFileImpl::validate_indexes(local_snapshot_db_path, connection)?;
        Ok(())
    }

    fn prepare_for_upload(
        &self,
        database: SnapshotFileOpenDatabase,
    ) -> Result<SnapshotFilePreparedForUpload, SnapshotFileError> {
        self.validate_snapshot_schema(&database.local_snapshot_db_path, &database.connection)?;
        SnapshotFileImpl::setup_rollback_journal(
            &database.local_snapshot_db_path,
            &database.connection,
        )?;

        let local_snapshot_db_path = database.local_snapshot_db_path;
        database.connection.close().map_err(|(_, error)| {
            SnapshotFileImpl::error(
                &local_snapshot_db_path,
                SnapshotFileErrorReason::ConnectionClose,
                error.to_string(),
            )
        })?;

        Ok(SnapshotFilePreparedForUpload {
            local_snapshot_db_path,
        })
    }
}

pub fn new() -> std::sync::Arc<dyn SnapshotFile> {
    Arc::new(SnapshotFileImpl)
}
