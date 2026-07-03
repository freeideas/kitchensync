use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use peertransportsurface::{PeerReadChunk, PeerTransportError};
use rusqlite::{params, Connection, OptionalExtension};

use crate::api::*;

const LIVE_SNAPSHOT_PATH: &str = ".kitchensync/snapshot.db";
const COPY_CHUNK_BYTES: usize = 64 * 1024;

struct SnapshotDatabaseImpl {
    formatrules: std::sync::Arc<dyn formatrules::FormatRules>,
    peertransportsurface: std::sync::Arc<dyn peertransportsurface::PeerTransportSurface>,
}

impl SnapshotDatabaseImpl {
    fn diagnostic(
        &self,
        peer_index: usize,
        kind: SnapshotDatabaseDiagnosticKind,
    ) -> SnapshotDatabaseDiagnostic {
        SnapshotDatabaseDiagnostic {
            level: SnapshotDatabaseDiagnosticLevel::Error,
            peer_index,
            kind,
        }
    }

    fn local_file_error(&self) -> SnapshotDatabaseError {
        SnapshotDatabaseError {
            kind: SnapshotDatabaseErrorKind::LocalFileError,
        }
    }

    fn local_database_error(&self) -> SnapshotDatabaseError {
        SnapshotDatabaseError {
            kind: SnapshotDatabaseErrorKind::LocalDatabaseError,
        }
    }

    fn peer_transport_error(&self) -> SnapshotDatabaseError {
        SnapshotDatabaseError {
            kind: SnapshotDatabaseErrorKind::PeerTransportError,
        }
    }

    fn open_database(&self, path: &Path) -> Result<Connection, SnapshotDatabaseError> {
        let conn = Connection::open(path).map_err(|_| self.local_database_error())?;
        self.set_rollback_journal(&conn)?;
        Ok(conn)
    }

    fn set_rollback_journal(&self, conn: &Connection) -> Result<(), SnapshotDatabaseError> {
        let mode: String = conn
            .query_row("PRAGMA journal_mode=DELETE", [], |row| row.get(0))
            .map_err(|_| self.local_database_error())?;
        if mode.eq_ignore_ascii_case("delete") {
            Ok(())
        } else {
            Err(self.local_database_error())
        }
    }

