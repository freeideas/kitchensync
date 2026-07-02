use crate::api::*;
use std::path::{Path, PathBuf};
use std::sync::Arc;

struct SwapRecoveryImpl;

impl SwapRecovery for SwapRecoveryImpl {
    fn recover_swap(&self, request: SwapRecoveryRequest) -> SwapRecoveryResult {
        match request.peer.scheme {
            SwapRecoveryPeerScheme::File => recover_file_peer(&request),
            SwapRecoveryPeerScheme::Sftp => failure(
                &request,
                SwapRecoveryFailureKind::SwapDirectoryListFailed,
                Some(swap_root_path(&request.parent_path)),
                Some(SwapRecoveryTransportErrorCategory::IoError),
                "SFTP recovery is unavailable without an SFTP filesystem handle",
            ),
        }
    }
}

pub fn new() -> std::sync::Arc<dyn SwapRecovery> {
    Arc::new(SwapRecoveryImpl)
}

struct SwapChildPaths {
    basename: String,
    target: String,
    swap_child: String,
    swap_old: String,
    swap_new: String,
    bak_timestamp_directory: String,
    bak_destination: String,
}

impl SwapChildPaths {
    fn new(parent_path: &str, encoded_basename: &str, basename: String, bak_timestamp: &str) -> Self {
        let target = join_path(parent_path, &basename);
        let swap_child = join_path(&swap_root_path(parent_path), encoded_basename);
        let swap_old = join_path(&swap_child, "old");
        let swap_new = join_path(&swap_child, "new");
        let bak_timestamp_directory = join_path(
            &join_path(&join_path(parent_path, ".kitchensync"), "BAK"),
            bak_timestamp,
        );
        let bak_destination = join_path(&bak_timestamp_directory, &basename);

        Self {
            basename,
            target,
            swap_child,
            swap_old,
            swap_new,
            bak_timestamp_directory,
            bak_destination,
        }
    }
}

fn recover_file_peer(request: &SwapRecoveryRequest) -> SwapRecoveryResult {
    let Some(root) = file_root_path(request.peer.handle.as_ref()) else {
        return failure(
            request,
            SwapRecoveryFailureKind::SwapDirectoryListFailed,
            Some(swap_root_path(&request.parent_path)),
            Some(SwapRecoveryTransportErrorCategory::IoError),
            "file peer handle does not contain a supported local root path",
        );
    };

    let swap_root = swap_root_path(&request.parent_path);
    let entries = match std::fs::read_dir(root.join(relative_path(&swap_root))) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return SwapRecoveryResult::Recovered;
        }
        Err(error) => {
            return failure(
                request,
                SwapRecoveryFailureKind::SwapDirectoryListFailed,
                Some(swap_root),
                Some(error_category(&error)),
                format!("failed to list SWAP directory: {}", error),
            );
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                return failure(
                    request,
                    SwapRecoveryFailureKind::SwapDirectoryListFailed,
                    Some(swap_root.clone()),
                    Some(error_category(&error)),
                    format!("failed to read SWAP directory child: {}", error),
                );
            }
        };

        let encoded_basename = match entry.file_name().into_string() {
            Ok(name) => name,
            Err(_) => {
                return failure(
                    request,
                    SwapRecoveryFailureKind::SwapBasenameDecodeFailed,
                    Some(swap_root.clone()),
                    None,
                    "SWAP child name is not valid UTF-8",
                );
            }
        };

        if encoded_basename == "snapshot.db" {
            continue;
        }

        let basename = match percent_decode_utf8(&encoded_basename) {
            Ok(basename) => basename,
            Err(message) => {
                return failure(
                    request,
                    SwapRecoveryFailureKind::SwapBasenameDecodeFailed,
                    Some(join_path(&swap_root, &encoded_basename)),
                    None,
                    message,
                );
            }
        };

        let paths = SwapChildPaths::new(
            &request.parent_path,
            &encoded_basename,
            basename,
            &request.bak_timestamp,
        );

        if let Err(result) = recover_file_swap_child(request, &root, &paths) {
            return result;
        }
    }

    SwapRecoveryResult::Recovered
}

fn recover_file_swap_child(
    request: &SwapRecoveryRequest,
    root: &Path,
    paths: &SwapChildPaths,
) -> Result<(), SwapRecoveryResult> {
    let old_exists = entry_exists(request, root, &paths.swap_old)?;
    let new_exists = entry_exists(request, root, &paths.swap_new)?;
    let target_exists = entry_exists(request, root, &paths.target)?;

    match (old_exists, new_exists, target_exists) {
        (true, true, true) => {
            return Err(failure(
                request,
                SwapRecoveryFailureKind::SwapStateCheckFailed,
                Some(paths.swap_child.clone()),
                None,
                format!(
                    "SWAP child for '{}' has old, new, and target entries",
                    paths.basename
                ),
            ));
        }
        (true, false, true) => {
            move_old_to_bak(request, root, paths)?;
        }
        (true, true, false) => {
            rename_entry(request, root, &paths.swap_new, &paths.target)?;
            move_old_to_bak(request, root, paths)?;
        }
        (true, false, false) => {
            rename_entry(request, root, &paths.swap_old, &paths.target)?;
        }
        (false, true, true) => {
            delete_entry(request, root, &paths.swap_new)?;
        }
        (false, true, false) => {
            rename_entry(request, root, &paths.swap_new, &paths.target)?;
        }
        (false, false, _) => {
            return Err(failure(
                request,
                SwapRecoveryFailureKind::SwapStateCheckFailed,
                Some(paths.swap_child.clone()),
                None,
                format!("SWAP child for '{}' has no recoverable old or new entry", paths.basename),
            ));
        }
    }

    std::fs::remove_dir(root.join(relative_path(&paths.swap_child))).map_err(|error| {
        failure(
            request,
            SwapRecoveryFailureKind::SwapCleanupFailed,
            Some(paths.swap_child.clone()),
            Some(error_category(&error)),
            format!("failed to remove recovered SWAP child directory: {}", error),
        )
    })?;

    Ok(())
}

