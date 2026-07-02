use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;
use snapshotstore_snapshotdatabase_snapshotfile::{
    new, SnapshotFile, SnapshotFileErrorReason,
};

#[derive(Debug, PartialEq, Eq)]
struct ColumnShape {
    name: String,
    sqlite_type: String,
    not_null: bool,
    primary_key: bool,
}

#[test]
fn created_database_has_the_required_snapshot_schema_on_the_local_file() {
    let subject: std::sync::Arc<dyn SnapshotFile> = new();
    let db_path = fresh_snapshot_path("created-schema");

    let database = subject
        .create_new_snapshot_database(db_path.clone())
        .expect("created snapshot database");

    assert_eq!(database.local_snapshot_db_path, db_path);
    assert_eq!(journal_mode(&database.connection), "delete");
    assert_eq!(
        schema_names(&database.connection, "table"),
        vec!["snapshot".to_string()]
    );
    assert!(schema_names(&database.connection, "view").is_empty());
    assert_eq!(columns(&database.connection), required_columns());
    let indexed_columns = indexed_single_columns(&database.connection);
    assert!(indexed_columns.contains("parent_id"));
    assert!(indexed_columns.contains("last_seen"));
    assert!(indexed_columns.contains("deleted_time"));

    subject
        .prepare_for_upload(database)
        .expect("prepared created database for upload");
}

#[test]
fn reads_and_writes_stay_on_the_caller_supplied_local_snapshot_file() {
    let subject: std::sync::Arc<dyn SnapshotFile> = new();
    let db_path = fresh_snapshot_path("local-file");

    let database = subject
        .create_new_snapshot_database(db_path.clone())
        .expect("created snapshot database");

    database
        .connection
        .execute(
            "INSERT INTO snapshot (
                id,
                parent_id,
                basename,
                mod_time,
                byte_size,
                last_seen,
                deleted_time
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            (
                "child-id",
                "parent-id",
                "file.txt",
                "2026-07-02T12:00:00Z",
                42_i64,
                Option::<String>::None,
                Option::<String>::None,
            ),
        )
        .expect("inserted snapshot row through returned handle");

    let prepared = subject
        .prepare_for_upload(database)
        .expect("prepared database for upload");
    assert_eq!(prepared.local_snapshot_db_path, db_path);

    assert!(!db_path.with_extension("db-wal").exists());
    assert!(!db_path.with_extension("db-shm").exists());
    assert!(!db_path.with_extension("db-journal").exists());

    let reopened = subject
        .open_existing_snapshot_database(db_path.clone())
        .expect("opened prepared local database");
    assert_eq!(reopened.local_snapshot_db_path, db_path);
    assert_eq!(journal_mode(&reopened.connection), "delete");

    let row: (String, String, i64) = reopened
        .connection
        .query_row(
            "SELECT parent_id, basename, byte_size FROM snapshot WHERE id = ?1",
            ["child-id"],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("read inserted row from reopened local file");
    assert_eq!(row, ("parent-id".to_string(), "file.txt".to_string(), 42));

    subject
        .prepare_for_upload(reopened)
        .expect("prepared reopened database for upload");
}

#[test]
fn open_existing_rejects_schema_drift_in_the_local_snapshot_file() {
    let subject: std::sync::Arc<dyn SnapshotFile> = new();
    let db_path = fresh_snapshot_path("schema-drift");

    {
        let connection = Connection::open(&db_path).expect("opened drifted fixture database");
        connection
            .execute_batch(
                "PRAGMA journal_mode=DELETE;
                CREATE TABLE snapshot (
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
                CREATE VIEW snapshot_names AS SELECT basename FROM snapshot;",
            )
            .expect("created drifted fixture database");
    }

    let error = match subject.open_existing_snapshot_database(db_path.clone()) {
        Ok(_) => panic!("schema drift must be rejected"),
        Err(error) => error,
    };

    assert_eq!(error.local_snapshot_db_path, db_path);
    assert_eq!(error.reason, SnapshotFileErrorReason::SchemaValidation);
}

fn fresh_snapshot_path(test_name: &str) -> PathBuf {
    let mut directory = std::env::temp_dir();
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time is after Unix epoch")
        .as_nanos();
    directory.push(format!(
        "kitchensync-snapshotfile-{test_name}-{}-{unique}",
        std::process::id()
    ));
    fs::remove_dir_all(&directory).ok();
    fs::create_dir_all(&directory).expect("created test directory");
    directory.join("snapshot.db")
}

fn journal_mode(connection: &Connection) -> String {
    connection
        .query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))
        .expect("read journal mode")
        .to_ascii_lowercase()
}

fn schema_names(connection: &Connection, schema_type: &str) -> Vec<String> {
    let mut statement = connection
        .prepare(
            "SELECT name
             FROM sqlite_schema
             WHERE type = ?1
             ORDER BY name",
        )
        .expect("prepared schema-name query");
    statement
        .query_map([schema_type], |row| row.get::<_, String>(0))
        .expect("queried schema names")
        .map(|row| row.expect("read schema name"))
        .collect()
}

fn columns(connection: &Connection) -> Vec<ColumnShape> {
    let mut statement = connection
        .prepare("PRAGMA table_info(snapshot)")
        .expect("prepared column query");
    statement
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
        .expect("queried columns")
        .map(|row| row.expect("read column"))
        .collect()
}

fn required_columns() -> Vec<ColumnShape> {
    vec![
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
    ]
}

fn indexed_single_columns(connection: &Connection) -> BTreeSet<String> {
    index_column_map(connection)
        .into_values()
        .filter_map(|columns| match columns.as_slice() {
            [only_column] => Some(only_column.clone()),
            _ => None,
        })
        .collect()
}

fn index_column_map(connection: &Connection) -> BTreeMap<String, Vec<String>> {
    let mut statement = connection
        .prepare("PRAGMA index_list(snapshot)")
        .expect("prepared index-list query");
    let index_names: Vec<String> = statement
        .query_map([], |row| row.get::<_, String>(1))
        .expect("queried indexes")
        .map(|row| row.expect("read index name"))
        .collect();

    index_names
        .into_iter()
        .map(|index_name| {
            let columns = index_columns(connection, &index_name);
            (index_name, columns)
        })
        .collect()
}

fn index_columns(connection: &Connection, index_name: &str) -> Vec<String> {
    let mut statement = connection
        .prepare(&format!("PRAGMA index_info({})", quote_identifier(index_name)))
        .expect("prepared index-info query");
    statement
        .query_map([], |row| row.get::<_, String>(2))
        .expect("queried index columns")
        .map(|row| row.expect("read index column"))
        .collect()
}

fn quote_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}