    fn create_database_at(&self, path: &Path) -> Result<(), SnapshotDatabaseError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|_| self.local_file_error())?;
        }
        if path.exists() {
            fs::remove_file(path).map_err(|_| self.local_file_error())?;
        }

        let conn = self.open_database(path)?;
        conn.execute_batch(
            "
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
            CREATE INDEX snapshot_deleted_time ON snapshot(deleted_time);
            ",
        )
        .map_err(|_| self.local_database_error())?;
        Ok(())
    }

    fn peer_path_exists(
        &self,
        peer: &peertransportsurface::ConnectedPeerRoot,
        path: &str,
    ) -> Result<bool, PeerTransportError> {
        match self.peertransportsurface.stat(peer, path) {
            Ok(_) => Ok(true),
            Err(PeerTransportError::NotFound) => Ok(false),
            Err(err) => Err(err),
        }
    }

    fn delete_peer_file_if_present(
        &self,
        peer: &peertransportsurface::ConnectedPeerRoot,
        path: &str,
    ) -> Result<(), PeerTransportError> {
        match self.peertransportsurface.delete_file(peer, path) {
            Ok(()) | Err(PeerTransportError::NotFound) => Ok(()),
            Err(err) => Err(err),
        }
    }

    fn recover_snapshot_swap(
        &self,
        peer: &peertransportsurface::ConnectedPeerRoot,
    ) -> Result<(), PeerTransportError> {
        let paths = self.formatrules.snapshot_swap_paths();
        let old_exists = self.peer_path_exists(peer, &paths.old_path)?;
        let new_exists = self.peer_path_exists(peer, &paths.new_path)?;
        let live_exists = self.peer_path_exists(peer, LIVE_SNAPSHOT_PATH)?;

        match (old_exists, new_exists, live_exists) {
            (true, _, true) => {
                if new_exists {
                    self.delete_peer_file_if_present(peer, &paths.new_path)?;
                }
                self.peertransportsurface.delete_file(peer, &paths.old_path)
            }
            (true, true, false) => {
                self.peertransportsurface
                    .rename(peer, &paths.new_path, LIVE_SNAPSHOT_PATH)?;
                self.peertransportsurface.delete_file(peer, &paths.old_path)
            }
            (true, false, false) => {
                self.peertransportsurface
                    .rename(peer, &paths.old_path, LIVE_SNAPSHOT_PATH)
            }
            (false, true, true) => self.peertransportsurface.delete_file(peer, &paths.new_path),
            (false, true, false) => {
                self.peertransportsurface
                    .rename(peer, &paths.new_path, LIVE_SNAPSHOT_PATH)
            }
            (false, false, _) => Ok(()),
        }
    }

    fn download_peer_snapshot(
        &self,
        peer: &peertransportsurface::ConnectedPeerRoot,
        local_path: &Path,
    ) -> Result<bool, SnapshotDatabaseError> {
        match self.peertransportsurface.stat(peer, LIVE_SNAPSHOT_PATH) {
            Ok(metadata) => {
                if metadata.is_dir {
                    return Err(self.peer_transport_error());
                }
            }
            Err(PeerTransportError::NotFound) => return Ok(false),
            Err(_) => return Err(self.peer_transport_error()),
        }

        if let Some(parent) = local_path.parent() {
            fs::create_dir_all(parent).map_err(|_| self.local_file_error())?;
        }

        let mut read_handle = match self.peertransportsurface.open_read(peer, LIVE_SNAPSHOT_PATH) {
            Ok(handle) => handle,
            Err(_) => return Err(self.peer_transport_error()),
        };
        let mut local_file = fs::File::create(local_path).map_err(|_| self.local_file_error())?;

        loop {
            match self.peertransportsurface.read(&mut read_handle, COPY_CHUNK_BYTES) {
                Ok(PeerReadChunk::Bytes(bytes)) => {
                    local_file
                        .write_all(&bytes)
                        .map_err(|_| self.local_file_error())?;
                }
                Ok(PeerReadChunk::Eof) => break,
                Err(_) => {
                    let _ = self.peertransportsurface.close_read(read_handle);
                    return Err(self.peer_transport_error());
                }
            }
        }

        self.peertransportsurface
            .close_read(read_handle)
            .map_err(|_| self.peer_transport_error())?;
        Ok(true)
    }

    fn upload_local_file(
        &self,
        peer: &peertransportsurface::ConnectedPeerRoot,
        local_path: &Path,
        peer_path: &str,
    ) -> Result<(), SnapshotDatabaseError> {
        let mut local_file = fs::File::open(local_path).map_err(|_| self.local_file_error())?;
        let mut write_handle = self
            .peertransportsurface
            .open_write(peer, peer_path)
            .map_err(|_| self.peer_transport_error())?;
        let mut buffer = vec![0_u8; COPY_CHUNK_BYTES];

        loop {
            let read = local_file
                .read(&mut buffer)
                .map_err(|_| self.local_file_error())?;
            if read == 0 {
                break;
            }
            if self
                .peertransportsurface
                .write(&mut write_handle, &buffer[..read])
                .is_err()
            {
                return Err(self.peer_transport_error());
            }
        }

        self.peertransportsurface
            .close_write(write_handle)
            .map_err(|_| self.peer_transport_error())
    }

    fn upsert_present_row(
        &self,
        database: &SnapshotDatabasePeerDatabase,
        entry: &SnapshotDatabaseEntryIdentity,
        mod_time: &str,
        byte_size: i64,
        last_seen: &str,
    ) -> Result<(), SnapshotDatabaseError> {
        let conn = self.open_database(&database.local_snapshot_path)?;
        conn.execute(
            "
            INSERT INTO snapshot
                (id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)
            ON CONFLICT(id) DO UPDATE SET
                parent_id = excluded.parent_id,
                basename = excluded.basename,
                mod_time = excluded.mod_time,
                byte_size = excluded.byte_size,
                last_seen = excluded.last_seen,
                deleted_time = NULL;
            ",
            params![
                &entry.id,
                &entry.parent_id,
                &entry.basename,
                mod_time,
                byte_size,
                last_seen
            ],
        )
        .map_err(|_| self.local_database_error())?;
        Ok(())
    }
}

