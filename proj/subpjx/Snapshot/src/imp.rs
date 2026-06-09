use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use crate::api::{Snapshot, SnapshotError, SnapshotRow};

impl std::fmt::Debug for SnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SnapshotError::Database(s) => write!(f, "SnapshotError::Database({:?})", s),
            SnapshotError::Transport(s) => write!(f, "SnapshotError::Transport({:?})", s),
        }
    }
}

fn map_te(e: transport::TransportError) -> snapshot_transfer::TransferError {
    match e {
        transport::TransportError::NotFound => snapshot_transfer::TransferError::NotFound,
        transport::TransportError::PermissionDenied => snapshot_transfer::TransferError::PermissionDenied,
        transport::TransportError::Io => snapshot_transfer::TransferError::Io,
    }
}

fn from_te(e: snapshot_transfer::TransferError) -> SnapshotError {
    SnapshotError::Transport(match e {
        snapshot_transfer::TransferError::NotFound => "not found".to_string(),
        snapshot_transfer::TransferError::PermissionDenied => "permission denied".to_string(),
        snapshot_transfer::TransferError::Io => "I/O error".to_string(),
    })
}

fn from_db(e: rusqlite::Error) -> SnapshotError {
    SnapshotError::Database(e.to_string())
}

struct PeerFilesImpl {
    transport: Arc<dyn transport::Transport>,
    handle_id: u64,
}

impl snapshot_transfer::PeerFiles for PeerFilesImpl {
    fn exists(&self, path: &str) -> Result<bool, snapshot_transfer::TransferError> {
        let h = transport::PeerHandle(self.handle_id);
        match self.transport.stat(&h, path) {
            Ok(_) => Ok(true),
            Err(transport::TransportError::NotFound) => Ok(false),
            Err(e) => Err(map_te(e)),
        }
    }

    fn download(&self, remote: &str, local: &Path) -> Result<(), snapshot_transfer::TransferError> {
        let h = transport::PeerHandle(self.handle_id);
        let rh = self.transport.open_read(&h, remote).map_err(map_te)?;
        if let Some(parent) = local.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|_| snapshot_transfer::TransferError::Io)?;
        }
        let mut file = std::fs::File::create(local)
            .map_err(|_| snapshot_transfer::TransferError::Io)?;
        loop {
            match self.transport.read(&rh, 65536).map_err(map_te)? {
                None => break,
                Some(bytes) => file.write_all(&bytes)
                    .map_err(|_| snapshot_transfer::TransferError::Io)?,
            }
        }
        self.transport.close_read(rh).map_err(map_te)
    }

    fn upload(&self, local: &Path, remote: &str) -> Result<(), snapshot_transfer::TransferError> {
        let h = transport::PeerHandle(self.handle_id);
        let mut file = std::fs::File::open(local)
            .map_err(|_| snapshot_transfer::TransferError::Io)?;
        let wh = self.transport.open_write(&h, remote).map_err(map_te)?;
        let mut buf = vec![0u8; 65536];
        loop {
            let n = file.read(&mut buf)
                .map_err(|_| snapshot_transfer::TransferError::Io)?;
            if n == 0 {
                break;
            }
            self.transport.write(&wh, &buf[..n]).map_err(map_te)?;
        }
        self.transport.close_write(wh).map_err(map_te)
    }

    fn rename(&self, src: &str, dst: &str) -> Result<(), snapshot_transfer::TransferError> {
        let h = transport::PeerHandle(self.handle_id);
        self.transport.rename(&h, src, dst).map_err(map_te)
    }

    fn delete(&self, path: &str) -> Result<(), snapshot_transfer::TransferError> {
        let h = transport::PeerHandle(self.handle_id);
        self.transport.delete_file(&h, path).map_err(map_te)
    }

    fn delete_dir(&self, path: &str) -> Result<(), snapshot_transfer::TransferError> {
        let h = transport::PeerHandle(self.handle_id);
        self.transport.delete_dir(&h, path).map_err(map_te)
    }
}

struct PeerState {
    db_path: PathBuf,
    conn: rusqlite::Connection,
    handle_id: u64,
}

struct SnapshotImpl {
    transport: Arc<dyn transport::Transport>,
    clock: Arc<dyn snapshot_clock::Clock>,
    identity: Arc<dyn snapshot_identity::Identity>,
    store: Arc<dyn snapshot_store::Store>,
    transfer: Arc<dyn snapshot_transfer::Transfer>,
    peers: Mutex<HashMap<String, PeerState>>,
}

