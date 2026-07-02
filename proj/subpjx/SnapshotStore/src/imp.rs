use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use crate::api::*;

type DatabaseHandle = snapshotstore_snapshotdatabase::SnapshotDatabaseHandle;

struct SnapshotStoreImpl {
    snapshotdatabase: Arc<dyn snapshotstore_snapshotdatabase::SnapshotDatabase>,
    snapshotidentity: Arc<dyn snapshotstore_snapshotidentity::SnapshotIdentity>,
    snapshotpeerfiles: Arc<dyn snapshotstore_snapshotpeerfiles::SnapshotPeerFiles>,
    runs: Mutex<RunState>,
}

struct RunState {
    next_run_id: u64,
    runs: HashMap<SnapshotRunId, RunRecord>,
}

struct RunRecord {
    mode: SnapshotRunMode,
    peers: HashMap<String, PeerRecord>,
}

struct PeerRecord {
    peer: SnapshotPeerHandle,
    database: Option<DatabaseHandle>,
}

impl SnapshotStoreImpl {
    fn next_run_id(&self) -> SnapshotRunId {
        let mut runs = self
            .runs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let run_id = SnapshotRunId(runs.next_run_id);
        runs.next_run_id += 1;
        runs.runs.insert(
            run_id,
            RunRecord {
                mode: SnapshotRunMode::Normal,
                peers: HashMap::new(),
            },
        );
        run_id
    }

    fn replace_run(&self, run_id: SnapshotRunId, record: RunRecord) {
        self.runs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .runs
            .insert(run_id, record);
    }

    fn with_peer<T>(
        &self,
        run_id: SnapshotRunId,
        peer_identity: &str,
        operation: impl FnOnce(&DatabaseHandle) -> Result<T, SnapshotStoreError>,
    ) -> Result<T, SnapshotStoreError> {
        let database = {
            let runs = self
                .runs
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let run = runs
                .runs
                .get(&run_id)
                .ok_or(SnapshotStoreError::UnknownRun(run_id))?;
            let peer = run
                .peers
                .get(peer_identity)
                .ok_or_else(|| SnapshotStoreError::UnknownPeer(peer_identity.to_string()))?;
            peer.database
                .clone()
                .ok_or_else(|| SnapshotStoreError::UnknownPeer(peer_identity.to_string()))?
        };

        operation(&database)
    }

    fn row_identity(
        &self,
        relative_path: &str,
    ) -> Result<snapshotstore_snapshotdatabase::SnapshotRowIdentity, SnapshotStoreError> {
        let id = self.path_id(relative_path)?;
        let parent_id = self.parent_path_id(relative_path)?;
        let basename = relative_path
            .rsplit('/')
            .next()
            .ok_or_else(|| SnapshotStoreError::InvalidRelativePath(relative_path.to_string()))?;

        Ok(snapshotstore_snapshotdatabase::SnapshotRowIdentity {
            id,
            parent_id,
            basename: basename.to_string(),
        })
    }

    fn row_facts(
        &self,
        relative_path: &str,
        mod_time: String,
        byte_size: i64,
    ) -> Result<snapshotstore_snapshotdatabase::SnapshotRowFacts, SnapshotStoreError> {
        validate_timestamp(&mod_time)?;

        Ok(snapshotstore_snapshotdatabase::SnapshotRowFacts {
            identity: self.row_identity(relative_path)?,
            mod_time,
            byte_size,
        })
    }

