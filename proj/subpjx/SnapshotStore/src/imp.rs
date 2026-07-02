use std::sync::Arc;
use std::time::SystemTime;
use crate::api::*;

struct SnapshotStoreImpl {
    snapshotdatabase: std::sync::Arc<dyn snapshotstore_snapshotdatabase::SnapshotDatabase>,
    snapshotidentity: std::sync::Arc<dyn snapshotstore_snapshotidentity::SnapshotIdentity>,
    snapshotpeerfiles: std::sync::Arc<dyn snapshotstore_snapshotpeerfiles::SnapshotPeerFiles>,
}

impl SnapshotStore for SnapshotStoreImpl {
    fn start_run(&self, request: SnapshotStartupRequest) -> SnapshotStartupResult {
        unimplemented!()
    }
    fn path_id(&self, relative_path: &str) -> Result<String, SnapshotStoreError> {
        unimplemented!()
    }
    fn parent_path_id(&self, relative_path: &str) -> Result<String, SnapshotStoreError> {
        unimplemented!()
    }
    fn format_utc_timestamp(&self, time: SystemTime) -> Result<String, SnapshotStoreError> {
        unimplemented!()
    }
    fn generate_timestamp(&self) -> Result<String, SnapshotStoreError> {
        unimplemented!()
    }
    fn lookup_row( &self, run_id: SnapshotRunId, peer_identity: &str, relative_path: &str, ) -> Result<Option<SnapshotRow>, SnapshotStoreError> {
        unimplemented!()
    }
    fn confirm_present( &self, run_id: SnapshotRunId, entry: SnapshotObservedEntry, ) -> Result<String, SnapshotStoreError> {
        unimplemented!()
    }
    fn confirm_absent( &self, run_id: SnapshotRunId, peer_identity: &str, relative_path: &str, ) -> Result<(), SnapshotStoreError> {
        unimplemented!()
    }
    fn record_intended_file_copy( &self, run_id: SnapshotRunId, copy: SnapshotIntendedFileCopy, ) -> Result<(), SnapshotStoreError> {
        unimplemented!()
    }
    fn complete_file_copy( &self, run_id: SnapshotRunId, peer_identity: &str, relative_path: &str, ) -> Result<String, SnapshotStoreError> {
        unimplemented!()
    }
    fn complete_directory_creation( &self, run_id: SnapshotRunId, peer_identity: &str, relative_path: &str, mod_time: String, ) -> Result<String, SnapshotStoreError> {
        unimplemented!()
    }
    fn complete_file_displacement( &self, run_id: SnapshotRunId, peer_identity: &str, relative_path: &str, ) -> Result<(), SnapshotStoreError> {
        unimplemented!()
    }
    fn complete_directory_displacement( &self, run_id: SnapshotRunId, peer_identity: &str, relative_path: &str, ) -> Result<(), SnapshotStoreError> {
        unimplemented!()
    }
    fn cleanup_peer( &self, run_id: SnapshotRunId, peer_identity: &str, keep_del_days: u32, ) -> Result<(), SnapshotStoreError> {
        unimplemented!()
    }
    fn upload_snapshots( &self, run_id: SnapshotRunId, peer_identities: Vec<String>, ) -> Result<SnapshotUploadResult, SnapshotStoreError> {
        unimplemented!()
    }
}

pub fn new(snapshotdatabase: std::sync::Arc<dyn snapshotstore_snapshotdatabase::SnapshotDatabase>, snapshotidentity: std::sync::Arc<dyn snapshotstore_snapshotidentity::SnapshotIdentity>, snapshotpeerfiles: std::sync::Arc<dyn snapshotstore_snapshotpeerfiles::SnapshotPeerFiles>) -> std::sync::Arc<dyn SnapshotStore> {
    Arc::new(SnapshotStoreImpl { snapshotdatabase, snapshotidentity, snapshotpeerfiles })
}
