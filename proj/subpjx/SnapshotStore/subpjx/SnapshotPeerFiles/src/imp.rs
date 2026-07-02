use std::sync::Arc;
use crate::api::*;

struct SnapshotPeerFilesImpl {
    snapshotdatabase: std::sync::Arc<dyn snapshotstore_snapshotdatabase::SnapshotDatabase>,
}

impl SnapshotPeerFiles for SnapshotPeerFilesImpl {
    fn start_normal_peer_snapshot( &self, request: SnapshotPeerFilesStartupRequest, ) -> SnapshotPeerFilesStartupResult {
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
    fn upload_normal_peer_snapshot( &self, request: SnapshotPeerFilesUploadRequest, ) -> SnapshotPeerFilesUploadResult {
        let paths = SnapshotPeerPaths::new(&request.peer).map_err(|details| SnapshotPeerFilesUploadFailure {
            peer_identity: request.peer.identity.clone(),
            kind: SnapshotPeerFilesUploadFailureKind::WriteSwapNewFailed,
            transport_error: None,
            details,
        })?;

        if let Some(parent) = paths.new.parent() {
            std::fs::create_dir_all(parent).map_err(|error| upload_failure(
                &request.peer.identity,
                SnapshotPeerFilesUploadFailureKind::WriteSwapNewFailed,
                error,
                "failed to create snapshot SWAP directory",
            ))?;
        }

        std::fs::copy(&request.local_snapshot_path, &paths.new).map_err(|error| upload_failure(
            &request.peer.identity,
            SnapshotPeerFilesUploadFailureKind::WriteSwapNewFailed,
            error,
            "failed to write snapshot SWAP new",
        ))?;

        if paths.live.exists() {
            if paths.old.exists() {
                std::fs::remove_file(&paths.old).map_err(|error| upload_failure(
                    &request.peer.identity,
                    SnapshotPeerFilesUploadFailureKind::MoveLiveToSwapOldFailed,
                    error,
                    "failed to clear existing snapshot SWAP old",
                ))?;
            }

            std::fs::rename(&paths.live, &paths.old).map_err(|error| upload_failure(
                &request.peer.identity,
                SnapshotPeerFilesUploadFailureKind::MoveLiveToSwapOldFailed,
                error,
                "failed to move live snapshot to snapshot SWAP old",
            ))?;
        }

        if let Some(parent) = paths.live.parent() {
            std::fs::create_dir_all(parent).map_err(|error| upload_failure(
                &request.peer.identity,
                SnapshotPeerFilesUploadFailureKind::MoveNewToLiveFailed,
                error,
                "failed to create live snapshot directory",
            ))?;
        }

        std::fs::rename(&paths.new, &paths.live).map_err(|error| upload_failure(
            &request.peer.identity,
            SnapshotPeerFilesUploadFailureKind::MoveNewToLiveFailed,
            error,
            "failed to move snapshot SWAP new to live snapshot",
        ))?;

        if paths.old.exists() {
            std::fs::remove_file(&paths.old).map_err(|error| upload_failure(
                &request.peer.identity,
                SnapshotPeerFilesUploadFailureKind::RemoveSwapOldFailed,
                error,
                "failed to remove snapshot SWAP old",
            ))?;
        }

        Ok(())
    }
}

pub fn new(snapshotdatabase: std::sync::Arc<dyn snapshotstore_snapshotdatabase::SnapshotDatabase>) -> std::sync::Arc<dyn SnapshotPeerFiles> {
    Arc::new(SnapshotPeerFilesImpl { snapshotdatabase })
}

struct SnapshotPeerPaths {
    live: std::path::PathBuf,
    new: std::path::PathBuf,
    old: std::path::PathBuf,
}

impl SnapshotPeerPaths {
    fn new(peer: &SnapshotPeerFilesConnectedPeer) -> Result<Self, String> {
        if peer.scheme != SnapshotPeerFilesPeerScheme::File {
            return Err("only file peers are supported by this implementation".to_string());
        }

        let root = file_root_path(peer.handle.as_ref())
            .ok_or_else(|| "file peer handle is not a filesystem path".to_string())?;

        Ok(Self {
            live: root.join(".kitchensync/snapshot.db"),
            new: root.join(".kitchensync/SWAP/snapshot.db/new"),
            old: root.join(".kitchensync/SWAP/snapshot.db/old"),
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

fn remove_file(path: &std::path::Path, context: &str) -> Result<(), FileOperationError> {
    std::fs::remove_file(path).map_err(|error| FileOperationError {
        kind: error_kind(&error),
        details: format!("{}: {}", context, error),
    })
}

fn rename_file(
    source: &std::path::Path,
    destination: &std::path::Path,
    context: &str,
) -> Result<(), FileOperationError> {
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

fn file_root_path(handle: &(dyn std::any::Any + Send + Sync)) -> Option<std::path::PathBuf> {
    if let Some(path) = handle.downcast_ref::<std::path::PathBuf>() {
        return Some(path.clone());
    }

    if let Some(path) = handle.downcast_ref::<String>() {
        return Some(std::path::PathBuf::from(path));
    }

    None
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