    fn dry_run_start_peer(
        &self,
        peer: &SnapshotPeerHandle,
        local_snapshot_directory: &Path,
    ) -> Result<(PathBuf, bool, DatabaseHandle), SnapshotStartupDiagnostic> {
        std::fs::create_dir_all(local_snapshot_directory).map_err(|error| {
            startup_diagnostic(
                SnapshotStartupFailureKind::LocalDatabaseFailed,
                format!("failed to create local snapshot directory: {}", error),
            )
        })?;

        let local_snapshot_path = local_snapshot_directory.join("snapshot.db");
        let live = peer_live_snapshot_path(peer).ok_or_else(|| {
            startup_diagnostic(
                SnapshotStartupFailureKind::SnapshotDownloadFailed,
                "file peer handle is not a filesystem path",
            )
        })?;

        match std::fs::copy(&live, &local_snapshot_path) {
            Ok(_) => {
                let database = self
                    .snapshotdatabase
                    .open_existing(&local_snapshot_path)
                    .map_err(|error| {
                        startup_diagnostic(SnapshotStartupFailureKind::LocalDatabaseFailed, error.message)
                    })?;
                Ok((local_snapshot_path, true, database))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let database = self
                    .snapshotdatabase
                    .create_empty(&local_snapshot_path)
                    .map_err(|error| {
                        startup_diagnostic(SnapshotStartupFailureKind::LocalDatabaseFailed, error.message)
                    })?;
                Ok((local_snapshot_path, false, database))
            }
            Err(error) => Err(startup_diagnostic(
                SnapshotStartupFailureKind::SnapshotDownloadFailed,
                format!("failed to download peer snapshot: {}", error),
            )),
        }
    }

    fn normal_start_peer(
        &self,
        peer: &SnapshotPeerHandle,
        local_snapshot_directory: PathBuf,
    ) -> Result<(PathBuf, bool, DatabaseHandle), SnapshotStartupDiagnostic> {
        let request = snapshotstore_snapshotpeerfiles::SnapshotPeerFilesStartupRequest {
            peer: to_peer_files_peer(peer),
            local_snapshot_directory,
        };

        match self.snapshotpeerfiles.start_normal_peer_snapshot(request) {
            snapshotstore_snapshotpeerfiles::SnapshotPeerFilesStartupResult::RecoveredAndDownloaded {
                local_snapshot_path,
                ..
            } => {
                let database = self
                    .snapshotdatabase
                    .open_existing(&local_snapshot_path)
                    .map_err(|error| {
                        startup_diagnostic(SnapshotStartupFailureKind::LocalDatabaseFailed, error.message)
                    })?;
                Ok((local_snapshot_path, true, database))
            }
            snapshotstore_snapshotpeerfiles::SnapshotPeerFilesStartupResult::RecoveredWithNewEmptyLocalSnapshot {
                local_snapshot_path,
                ..
            } => {
                let database = self
                    .snapshotdatabase
                    .open_existing(&local_snapshot_path)
                    .map_err(|error| {
                        startup_diagnostic(SnapshotStartupFailureKind::LocalDatabaseFailed, error.message)
                    })?;
                Ok((local_snapshot_path, false, database))
            }
            snapshotstore_snapshotpeerfiles::SnapshotPeerFilesStartupResult::Unavailable(failure) => {
                Err(startup_diagnostic(map_startup_failure_kind(failure.kind), failure.details))
            }
        }
    }
}

impl SnapshotStore for SnapshotStoreImpl {
    fn start_run(&self, request: SnapshotStartupRequest) -> SnapshotStartupResult {
        let run_id = self.next_run_id();
        let mut available_peers = Vec::new();
        let mut unavailable_peers = Vec::new();
        let mut peers = HashMap::new();

        for (index, peer) in request.peers.into_iter().enumerate() {
            let local_directory = request
                .temporary_root
                .join(run_id.0.to_string())
                .join(index.to_string());

            let startup = match request.run_mode {
                SnapshotRunMode::Normal => self.normal_start_peer(&peer, local_directory),
                SnapshotRunMode::DryRun => self.dry_run_start_peer(&peer, &local_directory),
            };

            match startup {
                Ok((local_snapshot_path, had_snapshot_history, database)) => {
                    available_peers.push(SnapshotStartupPeer {
                        peer_identity: peer.identity.clone(),
                        role: peer.role,
                        local_snapshot_path: local_snapshot_path.clone(),
                        had_snapshot_history,
                    });
                    peers.insert(
                        peer.identity.clone(),
                        PeerRecord {
                            peer,
                            database: Some(database),
                        },
                    );
                }
                Err(diagnostic) => unavailable_peers.push(UnavailableSnapshotPeer {
                    peer_identity: peer.identity,
                    role: peer.role,
                    diagnostic,
                }),
            }
        }

        self.replace_run(
            run_id,
            RunRecord {
                mode: request.run_mode,
                peers,
            },
        );

        SnapshotStartupResult {
            run_id,
            available_peers,
            unavailable_peers,
        }
    }