fn entry_exists(
    request: &SwapRecoveryRequest,
    root: &Path,
    path: &str,
) -> Result<bool, SwapRecoveryResult> {
    match std::fs::metadata(root.join(relative_path(path))) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(failure(
            request,
            SwapRecoveryFailureKind::SwapStateCheckFailed,
            Some(path.to_string()),
            Some(error_category(&error)),
            format!("failed to inspect SWAP recovery path: {}", error),
        )),
    }
}

fn move_old_to_bak(
    request: &SwapRecoveryRequest,
    root: &Path,
    paths: &SwapChildPaths,
) -> Result<(), SwapRecoveryResult> {
    std::fs::create_dir_all(root.join(relative_path(&paths.bak_timestamp_directory))).map_err(
        |error| {
            failure(
                request,
                SwapRecoveryFailureKind::SwapCreateBakDirectoryFailed,
                Some(paths.bak_timestamp_directory.clone()),
                Some(error_category(&error)),
                format!("failed to create BAK timestamp directory: {}", error),
            )
        },
    )?;

    rename_entry(request, root, &paths.swap_old, &paths.bak_destination)
}

fn rename_entry(
    request: &SwapRecoveryRequest,
    root: &Path,
    from: &str,
    to: &str,
) -> Result<(), SwapRecoveryResult> {
    std::fs::rename(root.join(relative_path(from)), root.join(relative_path(to))).map_err(|error| {
        failure(
            request,
            SwapRecoveryFailureKind::SwapRenameFailed,
            Some(from.to_string()),
            Some(error_category(&error)),
            format!("failed to rename '{}' to '{}': {}", from, to, error),
        )
    })
}

fn delete_entry(
    request: &SwapRecoveryRequest,
    root: &Path,
    path: &str,
) -> Result<(), SwapRecoveryResult> {
    let absolute = root.join(relative_path(path));
    let metadata = std::fs::metadata(&absolute).map_err(|error| {
        failure(
            request,
            SwapRecoveryFailureKind::SwapDeleteFailed,
            Some(path.to_string()),
            Some(error_category(&error)),
            format!("failed to inspect entry before delete: {}", error),
        )
    })?;

    let result = if metadata.is_dir() {
        std::fs::remove_dir_all(&absolute)
    } else {
        std::fs::remove_file(&absolute)
    };

    result.map_err(|error| {
        failure(
            request,
            SwapRecoveryFailureKind::SwapDeleteFailed,
            Some(path.to_string()),
            Some(error_category(&error)),
            format!("failed to delete SWAP new entry: {}", error),
        )
    })
}

fn percent_decode_utf8(encoded: &str) -> Result<String, String> {
    let bytes = encoded.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err("SWAP child name contains an incomplete percent escape".to_string());
            }

            let high = hex_value(bytes[index + 1]).ok_or_else(|| {
                "SWAP child name contains an invalid percent escape".to_string()
            })?;
            let low = hex_value(bytes[index + 2]).ok_or_else(|| {
                "SWAP child name contains an invalid percent escape".to_string()
            })?;
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }

    String::from_utf8(decoded)
        .map_err(|_| "SWAP child name does not decode to valid UTF-8".to_string())
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn file_root_path(handle: &(dyn std::any::Any + Send + Sync)) -> Option<PathBuf> {
    if let Some(path) = handle.downcast_ref::<PathBuf>() {
        return Some(path.clone());
    }

    if let Some(path) = handle.downcast_ref::<String>() {
        return Some(PathBuf::from(path));
    }

    None
}

fn relative_path(path: &str) -> &Path {
    Path::new(path)
}

fn swap_root_path(parent_path: &str) -> String {
    join_path(&join_path(parent_path, ".kitchensync"), "SWAP")
}

fn join_path(parent: &str, child: &str) -> String {
    if parent.is_empty() {
        child.to_string()
    } else {
        format!("{}/{}", parent, child)
    }
}

fn error_category(error: &std::io::Error) -> SwapRecoveryTransportErrorCategory {
    match error.kind() {
        std::io::ErrorKind::NotFound => SwapRecoveryTransportErrorCategory::NotFound,
        std::io::ErrorKind::PermissionDenied => SwapRecoveryTransportErrorCategory::PermissionDenied,
        _ => SwapRecoveryTransportErrorCategory::IoError,
    }
}

fn failure(
    request: &SwapRecoveryRequest,
    kind: SwapRecoveryFailureKind,
    failed_path: Option<String>,
    transport_error: Option<SwapRecoveryTransportErrorCategory>,
    message: impl Into<String>,
) -> SwapRecoveryResult {
    SwapRecoveryResult::FailedListing(SwapRecoveryFailure {
        kind,
        peer_identity: request.peer.identity.clone(),
        parent_path: request.parent_path.clone(),
        failed_path,
        transport_error,
        message: message.into(),
    })
}
