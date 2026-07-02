use crate::api::*;
use std::path::{Path, PathBuf};
use std::sync::Arc;

struct BakDisplacementImpl;

impl BakDisplacement for BakDisplacementImpl {
    fn displace_to_bak(
        &self,
        request: BakDisplacementRequest,
    ) -> Result<BakDisplacementRecord, BakDisplacementError> {
        let paths = DisplacementPaths::new(
            &request.parent_path,
            &request.basename,
            &request.bak_timestamp,
        );

        match request.peer.scheme {
            BakDisplacementPeerScheme::File => displace_file_peer(&request, &paths),
            BakDisplacementPeerScheme::Sftp => Err(error(
                &request,
                &paths,
                BakDisplacementFailure::CreateBakTimestampDirectory,
                "SFTP displacement is unavailable without an SFTP filesystem handle",
            )),
        }
    }
}

pub fn new() -> std::sync::Arc<dyn BakDisplacement> {
    Arc::new(BakDisplacementImpl)
}

struct DisplacementPaths {
    original_path: String,
    bak_timestamp_directory: String,
    bak_destination_path: String,
}

impl DisplacementPaths {
    fn new(parent_path: &str, basename: &str, bak_timestamp: &str) -> Self {
        let original_path = join_path(parent_path, basename);
        let bak_root = join_path(parent_path, ".kitchensync/BAK");
        let bak_timestamp_directory = join_path(&bak_root, bak_timestamp);
        let bak_destination_path = join_path(&bak_timestamp_directory, basename);

        Self {
            original_path,
            bak_timestamp_directory,
            bak_destination_path,
        }
    }
}

fn displace_file_peer(
    request: &BakDisplacementRequest,
    paths: &DisplacementPaths,
) -> Result<BakDisplacementRecord, BakDisplacementError> {
    let Some(root) = file_root_path(request.peer.handle.as_ref()) else {
        return Err(error(
            request,
            paths,
            BakDisplacementFailure::CreateBakTimestampDirectory,
            "file peer handle does not contain a supported local root path",
        ));
    };

    let bak_directory = root.join(relative_path(&paths.bak_timestamp_directory));
    if let Err(cause) = std::fs::create_dir_all(&bak_directory) {
        return Err(error(
            request,
            paths,
            BakDisplacementFailure::CreateBakTimestampDirectory,
            format!("failed to create BAK timestamp directory: {}", cause),
        ));
    }

    let original = root.join(relative_path(&paths.original_path));
    let destination = root.join(relative_path(&paths.bak_destination_path));
    if let Err(cause) = std::fs::rename(&original, &destination) {
        return Err(error(
            request,
            paths,
            BakDisplacementFailure::MoveDisplacedEntry,
            format!("failed to move displaced entry: {}", cause),
        ));
    }

    Ok(BakDisplacementRecord {
        peer_identity: request.peer.identity.clone(),
        original_path: paths.original_path.clone(),
        bak_timestamp_directory: paths.bak_timestamp_directory.clone(),
        bak_destination_path: paths.bak_destination_path.clone(),
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
    request: &BakDisplacementRequest,
    paths: &DisplacementPaths,
    failure: BakDisplacementFailure,
    message: impl Into<String>,
) -> BakDisplacementError {
    BakDisplacementError {
        failure,
        peer_identity: request.peer.identity.clone(),
        original_path: paths.original_path.clone(),
        bak_timestamp_directory: paths.bak_timestamp_directory.clone(),
        bak_destination_path: paths.bak_destination_path.clone(),
        message: message.into(),
    }
}