    fn path_id(&self, relative_path: &str) -> Result<String, SnapshotStoreError> {
        self.snapshotidentity
            .path_id(relative_path)
            .map_err(identity_error)
    }

    fn parent_path_id(&self, relative_path: &str) -> Result<String, SnapshotStoreError> {
        self.snapshotidentity
            .parent_path_id(relative_path)
            .map_err(identity_error)
    }

    fn format_utc_timestamp(&self, time: SystemTime) -> Result<String, SnapshotStoreError> {
        self.snapshotidentity
            .format_utc_timestamp(time)
            .map_err(identity_error)
    }

    fn generate_timestamp(&self) -> Result<String, SnapshotStoreError> {
        self.snapshotidentity
            .generate_timestamp()
            .map_err(identity_error)
    }

    fn lookup_row(
        &self,
        run_id: SnapshotRunId,
        peer_identity: &str,
        relative_path: &str,
    ) -> Result<Option<SnapshotRow>, SnapshotStoreError> {
        let id = self.path_id(relative_path)?;
        self.with_peer(run_id, peer_identity, |database| {
            self.snapshotdatabase
                .lookup_row(database, &id)
                .map(|row| row.map(to_snapshot_row))
                .map_err(database_error)
        })
    }

    fn confirm_present(
        &self,
        run_id: SnapshotRunId,
        entry: SnapshotObservedEntry,
    ) -> Result<String, SnapshotStoreError> {
        let byte_size = match entry.entry_kind {
            SnapshotEntryKind::File { byte_size } => file_size_to_i64(byte_size)?,
            SnapshotEntryKind::Directory => -1,
        };
        let facts = self.row_facts(&entry.relative_path, entry.mod_time, byte_size)?;
        let last_seen = self.generate_timestamp()?;
        self.with_peer(run_id, &entry.peer_identity, |database| {
            self.snapshotdatabase
                .confirm_present(database, &facts, &last_seen)
                .map_err(database_error)
        })?;
        Ok(last_seen)
    }

    fn confirm_absent(
        &self,
        run_id: SnapshotRunId,
        peer_identity: &str,
        relative_path: &str,
    ) -> Result<(), SnapshotStoreError> {
        let identity = self.row_identity(relative_path)?;
        self.with_peer(run_id, peer_identity, |database| {
            self.snapshotdatabase
                .confirm_absent(database, &identity)
                .map_err(database_error)
        })
    }

    fn record_intended_file_copy(
        &self,
        run_id: SnapshotRunId,
        copy: SnapshotIntendedFileCopy,
    ) -> Result<(), SnapshotStoreError> {
        let facts = self.row_facts(
            &copy.destination_relative_path,
            copy.winning_mod_time,
            file_size_to_i64(copy.winning_byte_size)?,
        )?;
        self.with_peer(run_id, &copy.destination_peer_identity, |database| {
            self.snapshotdatabase
                .record_intended_file_copy(database, &facts)
                .map_err(database_error)
        })
    }

    fn complete_file_copy(
        &self,
        run_id: SnapshotRunId,
        peer_identity: &str,
        relative_path: &str,
    ) -> Result<String, SnapshotStoreError> {
        let identity = self.row_identity(relative_path)?;
        let last_seen = self.generate_timestamp()?;
        self.with_peer(run_id, peer_identity, |database| {
            self.snapshotdatabase
                .complete_file_copy(database, &identity, &last_seen)
                .map_err(database_error)
        })?;
        Ok(last_seen)
    }

