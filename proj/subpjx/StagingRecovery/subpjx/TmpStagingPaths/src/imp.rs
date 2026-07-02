use crate::api::*;
use std::path::{Path, PathBuf};
use std::sync::Arc;

struct TmpStagingPathsImpl;

impl TmpStagingPaths for TmpStagingPathsImpl {
    fn prepare_tmp_staging_path(
        &self,
        request: TmpStagingPathRequest,
    ) -> Result<TmpStagingPathResult, TmpStagingPathError> {
        let paths = StagingPaths::new(
            &request.parent_path,
            &request.tmp_timestamp,
            &request.transfer_uuid,
        );

        match request.peer.scheme {
            TmpStagingPathPeerScheme::File => prepare_file_peer(&request, &paths),
            TmpStagingPathPeerScheme::Sftp => Err(error(
                &request,
                &paths,
                TmpStagingPathFailure::CreateTmpTimestampDirectory,
                "SFTP TMP staging is unavailable without an SFTP filesystem handle",
            )),
        }
    }
}

pub fn new() -> std::sync::Arc<dyn TmpStagingPaths> {
    Arc::new(TmpStagingPathsImpl)
}

struct StagingPaths {
    tmp_timestamp_directory: String,
    staging_path: String,
}

impl StagingPaths {
    fn new(parent_path: &str, tmp_timestamp: &str, transfer_uuid: &str) -> Self {
        let tmp_root = join_path(parent_path, ".kitchensync/TMP");
        let tmp_timestamp_directory = join_path(&tmp_root, tmp_timestamp);
        let staging_path = join_path(&tmp_timestamp_directory, transfer_uuid);

        Self {
            tmp_timestamp_directory,
            staging_path,
        }
    }
}

fn prepare_file_peer(
    request: &TmpStagingPathRequest,
    paths: &StagingPaths,
) -> Result<TmpStagingPathResult, TmpStagingPathError> {
    let Some(root) = file_root_path(request.peer.handle.as_ref()) else {
        return Err(error(
            request,
            paths,
            TmpStagingPathFailure::CreateTmpTimestampDirectory,
            "file peer handle does not contain a supported local root path",
        ));
    };

    let timestamp_directory = root.join(relative_path(&paths.tmp_timestamp_directory));
    if let Err(cause) = std::fs::create_dir_all(&timestamp_directory) {
        return Err(error(
            request,
            paths,
            TmpStagingPathFailure::CreateTmpTimestampDirectory,
            format!("failed to create TMP timestamp directory: {}", cause),
        ));
    }

    let staging_directory = root.join(relative_path(&paths.staging_path));
    if let Err(cause) = std::fs::create_dir_all(&staging_directory) {
        let failure = if staging_directory.exists() && !staging_directory.is_dir() {
            TmpStagingPathFailure::TmpPathNotDirectory
        } else {
            TmpStagingPathFailure::CreateTransferDirectory
        };

        return Err(error(
            request,
            paths,
            failure,
            format!("failed to create transfer TMP directory: {}", cause),
        ));
    }

    if !staging_directory.is_dir() {
        return Err(error(
            request,
            paths,
            TmpStagingPathFailure::TmpPathNotDirectory,
            "requested TMP staging path is not a directory",
        ));
    }

    Ok(TmpStagingPathResult {
        peer_identity: request.peer.identity.clone(),
        staging_path: paths.staging_path.clone(),
    })
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

fn join_path(parent: &str, child: &str) -> String {
    if parent.is_empty() {
        child.to_string()
    } else {
        format!("{}/{}", parent, child)
    }
}

fn error(
    request: &TmpStagingPathRequest,
    paths: &StagingPaths,
    failure: TmpStagingPathFailure,
    message: impl Into<String>,
) -> TmpStagingPathError {
    TmpStagingPathError {
        failure,
        peer_identity: request.peer.identity.clone(),
        parent_path: request.parent_path.clone(),
        tmp_timestamp_directory: paths.tmp_timestamp_directory.clone(),
        staging_path: paths.staging_path.clone(),
        message: message.into(),
    }
}
