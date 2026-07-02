use std::sync::Arc;
use crate::api::*;

struct SnapshotPeerFilesImpl {
    snapshotdatabase: std::sync::Arc<dyn snapshotstore_snapshotdatabase::SnapshotDatabase>,
}

impl SnapshotPeerFiles for SnapshotPeerFilesImpl {
    fn start_normal_peer_snapshot( &self, request: SnapshotPeerFilesStartupRequest, ) -> SnapshotPeerFilesStartupResult {
        unimplemented!()
    }
    fn upload_normal_peer_snapshot( &self, request: SnapshotPeerFilesUploadRequest, ) -> SnapshotPeerFilesUploadResult {
        unimplemented!()
    }
}

pub fn new(snapshotdatabase: std::sync::Arc<dyn snapshotstore_snapshotdatabase::SnapshotDatabase>) -> std::sync::Arc<dyn SnapshotPeerFiles> {
    Arc::new(SnapshotPeerFilesImpl { snapshotdatabase })
}
