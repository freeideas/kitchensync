use crate::api::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

const LIVE_SNAPSHOT: &str = ".kitchensync/snapshot.db";
const SWAP_NEW: &str = ".kitchensync/SWAP/snapshot.db/new";
const SWAP_OLD: &str = ".kitchensync/SWAP/snapshot.db/old";

struct SnapshotPeerFilesImpl {
    snapshotdatabase: Arc<dyn snapshotstore_snapshotdatabase::SnapshotDatabase>,
}

impl SnapshotPeerFiles for SnapshotPeerFilesImpl {
    fn start_normal_peer_snapshot(
        &self,
        request: SnapshotPeerFilesStartupRequest,
    ) -> SnapshotPeerFilesStartupResult {
        let paths = match SnapshotPeerPaths::new(&request.peer) {
            Ok(paths) => paths,
            Err(details) => {
                return startup_failure(
                    &request.peer.identity,
                    SnapshotPeerFilesStartupFailureKind::SwapRecoveryFailed,
                    None,
                    details,
                );
            }
        };

        if let Err(error) = recover_snapshot_swap(&paths) {
            return startup_failure(
                &request.peer.identity,
                SnapshotPeerFilesStartupFailureKind::SwapRecoveryFailed,
                Some(error.kind),
                error.details,
            );
        }

        let local_snapshot_path = request.local_snapshot_directory.join("snapshot.db");
        if let Err(error) = std::fs::create_dir_all(&request.local_snapshot_directory) {
            return startup_failure(
                &request.peer.identity,
                SnapshotPeerFilesStartupFailureKind::LocalDatabaseFailed,
                Some(error_kind(&error)),
                format!("failed to create local snapshot directory: {}", error),
            );
        }

        match std::fs::copy(&paths.live, &local_snapshot_path) {
            Ok(_) => SnapshotPeerFilesStartupResult::RecoveredAndDownloaded {
                peer_identity: request.peer.identity,
                local_snapshot_path,
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                match self.snapshotdatabase.create_empty(&local_snapshot_path) {
                    Ok(_) => SnapshotPeerFilesStartupResult::RecoveredWithNewEmptyLocalSnapshot {
                        peer_identity: request.peer.identity,
                        local_snapshot_path,
                    },
                    Err(error) => startup_failure(
                        &request.peer.identity,
                        SnapshotPeerFilesStartupFailureKind::LocalDatabaseFailed,
                        None,
                        error.message,
                    ),
                }
            }
            Err(error) => startup_failure(
                &request.peer.identity,
                SnapshotPeerFilesStartupFailureKind::SnapshotDownloadFailed,
                Some(error_kind(&error)),
                format!("failed to download peer snapshot: {}", error),
            ),
        }
    }