    fn complete_directory_creation(
        &self,
        run_id: SnapshotRunId,
        peer_identity: &str,
        relative_path: &str,
        mod_time: String,
    ) -> Result<String, SnapshotStoreError> {
        validate_timestamp(&mod_time)?;

        let identity = self.row_identity(relative_path)?;
        let last_seen = self.generate_timestamp()?;
        self.with_peer(run_id, peer_identity, |database| {
            self.snapshotdatabase
                .complete_directory_creation(database, &identity, &mod_time, &last_seen)
                .map_err(database_error)
        })?;
        Ok(last_seen)
    }

    fn complete_file_displacement(
        &self,
        run_id: SnapshotRunId,
        peer_identity: &str,
        relative_path: &str,
    ) -> Result<(), SnapshotStoreError> {
        let identity = self.row_identity(relative_path)?;
        self.with_peer(run_id, peer_identity, |database| {
            self.snapshotdatabase
                .complete_displacement(database, &identity)
                .map_err(database_error)
        })
    }

    fn complete_directory_displacement(
        &self,
        run_id: SnapshotRunId,
        peer_identity: &str,
        relative_path: &str,
    ) -> Result<(), SnapshotStoreError> {
        let identity = self.row_identity(relative_path)?;
        self.with_peer(run_id, peer_identity, |database| {
            self.snapshotdatabase
                .complete_directory_displacement_cascade(database, &identity)
                .map_err(database_error)
        })
    }

    fn cleanup_peer(
        &self,
        run_id: SnapshotRunId,
        peer_identity: &str,
        keep_del_days: u32,
    ) -> Result<(), SnapshotStoreError> {
        let cutoff_time = SystemTime::now()
            .checked_sub(Duration::from_secs(u64::from(keep_del_days) * 86_400))
            .ok_or_else(|| SnapshotStoreError::TimestampUnavailable("cleanup cutoff is out of range".to_string()))?;
        let cutoff = self.format_utc_timestamp(cutoff_time)?;
        self.with_peer(run_id, peer_identity, |database| {
            self.snapshotdatabase
                .cleanup_old_rows(database, &cutoff)
                .map_err(database_error)
        })
    }

    fn upload_snapshots(
        &self,
        run_id: SnapshotRunId,
        peer_identities: Vec<String>,
    ) -> Result<SnapshotUploadResult, SnapshotStoreError> {
        {
            let runs = self
                .runs
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let run = runs
                .runs
                .get(&run_id)
                .ok_or(SnapshotStoreError::UnknownRun(run_id))?;
            if run.mode == SnapshotRunMode::DryRun {
                return Err(SnapshotStoreError::DryRunUploadForbidden);
            }
        }

        let mut uploaded_peers = Vec::new();
        let mut failed_peers = Vec::new();

        for peer_identity in peer_identities {
            let (peer, database) = {
                let mut runs = self
                    .runs
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let run = runs
                    .runs
                    .get_mut(&run_id)
                    .ok_or(SnapshotStoreError::UnknownRun(run_id))?;
                let peer = run
                    .peers
                    .get_mut(&peer_identity)
                    .ok_or_else(|| SnapshotStoreError::UnknownPeer(peer_identity.clone()))?;
                let database = peer
                    .database
                    .take()
                    .ok_or_else(|| SnapshotStoreError::UnknownPeer(peer_identity.clone()))?;
                (peer.peer.clone(), database)
            };

            let local_snapshot_path = match self.snapshotdatabase.prepare_for_upload(database) {
                Ok(path) => path,
                Err(error) => {
                    failed_peers.push(SnapshotUploadFailure {
                        peer_identity,
                        kind: SnapshotUploadFailureKind::PrepareLocalDatabaseFailed,
                        details: error.message,
                    });
                    continue;
                }
            };

            let request = snapshotstore_snapshotpeerfiles::SnapshotPeerFilesUploadRequest {
                peer: to_peer_files_peer(&peer),
                local_snapshot_path,
            };

            match self.snapshotpeerfiles.upload_normal_peer_snapshot(request) {
                Ok(()) => uploaded_peers.push(peer_identity),
                Err(error) => failed_peers.push(SnapshotUploadFailure {
                    peer_identity: error.peer_identity,
                    kind: map_upload_failure_kind(error.kind),
                    details: error.details,
                }),
            }
        }

        Ok(SnapshotUploadResult {
            uploaded_peers,
            failed_peers,
        })
    }
}

