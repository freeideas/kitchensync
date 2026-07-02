use crate::api::*;
use std::io::ErrorKind;
use std::sync::Arc;

struct FileUrlConnectionImpl;

impl FileUrlConnection for FileUrlConnectionImpl {
    fn establish_file_url(
        &self,
        request: FileUrlConnectionRequest,
    ) -> Result<FileUrlConnectionHandle, FileUrlConnectionFailure> {
        match request.run_mode {
            FileUrlConnectionRunMode::Normal => establish_normal(request),
            FileUrlConnectionRunMode::DryRun => establish_dry_run(request),
        }
    }
}

pub fn new() -> std::sync::Arc<dyn FileUrlConnection> {
    Arc::new(FileUrlConnectionImpl)
}

fn establish_normal(
    request: FileUrlConnectionRequest,
) -> Result<FileUrlConnectionHandle, FileUrlConnectionFailure> {
    match std::fs::metadata(&request.local_peer_root_path) {
        Ok(metadata) if metadata.is_dir() => Ok(handle(request)),
        Ok(_) => Err(failure(
            request,
            FileUrlConnectionFailureReason::PathIsNotDirectory,
            "path exists but is not a directory",
        )),
        Err(error)
            if error.kind() == ErrorKind::NotFound
                || error.kind() == ErrorKind::NotADirectory =>
        {
            create_missing_directory(request)
        }
        Err(error) => {
            let detail = format!("directory status unavailable: {}", error);
            Err(failure(
                request,
                FileUrlConnectionFailureReason::DirectoryStatusUnavailable,
                detail,
            ))
        }
    }
}

fn establish_dry_run(
    request: FileUrlConnectionRequest,
) -> Result<FileUrlConnectionHandle, FileUrlConnectionFailure> {
    match std::fs::metadata(&request.local_peer_root_path) {
        Ok(metadata) if metadata.is_dir() => Ok(handle(request)),
        Ok(_) => Err(failure(
            request,
            FileUrlConnectionFailureReason::PathIsNotDirectory,
            "path exists but is not a directory",
        )),
        Err(error) if error.kind() == ErrorKind::NotFound => Err(failure(
            request,
            FileUrlConnectionFailureReason::MissingDirectoryInDryRun,
            "directory is missing in dry-run mode",
        )),
        Err(error) => {
            let detail = format!("directory status unavailable: {}", error);
            Err(failure(
                request,
                FileUrlConnectionFailureReason::DirectoryStatusUnavailable,
                detail,
            ))
        }
    }
}

fn create_missing_directory(
    request: FileUrlConnectionRequest,
) -> Result<FileUrlConnectionHandle, FileUrlConnectionFailure> {
    if let Err(error) = std::fs::create_dir_all(&request.local_peer_root_path) {
        let detail = format!("directory creation failed: {}", error);
        return Err(failure(
            request,
            FileUrlConnectionFailureReason::DirectoryCreationFailed,
            detail,
        ));
    }

    match std::fs::metadata(&request.local_peer_root_path) {
        Ok(metadata) if metadata.is_dir() => Ok(handle(request)),
        Ok(_) => Err(failure(
            request,
            FileUrlConnectionFailureReason::DirectoryCreationFailed,
            "directory creation did not leave a directory at the peer root",
        )),
        Err(error) => {
            let detail = format!("directory status unavailable after creation: {}", error);
            Err(failure(
                request,
                FileUrlConnectionFailureReason::DirectoryCreationFailed,
                detail,
            ))
        }
    }
}

fn handle(request: FileUrlConnectionRequest) -> FileUrlConnectionHandle {
    FileUrlConnectionHandle {
        local_peer_root_path: request.local_peer_root_path,
    }
}

fn failure(
    request: FileUrlConnectionRequest,
    reason: FileUrlConnectionFailureReason,
    detail: impl Into<String>,
) -> FileUrlConnectionFailure {
    FileUrlConnectionFailure {
        local_peer_root_path: request.local_peer_root_path,
        reason,
        detail: detail.into(),
    }
}