    fn upload_normal_peer_snapshot(
        &self,
        request: SnapshotPeerFilesUploadRequest,
    ) -> SnapshotPeerFilesUploadResult {
        let paths = SnapshotPeerPaths::new(&request.peer).map_err(|details| {
            SnapshotPeerFilesUploadFailure {
                peer_identity: request.peer.identity.clone(),
                kind: SnapshotPeerFilesUploadFailureKind::WriteSwapNewFailed,
                transport_error: None,
                details,
            }
        })?;

        let upload_lock = peer_upload_lock(&request.peer.identity, &request.peer.winning_url);
        let _upload_guard = upload_lock.lock().expect("snapshot upload lock poisoned");

        let snapshot_bytes =
            std::fs::read(&request.local_snapshot_path).map_err(|error| SnapshotPeerFilesUploadFailure {
                peer_identity: request.peer.identity.clone(),
                kind: SnapshotPeerFilesUploadFailureKind::PrepareLocalDatabaseFailed,
                transport_error: Some(error_kind(&error)),
                details: format!("failed to read local snapshot database: {}", error),
            })?;

        if let Some(parent) = paths.new.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                upload_failure(
                    &request.peer.identity,
                    SnapshotPeerFilesUploadFailureKind::WriteSwapNewFailed,
                    error,
                    "failed to create snapshot SWAP directory",
                )
            })?;
        }

        std::fs::write(&paths.new, snapshot_bytes).map_err(|error| {
            upload_failure(
                &request.peer.identity,
                SnapshotPeerFilesUploadFailureKind::WriteSwapNewFailed,
                error,
                "failed to write snapshot SWAP new",
            )
        })?;

        if paths.live.exists() {
            if paths.old.exists() {
                std::fs::remove_file(&paths.old).map_err(|error| {
                    upload_failure(
                        &request.peer.identity,
                        SnapshotPeerFilesUploadFailureKind::MoveLiveToSwapOldFailed,
                        error,
                        "failed to clear existing snapshot SWAP old",
                    )
                })?;
            }

            std::fs::rename(&paths.live, &paths.old).map_err(|error| {
                upload_failure(
                    &request.peer.identity,
                    SnapshotPeerFilesUploadFailureKind::MoveLiveToSwapOldFailed,
                    error,
                    "failed to move live snapshot to snapshot SWAP old",
                )
            })?;
        }

        if let Some(parent) = paths.live.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                upload_failure(
                    &request.peer.identity,
                    SnapshotPeerFilesUploadFailureKind::MoveNewToLiveFailed,
                    error,
                    "failed to create live snapshot directory",
                )
            })?;
        }

        std::fs::rename(&paths.new, &paths.live).map_err(|error| {
            upload_failure(
                &request.peer.identity,
                SnapshotPeerFilesUploadFailureKind::MoveNewToLiveFailed,
                error,
                "failed to move snapshot SWAP new to live snapshot",
            )
        })?;

        if paths.old.exists() {
            std::fs::remove_file(&paths.old).map_err(|error| {
                upload_failure(
                    &request.peer.identity,
                    SnapshotPeerFilesUploadFailureKind::RemoveSwapOldFailed,
                    error,
                    "failed to remove snapshot SWAP old",
                )
            })?;
        }

        Ok(())
    }
}

pub fn new(
    snapshotdatabase: Arc<dyn snapshotstore_snapshotdatabase::SnapshotDatabase>,
) -> Arc<dyn SnapshotPeerFiles> {
    Arc::new(SnapshotPeerFilesImpl { snapshotdatabase })
}

struct SnapshotPeerPaths {
    live: PathBuf,
    new: PathBuf,
    old: PathBuf,
}

impl SnapshotPeerPaths {
    fn new(peer: &SnapshotPeerFilesConnectedPeer) -> Result<Self, String> {
        if peer.scheme != SnapshotPeerFilesPeerScheme::File {
            return Err("snapshot peer file exchange requires a file peer".to_string());
        }

        let root = file_root_path(peer)
            .ok_or_else(|| "file peer handle does not identify a filesystem root".to_string())?;

        Ok(Self {
            live: root.join(LIVE_SNAPSHOT),
            new: root.join(SWAP_NEW),
            old: root.join(SWAP_OLD),
        })
    }
}

struct FileOperationError {
    kind: SnapshotPeerFilesTransportErrorKind,
    details: String,
}

fn recover_snapshot_swap(paths: &SnapshotPeerPaths) -> Result<(), FileOperationError> {
    let live_exists = paths.live.exists();
    let old_exists = paths.old.exists();
    let new_exists = paths.new.exists();

    match (live_exists, old_exists, new_exists) {
        (true, true, true) => {
            remove_file(&paths.old, "failed to remove snapshot SWAP old")?;
            remove_file(&paths.new, "failed to remove snapshot SWAP new")?;
        }
        (true, true, false) => {
            remove_file(&paths.old, "failed to remove snapshot SWAP old")?;
        }
        (false, true, true) => {
            rename_file(&paths.new, &paths.live, "failed to recover snapshot SWAP new")?;
            remove_file(&paths.old, "failed to remove snapshot SWAP old")?;
        }
        (false, true, false) => {
            rename_file(&paths.old, &paths.live, "failed to recover snapshot SWAP old")?;
        }
        (true, false, true) => {
            remove_file(&paths.new, "failed to remove snapshot SWAP new")?;
        }
        (false, false, true) => {
            rename_file(&paths.new, &paths.live, "failed to recover snapshot SWAP new")?;
        }
        _ => {}
    }

    Ok(())
}