pub fn new(
    snapshotdatabase: Arc<dyn snapshotstore_snapshotdatabase::SnapshotDatabase>,
    snapshotidentity: Arc<dyn snapshotstore_snapshotidentity::SnapshotIdentity>,
    snapshotpeerfiles: Arc<dyn snapshotstore_snapshotpeerfiles::SnapshotPeerFiles>,
) -> Arc<dyn SnapshotStore> {
    Arc::new(SnapshotStoreImpl {
        snapshotdatabase,
        snapshotidentity,
        snapshotpeerfiles,
        runs: Mutex::new(RunState {
            next_run_id: 1,
            runs: HashMap::new(),
        }),
    })
}

fn to_snapshot_row(row: snapshotstore_snapshotdatabase::SnapshotRow) -> SnapshotRow {
    SnapshotRow {
        id: row.identity.id,
        parent_id: row.identity.parent_id,
        basename: row.identity.basename,
        mod_time: row.mod_time,
        byte_size: row.byte_size,
        last_seen: row.last_seen,
        deleted_time: row.deleted_time,
    }
}

fn to_peer_files_peer(
    peer: &SnapshotPeerHandle,
) -> snapshotstore_snapshotpeerfiles::SnapshotPeerFilesConnectedPeer {
    snapshotstore_snapshotpeerfiles::SnapshotPeerFilesConnectedPeer {
        identity: peer.identity.clone(),
        winning_url: peer.winning_url.clone(),
        scheme: match peer.scheme {
            SnapshotPeerScheme::File => snapshotstore_snapshotpeerfiles::SnapshotPeerFilesPeerScheme::File,
            SnapshotPeerScheme::Sftp => snapshotstore_snapshotpeerfiles::SnapshotPeerFilesPeerScheme::Sftp,
        },
        handle: peer.handle.clone(),
    }
}

fn peer_live_snapshot_path(peer: &SnapshotPeerHandle) -> Option<PathBuf> {
    if peer.scheme != SnapshotPeerScheme::File {
        return None;
    }

    if let Some(path) = peer.handle.downcast_ref::<PathBuf>() {
        return Some(path.join(".kitchensync/snapshot.db"));
    }

    peer.handle
        .downcast_ref::<String>()
        .map(|path| PathBuf::from(path).join(".kitchensync/snapshot.db"))
}

fn identity_error(error: snapshotstore_snapshotidentity::SnapshotIdentityError) -> SnapshotStoreError {
    match error.kind {
        snapshotstore_snapshotidentity::SnapshotIdentityErrorKind::InvalidRelativePath => {
            SnapshotStoreError::InvalidRelativePath(error.message)
        }
        snapshotstore_snapshotidentity::SnapshotIdentityErrorKind::TimestampOutOfRange => {
            SnapshotStoreError::InvalidTimestamp(error.message)
        }
        snapshotstore_snapshotidentity::SnapshotIdentityErrorKind::SystemClockUnavailable => {
            SnapshotStoreError::TimestampUnavailable(error.message)
        }
    }
}

fn database_error(error: snapshotstore_snapshotdatabase::SnapshotDatabaseError) -> SnapshotStoreError {
    SnapshotStoreError::Database(error.message)
}

fn file_size_to_i64(byte_size: u64) -> Result<i64, SnapshotStoreError> {
    i64::try_from(byte_size)
        .map_err(|_| SnapshotStoreError::Database("file size exceeds SQLite INTEGER range".to_string()))
}

