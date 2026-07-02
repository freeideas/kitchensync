use std::sync::Arc;

use rusqlite::{params, Connection, OptionalExtension};

use crate::api::*;

const SNAPSHOT_ROOT_PARENT_ID: &str = "JyBskcNRrBK";

struct SnapshotRowsImpl;

fn invalid_identity(message: &str) -> rusqlite::Error {
    rusqlite::Error::InvalidParameterName(message.to_string())
}

fn validate_identity(identity: &SnapshotRowIdentity) -> rusqlite::Result<()> {
    if identity.id.is_empty() {
        return Err(invalid_identity("snapshot row id is empty"));
    }
    if identity.id == SNAPSHOT_ROOT_PARENT_ID {
        return Err(invalid_identity("snapshot row id is the sync root"));
    }
    if identity.parent_id.is_empty() {
        return Err(invalid_identity("snapshot row parent id is empty"));
    }
    if identity.basename.is_empty() {
        return Err(invalid_identity("snapshot row basename is empty"));
    }
    if identity.basename == "."
        || identity.basename == ".."
        || identity.basename.contains('/')
        || identity.basename.contains('\\')
    {
        return Err(invalid_identity(
            "snapshot row basename is not a final path component",
        ));
    }
    Ok(())
}

impl SnapshotRows for SnapshotRowsImpl {
    fn confirm_present(
        &self,
        database: &mut Connection,
        facts: &SnapshotRowFacts,
        last_seen: &str,
    ) -> rusqlite::Result<()> {
        validate_identity(&facts.identity)?;

        let transaction = database.transaction()?;
        transaction.execute(
            "INSERT INTO snapshot (
                id,
                parent_id,
                basename,
                mod_time,
                byte_size,
                last_seen,
                deleted_time
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)
            ON CONFLICT(id) DO UPDATE SET
                parent_id = excluded.parent_id,
                basename = excluded.basename,
                mod_time = excluded.mod_time,
                byte_size = excluded.byte_size,
                last_seen = excluded.last_seen,
                deleted_time = NULL",
            params![
                facts.identity.id.as_str(),
                facts.identity.parent_id.as_str(),
                facts.identity.basename.as_str(),
                facts.mod_time.as_str(),
                facts.byte_size,
                last_seen
            ],
        )?;
        transaction.commit()
    }

    fn confirm_absent(
        &self,
        database: &mut Connection,
        identity: &SnapshotRowIdentity,
    ) -> rusqlite::Result<()> {
        validate_identity(identity)?;

        let transaction = database.transaction()?;
        transaction.execute(
            "UPDATE snapshot
            SET deleted_time = last_seen
            WHERE id = ?1
            AND deleted_time IS NULL",
            params![identity.id.as_str()],
        )?;
        transaction.commit()
    }

    fn record_intended_file_copy(
        &self,
        database: &mut Connection,
        facts: &SnapshotRowFacts,
    ) -> rusqlite::Result<()> {
        validate_identity(&facts.identity)?;

        let transaction = database.transaction()?;
        transaction.execute(
            "INSERT INTO snapshot (
                id,
                parent_id,
                basename,
                mod_time,
                byte_size,
                last_seen,
                deleted_time
            )
            VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL)
            ON CONFLICT(id) DO UPDATE SET
                parent_id = excluded.parent_id,
                basename = excluded.basename,
                mod_time = excluded.mod_time,
                byte_size = excluded.byte_size,
                deleted_time = NULL",
            params![
                facts.identity.id.as_str(),
                facts.identity.parent_id.as_str(),
                facts.identity.basename.as_str(),
                facts.mod_time.as_str(),
                facts.byte_size
            ],
        )?;
        transaction.commit()
    }

    fn complete_file_copy(
        &self,
        database: &mut Connection,
        identity: &SnapshotRowIdentity,
        last_seen: &str,
    ) -> rusqlite::Result<()> {
        validate_identity(identity)?;

        let transaction = database.transaction()?;
        let changed = transaction.execute(
            "UPDATE snapshot
            SET last_seen = ?2
            WHERE id = ?1",
            params![identity.id.as_str(), last_seen],
        )?;
        if changed == 0 {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }
        transaction.commit()
    }

    fn complete_directory_creation(
        &self,
        database: &mut Connection,
        identity: &SnapshotRowIdentity,
        mod_time: &str,
        last_seen: &str,
    ) -> rusqlite::Result<()> {
        validate_identity(identity)?;

        let transaction = database.transaction()?;
        transaction.execute(
            "INSERT INTO snapshot (
                id,
                parent_id,
                basename,
                mod_time,
                byte_size,
                last_seen,
                deleted_time
            )
            VALUES (?1, ?2, ?3, ?4, -1, ?5, NULL)
            ON CONFLICT(id) DO UPDATE SET
                parent_id = excluded.parent_id,
                basename = excluded.basename,
                mod_time = excluded.mod_time,
                byte_size = -1,
                last_seen = excluded.last_seen,
                deleted_time = NULL",
            params![
                identity.id.as_str(),
                identity.parent_id.as_str(),
                identity.basename.as_str(),
                mod_time,
                last_seen
            ],
        )?;
        transaction.commit()
    }

    fn complete_displacement(
        &self,
        database: &mut Connection,
        identity: &SnapshotRowIdentity,
    ) -> rusqlite::Result<()> {
        validate_identity(identity)?;

        let transaction = database.transaction()?;
        let changed = transaction.execute(
            "UPDATE snapshot
            SET deleted_time = last_seen
            WHERE id = ?1",
            params![identity.id.as_str()],
        )?;
        if changed == 0 {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }
        transaction.commit()
    }

    fn complete_directory_displacement_cascade(
        &self,
        database: &mut Connection,
        identity: &SnapshotRowIdentity,
    ) -> rusqlite::Result<()> {
        validate_identity(identity)?;

        let transaction = database.transaction()?;
        let deleted_time = transaction
            .query_row(
                "SELECT last_seen
                FROM snapshot
                WHERE id = ?1",
                params![identity.id.as_str()],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .ok_or(rusqlite::Error::QueryReturnedNoRows)?;

        transaction.execute(
            "WITH RECURSIVE subtree(id) AS (
                VALUES(?1)

                UNION ALL

                SELECT child.id
                FROM snapshot child
                JOIN subtree parent ON child.parent_id = parent.id
                WHERE child.deleted_time IS NULL
            )
            UPDATE snapshot
            SET deleted_time = ?2
            WHERE deleted_time IS NULL
            AND id IN (SELECT id FROM subtree)",
            params![identity.id.as_str(), deleted_time],
        )?;
        transaction.commit()
    }
}

pub fn new() -> std::sync::Arc<dyn SnapshotRows> {
    Arc::new(SnapshotRowsImpl)
}