impl SnapshotDatabase for SnapshotDatabaseImpl {
    fn prepare_peer_snapshot(
        &self,
        request: SnapshotDatabasePrepareRequest,
    ) -> SnapshotDatabasePrepareResult {
        if request.mode == SnapshotDatabaseRunMode::Normal
            && self.recover_snapshot_swap(&request.peer).is_err()
        {
            return SnapshotDatabasePrepareResult::Excluded(self.diagnostic(
                request.peer_index,
                SnapshotDatabaseDiagnosticKind::SnapshotStartupFailed,
            ));
        }

        match self.download_peer_snapshot(&request.peer, &request.local_snapshot_path) {
            Ok(true) => SnapshotDatabasePrepareResult::Prepared(SnapshotDatabasePreparedPeer {
                peer_index: request.peer_index,
                local_snapshot_path: request.local_snapshot_path,
                had_snapshot_history: true,
            }),
            Ok(false) => match self.create_database_at(&request.local_snapshot_path) {
                Ok(()) => SnapshotDatabasePrepareResult::Prepared(SnapshotDatabasePreparedPeer {
                    peer_index: request.peer_index,
                    local_snapshot_path: request.local_snapshot_path,
                    had_snapshot_history: false,
                }),
                Err(_) => SnapshotDatabasePrepareResult::Excluded(self.diagnostic(
                    request.peer_index,
                    SnapshotDatabaseDiagnosticKind::SnapshotStartupFailed,
                )),
            },
            Err(_) => SnapshotDatabasePrepareResult::Excluded(self.diagnostic(
                request.peer_index,
                SnapshotDatabaseDiagnosticKind::SnapshotStartupFailed,
            )),
        }
    }

    fn create_snapshot_database(&self, path: PathBuf) -> Result<(), SnapshotDatabaseError> {
        self.create_database_at(&path)
    }

    fn read_snapshot_row(
        &self,
        database: SnapshotDatabasePeerDatabase,
        entry_id: String,
    ) -> Result<Option<SnapshotDatabaseRow>, SnapshotDatabaseError> {
        let conn = self.open_database(&database.local_snapshot_path)?;
        conn.query_row(
            "
            SELECT id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time
            FROM snapshot
            WHERE id = ?1;
            ",
            params![&entry_id],
            |row| {
                Ok(SnapshotDatabaseRow {
                    id: row.get(0)?,
                    parent_id: row.get(1)?,
                    basename: row.get(2)?,
                    mod_time: row.get(3)?,
                    byte_size: row.get(4)?,
                    last_seen: row.get(5)?,
                    deleted_time: row.get(6)?,
                })
            },
        )
        .optional()
        .map_err(|_| self.local_database_error())
    }

    fn record_listed_file(
        &self,
        request: SnapshotDatabaseListedFileRequest,
    ) -> Result<(), SnapshotDatabaseError> {
        self.upsert_present_row(
            &request.database,
            &request.entry,
            &request.mod_time,
            request.byte_size,
            &request.last_seen,
        )
    }

    fn record_listed_directory(
        &self,
        request: SnapshotDatabaseListedDirectoryRequest,
    ) -> Result<(), SnapshotDatabaseError> {
        self.upsert_present_row(
            &request.database,
            &request.entry,
            &request.mod_time,
            -1,
            &request.last_seen,
        )
    }

    fn record_confirmed_file(
        &self,
        request: SnapshotDatabaseConfirmedFileRequest,
    ) -> Result<(), SnapshotDatabaseError> {
        self.upsert_present_row(
            &request.database,
            &request.entry,
            &request.mod_time,
            request.byte_size,
            &request.last_seen,
        )
    }