fn unix_secs_to_ymdhms(unix_secs: u64) -> (u32, u32, u32, u32, u32, u32) {
    let day_secs = (unix_secs % 86400) as u32;
    let hour = day_secs / 3600;
    let minute = (day_secs % 3600) / 60;
    let second = day_secs % 60;
    let days = unix_secs / 86400;
    // Julian Day Number for 1970-01-01 is 2440588
    let jdn = days as i64 + 2440588;
    let l = jdn + 68569;
    let n = 4 * l / 146097;
    let l = l - (146097 * n + 3) / 4;
    let i = 4000 * (l + 1) / 1461001;
    let l = l - 1461 * i / 4 + 31;
    let j = 80 * l / 2447;
    let day = (l - 2447 * j / 80) as u32;
    let l = j / 11;
    let month = (j + 2 - 12 * l) as u32;
    let year = (100 * (n - 49) + i + l) as u32;
    (year, month, day, hour, minute, second)
}

fn threshold_timestamp(keep_del_days: u32) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs().saturating_sub(keep_del_days as u64 * 86400);
    let (y, mo, d, h, mi, s) = unix_secs_to_ymdhms(secs);
    format!("{:04}-{:02}-{:02}_{:02}-{:02}-{:02}_000000Z", y, mo, d, h, mi, s)
}

impl Snapshot for SnapshotImpl {
    fn path_identity(&self, relative_path: &str) -> String {
        self.identity.identity(relative_path)
    }

    fn now(&self) -> String {
        self.clock.now()
    }

    fn open(&self, peer: &str, dry_run: bool) -> Result<(), SnapshotError> {
        let connected = self.transport
            .open_peer(peer, &[], dry_run, std::time::Duration::from_secs(30))
            .ok_or_else(|| SnapshotError::Transport("I/O error".to_string()))?;
        let handle_id = connected.handle.0;
        let peer_files = PeerFilesImpl {
            transport: Arc::clone(&self.transport),
            handle_id,
        };
        self.transfer.recover(&peer_files, dry_run).map_err(from_te)?;
        let tmp_dir = std::env::temp_dir();
        let downloaded = self.transfer
            .download(&peer_files, &tmp_dir, dry_run)
            .map_err(from_te)?;
        self.store
            .initialize(&downloaded.local_path)
            .map_err(|e| SnapshotError::Database(e.detail))?;
        let conn =
            rusqlite::Connection::open(&downloaded.local_path).map_err(from_db)?;
        self.peers.lock().unwrap().insert(
            peer.to_string(),
            PeerState { db_path: downloaded.local_path, conn, handle_id },
        );
        Ok(())
    }

    fn writeback(&self, peer: &str, dry_run: bool) -> Result<(), SnapshotError> {
        let state = self.peers
            .lock()
            .unwrap()
            .remove(peer)
            .ok_or_else(|| SnapshotError::Database(format!("peer not opened: {}", peer)))?;
        let db_path = state.db_path.clone();
        let handle_id = state.handle_id;
        state.conn.close().map_err(|(_, e)| SnapshotError::Database(e.to_string()))?;
        let peer_files = PeerFilesImpl {
            transport: Arc::clone(&self.transport),
            handle_id,
        };
        self.transfer.upload(&peer_files, &db_path, dry_run).map_err(from_te)
    }