fn remove_file(path: &Path, context: &str) -> Result<(), FileOperationError> {
    std::fs::remove_file(path).map_err(|error| FileOperationError {
        kind: error_kind(&error),
        details: format!("{}: {}", context, error),
    })
}

fn rename_file(source: &Path, destination: &Path, context: &str) -> Result<(), FileOperationError> {
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).map_err(|error| FileOperationError {
            kind: error_kind(&error),
            details: format!("failed to create destination directory: {}", error),
        })?;
    }

    std::fs::rename(source, destination).map_err(|error| FileOperationError {
        kind: error_kind(&error),
        details: format!("{}: {}", context, error),
    })
}

fn file_root_path(peer: &SnapshotPeerFilesConnectedPeer) -> Option<PathBuf> {
    if let Some(path) = peer.handle.downcast_ref::<PathBuf>() {
        return Some(path.clone());
    }

    if let Some(path) = peer.handle.downcast_ref::<String>() {
        return Some(PathBuf::from(path));
    }

    file_url_path(&peer.winning_url)
}

fn file_url_path(url: &str) -> Option<PathBuf> {
    url.strip_prefix("file://")
        .map(percent_decode_file_url_path)
        .or_else(|| Some(PathBuf::from(url)).filter(|path| path.is_absolute()))
}

fn percent_decode_file_url_path(path: &str) -> PathBuf {
    let mut bytes = Vec::new();
    let raw = path.as_bytes();
    let mut index = 0;
    while index < raw.len() {
        if raw[index] == b'%' && index + 2 < raw.len() {
            if let Some(byte) = hex_byte(raw[index + 1], raw[index + 2]) {
                bytes.push(byte);
                index += 3;
                continue;
            }
        }
        bytes.push(raw[index]);
        index += 1;
    }

    let decoded = String::from_utf8_lossy(&bytes).into_owned();
    if cfg!(windows) && decoded.starts_with('/') && decoded.as_bytes().get(2) == Some(&b':') {
        PathBuf::from(&decoded[1..])
    } else {
        PathBuf::from(decoded)
    }
}

fn hex_byte(high: u8, low: u8) -> Option<u8> {
    Some(hex_digit(high)? * 16 + hex_digit(low)?)
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn startup_failure(
    peer_identity: &str,
    kind: SnapshotPeerFilesStartupFailureKind,
    transport_error: Option<SnapshotPeerFilesTransportErrorKind>,
    details: impl Into<String>,
) -> SnapshotPeerFilesStartupResult {
    SnapshotPeerFilesStartupResult::Unavailable(SnapshotPeerFilesStartupFailure {
        peer_identity: peer_identity.to_string(),
        kind,
        transport_error,
        details: details.into(),
    })
}

fn upload_failure(
    peer_identity: &str,
    kind: SnapshotPeerFilesUploadFailureKind,
    error: std::io::Error,
    context: &str,
) -> SnapshotPeerFilesUploadFailure {
    SnapshotPeerFilesUploadFailure {
        peer_identity: peer_identity.to_string(),
        kind,
        transport_error: Some(error_kind(&error)),
        details: format!("{}: {}", context, error),
    }
}

fn error_kind(error: &std::io::Error) -> SnapshotPeerFilesTransportErrorKind {
    match error.kind() {
        std::io::ErrorKind::NotFound => SnapshotPeerFilesTransportErrorKind::NotFound,
        std::io::ErrorKind::PermissionDenied => SnapshotPeerFilesTransportErrorKind::PermissionDenied,
        _ => SnapshotPeerFilesTransportErrorKind::IoError,
    }
}

fn peer_upload_lock(peer_identity: &str, winning_url: &str) -> Arc<Mutex<()>> {
    static LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();
    let locks = LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
    let key = format!("{}\n{}", peer_identity, winning_url);
    let mut locks = locks.lock().expect("snapshot peer file lock poisoned");

    locks
        .entry(key)
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}