    fn record_intended_file_copy(
        &self,
        request: SnapshotDatabaseIntendedCopyRequest,
    ) -> Result<(), SnapshotDatabaseError> {
        let conn = self.open_database(&request.database.local_snapshot_path)?;
        conn.execute(
            "
            INSERT INTO snapshot
                (id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time)
            VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL)
            ON CONFLICT(id) DO UPDATE SET
                parent_id = excluded.parent_id,
                basename = excluded.basename,
                mod_time = excluded.mod_time,
                byte_size = excluded.byte_size,
                deleted_time = NULL;
            ",
            params![
                &request.entry.id,
                &request.entry.parent_id,
                &request.entry.basename,
                &request.mod_time,
                request.byte_size
            ],
        )
        .map_err(|_| self.local_database_error())?;
        Ok(())
    }

    fn record_completed_file_copy(
        &self,
        request: SnapshotDatabaseCompletedCopyRequest,
    ) -> Result<(), SnapshotDatabaseError> {
        let conn = self.open_database(&request.database.local_snapshot_path)?;
        conn.execute(
            "UPDATE snapshot SET last_seen = ?1 WHERE id = ?2;",
            params![&request.last_seen, &request.entry_id],
        )
        .map_err(|_| self.local_database_error())?;
        Ok(())
    }

    fn record_created_directory(
        &self,
        request: SnapshotDatabaseCreatedDirectoryRequest,
    ) -> Result<(), SnapshotDatabaseError> {
        self.upsert_present_row(
            &request.database,
            &request.entry,
            &request.mod_time,
            -1,
            &request.last_seen,
        )
    }

    fn record_confirmed_absence(
        &self,
        request: SnapshotDatabaseConfirmedAbsenceRequest,
    ) -> Result<(), SnapshotDatabaseError> {
        let conn = self.open_database(&request.database.local_snapshot_path)?;
        let existing: Option<(Option<String>, Option<String>)> = conn
            .query_row(
                "SELECT last_seen, deleted_time FROM snapshot WHERE id = ?1;",
                params![&request.entry_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|_| self.local_database_error())?;

        let Some((Some(last_seen), deleted_time)) = existing else {
            return Ok(());
        };
        let parsed_last_seen = self
            .formatrules
            .parse_timestamp(&last_seen)
            .map_err(|_| self.local_database_error())?;
        let parsed_deleted_time = deleted_time
            .as_deref()
            .map(|text| self.formatrules.parse_timestamp(text))
            .transpose()
            .map_err(|_| self.local_database_error())?;

        if let formatrules::FormatRulesDeletionEstimateUpdate::Write(timestamp) = self
            .formatrules
            .confirmed_absence_deleted_time(&parsed_last_seen, parsed_deleted_time.as_ref())
        {
            let deleted_time_text = self.formatrules.timestamp_text(&timestamp);
            conn.execute(
                "UPDATE snapshot SET deleted_time = ?1 WHERE id = ?2 AND deleted_time IS NULL;",
                params![&deleted_time_text, &request.entry_id],
            )
            .map_err(|_| self.local_database_error())?;
        }
        Ok(())
    }

    fn record_successful_displacement(
        &self,
        request: SnapshotDatabaseDisplacementRequest,
    ) -> Result<(), SnapshotDatabaseError> {
        let conn = self.open_database(&request.database.local_snapshot_path)?;
        let last_seen: Option<Option<String>> = conn
            .query_row(
                "SELECT last_seen FROM snapshot WHERE id = ?1;",
                params![&request.entry_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|_| self.local_database_error())?;
        let Some(Some(last_seen)) = last_seen else {
            return Ok(());
        };

        let parsed_last_seen = self
            .formatrules
            .parse_timestamp(&last_seen)
            .map_err(|_| self.local_database_error())?;
        let displaced_deleted_time = self
            .formatrules
            .displacement_deleted_time(&parsed_last_seen);
        let deleted_time_text = self.formatrules.timestamp_text(&displaced_deleted_time);

        if request.is_directory {
            let cascade_timestamp = self
                .formatrules
                .displacement_cascade_deleted_time(&displaced_deleted_time);
            let cascade_text = self.formatrules.timestamp_text(&cascade_timestamp);
            conn.execute(
                "
                WITH RECURSIVE subtree(id) AS (
                    VALUES(?1)
                    UNION ALL
                    SELECT s.id FROM snapshot s
                    JOIN subtree st ON s.parent_id = st.id
                    WHERE s.deleted_time IS NULL
                )
                UPDATE snapshot
                SET deleted_time = ?2
                WHERE deleted_time IS NULL
                AND id IN (SELECT id FROM subtree);
                ",
                params![&request.entry_id, cascade_text],
            )
            .map_err(|_| self.local_database_error())?;
        } else {
            conn.execute(
                "UPDATE snapshot SET deleted_time = ?1 WHERE id = ?2 AND deleted_time IS NULL;",
                params![deleted_time_text, &request.entry_id],
            )
            .map_err(|_| self.local_database_error())?;
        }
        Ok(())
    }

    fn cleanup_snapshot_rows(
        &self,
        request: SnapshotDatabaseCleanupRequest,
    ) -> Result<SnapshotDatabaseCleanupResult, SnapshotDatabaseError> {
        let conn = self.open_database(&request.database.local_snapshot_path)?;
        let removed_tombstone_rows = conn
            .execute(
                "DELETE FROM snapshot WHERE deleted_time IS NOT NULL AND deleted_time < ?1;",
                params![&request.older_than_timestamp],
            )
            .map_err(|_| self.local_database_error())?;

        let mut removed_stale_rows = 0;
        for id in request.obsolete_untombstoned_ids {
            removed_stale_rows += conn
                .execute(
                    "
                    DELETE FROM snapshot
                    WHERE id = ?1
                    AND deleted_time IS NULL
                    AND (last_seen IS NULL OR last_seen < ?2);
                    ",
                    params![id, &request.older_than_timestamp],
                )
                .map_err(|_| self.local_database_error())?;
        }

        Ok(SnapshotDatabaseCleanupResult {
            removed_tombstone_rows,
            removed_stale_rows,
        })
    }

    fn upload_snapshot(&self, request: SnapshotDatabaseUploadRequest) -> SnapshotDatabaseUploadResult {
        let paths = self.formatrules.snapshot_swap_paths();
        let mut swap_old_exists = false;

        if self
            .upload_local_file(&request.peer, &request.local_snapshot_path, &paths.new_path)
            .is_err()
        {
            return SnapshotDatabaseUploadResult::Failed(self.diagnostic(
                request.peer_index,
                SnapshotDatabaseDiagnosticKind::SnapshotUploadFailedBeforeSwapOld,
            ));
        }

        match self.peer_path_exists(&request.peer, LIVE_SNAPSHOT_PATH) {
            Ok(true) => {
                if self
                    .peertransportsurface
                    .rename(&request.peer, LIVE_SNAPSHOT_PATH, &paths.old_path)
                    .is_err()
                {
                    return SnapshotDatabaseUploadResult::Failed(self.diagnostic(
                        request.peer_index,
                        SnapshotDatabaseDiagnosticKind::SnapshotUploadFailedBeforeSwapOld,
                    ));
                }
                swap_old_exists = true;
            }
            Ok(false) => {}
            Err(_) => {
                return SnapshotDatabaseUploadResult::Failed(self.diagnostic(
                    request.peer_index,
                    SnapshotDatabaseDiagnosticKind::SnapshotUploadFailedBeforeSwapOld,
                ));
            }
        }

        if self
            .peertransportsurface
            .rename(&request.peer, &paths.new_path, LIVE_SNAPSHOT_PATH)
            .is_err()
        {
            let kind = if swap_old_exists {
                SnapshotDatabaseDiagnosticKind::SnapshotUploadFailedAfterSwapOld
            } else {
                SnapshotDatabaseDiagnosticKind::SnapshotUploadFailedBeforeSwapOld
            };
            return SnapshotDatabaseUploadResult::Failed(self.diagnostic(
                request.peer_index,
                kind,
            ));
        }

        match self.peertransportsurface.delete_file(&request.peer, &paths.old_path) {
            Ok(()) | Err(PeerTransportError::NotFound) => SnapshotDatabaseUploadResult::Uploaded,
            Err(_) => SnapshotDatabaseUploadResult::Failed(self.diagnostic(
                request.peer_index,
                SnapshotDatabaseDiagnosticKind::SnapshotUploadFailedAfterSwapOld,
            )),
        }
    }
}

pub fn new(
    formatrules: std::sync::Arc<dyn formatrules::FormatRules>,
    peertransportsurface: std::sync::Arc<dyn peertransportsurface::PeerTransportSurface>,
) -> std::sync::Arc<dyn SnapshotDatabase> {
    std::sync::Arc::new(SnapshotDatabaseImpl {
        formatrules,
        peertransportsurface,
    })
}