fn validate_timestamp(timestamp: &str) -> Result<(), SnapshotStoreError> {
    let bytes = timestamp.as_bytes();
    let structurally_valid = bytes.len() == 27
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'_'
        && bytes[13] == b'-'
        && bytes[16] == b'-'
        && bytes[19] == b'_'
        && bytes[26] == b'Z'
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[8..10].iter().all(u8::is_ascii_digit)
        && bytes[11..13].iter().all(u8::is_ascii_digit)
        && bytes[14..16].iter().all(u8::is_ascii_digit)
        && bytes[17..19].iter().all(u8::is_ascii_digit)
        && bytes[20..26].iter().all(u8::is_ascii_digit);

    if !structurally_valid {
        return Err(SnapshotStoreError::InvalidTimestamp(timestamp.to_string()));
    }

    let year = parse_digits(&bytes[..4]);
    let month = parse_digits(&bytes[5..7]);
    let day = parse_digits(&bytes[8..10]);
    let hour = parse_digits(&bytes[11..13]);
    let minute = parse_digits(&bytes[14..16]);
    let second = parse_digits(&bytes[17..19]);

    if year >= 1
        && (1..=12).contains(&month)
        && (1..=days_in_month(year, month)).contains(&day)
        && hour <= 23
        && minute <= 59
        && second <= 59
    {
        Ok(())
    } else {
        Err(SnapshotStoreError::InvalidTimestamp(timestamp.to_string()))
    }
}

fn parse_digits(bytes: &[u8]) -> u32 {
    bytes
        .iter()
        .fold(0, |value, byte| value * 10 + u32::from(byte - b'0'))
}

fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap_year(year: u32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn startup_diagnostic(
    kind: SnapshotStartupFailureKind,
    details: impl Into<String>,
) -> SnapshotStartupDiagnostic {
    SnapshotStartupDiagnostic {
        kind,
        details: details.into(),
    }
}

fn map_startup_failure_kind(
    kind: snapshotstore_snapshotpeerfiles::SnapshotPeerFilesStartupFailureKind,
) -> SnapshotStartupFailureKind {
    match kind {
        snapshotstore_snapshotpeerfiles::SnapshotPeerFilesStartupFailureKind::SwapRecoveryFailed => {
            SnapshotStartupFailureKind::SwapRecoveryFailed
        }
        snapshotstore_snapshotpeerfiles::SnapshotPeerFilesStartupFailureKind::SnapshotDownloadFailed => {
            SnapshotStartupFailureKind::SnapshotDownloadFailed
        }
        snapshotstore_snapshotpeerfiles::SnapshotPeerFilesStartupFailureKind::LocalDatabaseFailed => {
            SnapshotStartupFailureKind::LocalDatabaseFailed
        }
    }
}

fn map_upload_failure_kind(
    kind: snapshotstore_snapshotpeerfiles::SnapshotPeerFilesUploadFailureKind,
) -> SnapshotUploadFailureKind {
    match kind {
        snapshotstore_snapshotpeerfiles::SnapshotPeerFilesUploadFailureKind::PrepareLocalDatabaseFailed => {
            SnapshotUploadFailureKind::PrepareLocalDatabaseFailed
        }
        snapshotstore_snapshotpeerfiles::SnapshotPeerFilesUploadFailureKind::WriteSwapNewFailed => {
            SnapshotUploadFailureKind::WriteSwapNewFailed
        }
        snapshotstore_snapshotpeerfiles::SnapshotPeerFilesUploadFailureKind::MoveLiveToSwapOldFailed => {
            SnapshotUploadFailureKind::MoveLiveToSwapOldFailed
        }
        snapshotstore_snapshotpeerfiles::SnapshotPeerFilesUploadFailureKind::MoveNewToLiveFailed => {
            SnapshotUploadFailureKind::MoveNewToLiveFailed
        }
        snapshotstore_snapshotpeerfiles::SnapshotPeerFilesUploadFailureKind::RemoveSwapOldFailed => {
            SnapshotUploadFailureKind::RemoveSwapOldFailed
        }
    }
}
