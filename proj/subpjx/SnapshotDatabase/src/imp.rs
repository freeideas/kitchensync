use std::sync::Arc;
use crate::api::*;

struct SnapshotDatabaseImpl {
    formatrules: std::sync::Arc<dyn formatrules::FormatRules>,
    peertransportsurface: std::sync::Arc<dyn peertransportsurface::PeerTransportSurface>,
}

impl SnapshotDatabase for SnapshotDatabaseImpl {
    fn prepare_peer_snapshot( &self, request: SnapshotDatabasePrepareRequest, ) -> SnapshotDatabasePrepareResult {
        unimplemented!()
    }
    fn create_snapshot_database(&self, path: PathBuf) -> Result<(), SnapshotDatabaseError> {
        unimplemented!()
    }
    fn read_snapshot_row( &self, database: SnapshotDatabasePeerDatabase, entry_id: String, ) -> Result<Option<SnapshotDatabaseRow>, SnapshotDatabaseError> {
        unimplemented!()
    }
    fn record_listed_file( &self, request: SnapshotDatabaseListedFileRequest, ) -> Result<(), SnapshotDatabaseError> {
        unimplemented!()
    }
    fn record_listed_directory( &self, request: SnapshotDatabaseListedDirectoryRequest, ) -> Result<(), SnapshotDatabaseError> {
        unimplemented!()
    }
    fn record_confirmed_file( &self, request: SnapshotDatabaseConfirmedFileRequest, ) -> Result<(), SnapshotDatabaseError> {
        unimplemented!()
    }
    fn record_intended_file_copy( &self, request: SnapshotDatabaseIntendedCopyRequest, ) -> Result<(), SnapshotDatabaseError> {
        unimplemented!()
    }
    fn record_completed_file_copy( &self, request: SnapshotDatabaseCompletedCopyRequest, ) -> Result<(), SnapshotDatabaseError> {
        unimplemented!()
    }
    fn record_created_directory( &self, request: SnapshotDatabaseCreatedDirectoryRequest, ) -> Result<(), SnapshotDatabaseError> {
        unimplemented!()
    }
    fn record_confirmed_absence( &self, request: SnapshotDatabaseConfirmedAbsenceRequest, ) -> Result<(), SnapshotDatabaseError> {
        unimplemented!()
    }
    fn record_successful_displacement( &self, request: SnapshotDatabaseDisplacementRequest, ) -> Result<(), SnapshotDatabaseError> {
        unimplemented!()
    }
    fn cleanup_snapshot_rows( &self, request: SnapshotDatabaseCleanupRequest, ) -> Result<SnapshotDatabaseCleanupResult, SnapshotDatabaseError> {
        unimplemented!()
    }
    fn upload_snapshot( &self, request: SnapshotDatabaseUploadRequest, ) -> SnapshotDatabaseUploadResult {
        unimplemented!()
    }
}

pub fn new(formatrules: std::sync::Arc<dyn formatrules::FormatRules>, peertransportsurface: std::sync::Arc<dyn peertransportsurface::PeerTransportSurface>) -> std::sync::Arc<dyn SnapshotDatabase> {
    Arc::new(SnapshotDatabaseImpl { formatrules, peertransportsurface })
}
