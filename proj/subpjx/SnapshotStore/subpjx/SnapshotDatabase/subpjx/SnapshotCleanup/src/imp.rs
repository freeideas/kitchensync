use std::sync::Arc;

use rusqlite::{params, Connection};

use crate::api::*;

const SNAPSHOT_ROOT_PARENT_ID: &str = "JyBskcNRrBK";

struct SnapshotCleanupImpl;

impl SnapshotCleanup for SnapshotCleanupImpl {
    fn cleanup_snapshot(
        &self,
        database: &mut Connection,
        cutoff_timestamp: &str,
    ) -> rusqlite::Result<()> {
        let transaction = database.transaction()?;

        transaction.execute(
            "DELETE FROM snapshot
            WHERE deleted_time IS NOT NULL
            AND deleted_time < ?1",
            params![cutoff_timestamp],
        )?;

        transaction.execute(
            "WITH RECURSIVE reachable(id) AS (
                SELECT id
                FROM snapshot
                WHERE deleted_time IS NULL
                AND parent_id = ?2

                UNION ALL

                SELECT child.id
                FROM snapshot child
                JOIN reachable parent ON child.parent_id = parent.id
                WHERE child.deleted_time IS NULL
            )
            DELETE FROM snapshot
            WHERE deleted_time IS NULL
            AND last_seen IS NOT NULL
            AND last_seen < ?1
            AND id NOT IN (SELECT id FROM reachable)",
            params![cutoff_timestamp, SNAPSHOT_ROOT_PARENT_ID],
        )?;

        transaction.commit()
    }
}

pub fn new() -> std::sync::Arc<dyn SnapshotCleanup> {
    Arc::new(SnapshotCleanupImpl)
}