    fn read_row(&self, peer: &str, id: &str) -> Result<Option<SnapshotRow>, SnapshotError> {
        let peers = self.peers.lock().unwrap();
        let state = peers
            .get(peer)
            .ok_or_else(|| SnapshotError::Database(format!("peer not opened: {}", peer)))?;
        let result = state.conn.query_row(
            "SELECT id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time \
             FROM snapshot WHERE id = ?1",
            rusqlite::params![id],
            |row| {
                Ok(SnapshotRow {
                    id: row.get(0)?,
                    parent_id: row.get(1)?,
                    basename: row.get(2)?,
                    mod_time: row.get(3)?,
                    byte_size: row.get(4)?,
                    last_seen: row.get(5)?,
                    deleted_time: row.get(6)?,
                })
            },
        );
        match result {
            Ok(row) => Ok(Some(row)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(from_db(e)),
        }
    }

    fn record_present(
        &self,
        peer: &str,
        id: &str,
        parent_id: &str,
        basename: &str,
        mod_time: &str,
        byte_size: i64,
    ) -> Result<(), SnapshotError> {
        let last_seen = self.clock.now();
        let peers = self.peers.lock().unwrap();
        let state = peers
            .get(peer)
            .ok_or_else(|| SnapshotError::Database(format!("peer not opened: {}", peer)))?;
        state
            .conn
            .execute(
                "INSERT INTO snapshot \
                   (id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL) \
                 ON CONFLICT(id) DO UPDATE SET \
                   mod_time = excluded.mod_time, \
                   byte_size = excluded.byte_size, \
                   last_seen = excluded.last_seen, \
                   deleted_time = NULL",
                rusqlite::params![id, parent_id, basename, mod_time, byte_size, last_seen],
            )
            .map(|_| ())
            .map_err(from_db)
    }

    fn record_absent(&self, peer: &str, id: &str) -> Result<(), SnapshotError> {
        let peers = self.peers.lock().unwrap();
        let state = peers
            .get(peer)
            .ok_or_else(|| SnapshotError::Database(format!("peer not opened: {}", peer)))?;
        state
            .conn
            .execute(
                "UPDATE snapshot SET deleted_time = last_seen \
                 WHERE id = ?1 AND deleted_time IS NULL",
                rusqlite::params![id],
            )
            .map(|_| ())
            .map_err(from_db)
    }

    fn record_push(
        &self,
        peer: &str,
        id: &str,
        parent_id: &str,
        basename: &str,
        mod_time: &str,
        byte_size: i64,
    ) -> Result<(), SnapshotError> {
        let peers = self.peers.lock().unwrap();
        let state = peers
            .get(peer)
            .ok_or_else(|| SnapshotError::Database(format!("peer not opened: {}", peer)))?;
        state
            .conn
            .execute(
                "INSERT INTO snapshot \
                   (id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time) \
                 VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL) \
                 ON CONFLICT(id) DO UPDATE SET \
                   mod_time = excluded.mod_time, \
                   byte_size = excluded.byte_size, \
                   deleted_time = NULL",
                rusqlite::params![id, parent_id, basename, mod_time, byte_size],
            )
            .map(|_| ())
            .map_err(from_db)
    }

    fn record_copied(&self, peer: &str, id: &str) -> Result<(), SnapshotError> {
        let last_seen = self.clock.now();
        let peers = self.peers.lock().unwrap();
        let state = peers
            .get(peer)
            .ok_or_else(|| SnapshotError::Database(format!("peer not opened: {}", peer)))?;
        state
            .conn
            .execute(
                "UPDATE snapshot SET last_seen = ?1 WHERE id = ?2",
                rusqlite::params![last_seen, id],
            )
            .map(|_| ())
            .map_err(from_db)
    }

    fn record_displaced(&self, peer: &str, id: &str) -> Result<(), SnapshotError> {
        let peers = self.peers.lock().unwrap();
        let state = peers
            .get(peer)
            .ok_or_else(|| SnapshotError::Database(format!("peer not opened: {}", peer)))?;
        let last_seen: Option<String> = state
            .conn
            .query_row(
                "SELECT last_seen FROM snapshot WHERE id = ?1",
                rusqlite::params![id],
                |row| row.get(0),
            )
            .map_err(from_db)?;
        state
            .conn
            .execute(
                "UPDATE snapshot SET deleted_time = ?1 WHERE id = ?2",
                rusqlite::params![last_seen, id],
            )
            .map(|_| ())
            .map_err(from_db)?;
        state
            .conn
            .execute(
                "WITH RECURSIVE desc(id) AS (\
                    SELECT id FROM snapshot WHERE parent_id = ?1 \
                    UNION ALL \
                    SELECT s.id FROM snapshot s INNER JOIN desc d ON s.parent_id = d.id\
                 ) \
                 UPDATE snapshot SET deleted_time = ?2 \
                 WHERE id IN (SELECT id FROM desc) AND deleted_time IS NULL",
                rusqlite::params![id, last_seen],
            )
            .map(|_| ())
            .map_err(from_db)
    }

    fn prune(&self, peer: &str, keep_del_days: u32) -> Result<(), SnapshotError> {
        let threshold = threshold_timestamp(keep_del_days);
        let peers = self.peers.lock().unwrap();
        let state = peers
            .get(peer)
            .ok_or_else(|| SnapshotError::Database(format!("peer not opened: {}", peer)))?;
        state
            .conn
            .execute(
                "DELETE FROM snapshot \
                 WHERE deleted_time IS NOT NULL AND deleted_time < ?1",
                rusqlite::params![threshold],
            )
            .map(|_| ())
            .map_err(from_db)?;
        state
            .conn
            .execute(
                "DELETE FROM snapshot \
                 WHERE deleted_time IS NULL AND last_seen IS NOT NULL AND last_seen < ?1",
                rusqlite::params![threshold],
            )
            .map(|_| ())
            .map_err(from_db)
    }
}

pub fn new(transport: Arc<dyn transport::Transport>) -> std::sync::Arc<dyn Snapshot> {
    // Snapshot owns its private helpers and builds them itself; a caller hands it
    // only the Transport service and never names the helper crates (see SPEC.md).
    let clock = snapshot_clock::new();
    let identity = snapshot_identity::new();
    let store = snapshot_store::new(clock.clone(), identity.clone());
    let transfer = snapshot_transfer::new();
    Arc::new(SnapshotImpl {
        transport,
        clock,
        identity,
        store,
        transfer,
        peers: Mutex::new(HashMap::new()),
    })
}
