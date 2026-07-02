use std::sync::Arc;
use crate::api::*;

struct SnapshotDatabaseImpl {
    snapshotcleanup: std::sync::Arc<dyn snapshotstore_snapshotdatabase_snapshotcleanup::SnapshotCleanup>,
    snapshotfile: std::sync::Arc<dyn snapshotstore_snapshotdatabase_snapshotfile::SnapshotFile>,
    snapshotrows: std::sync::Arc<dyn snapshotstore_snapshotdatabase_snapshotrows::SnapshotRows>,
}

impl SnapshotDatabase for SnapshotDatabaseImpl {
    fn create_empty( &self, local_path: &Path, ) -> SnapshotDatabaseResult<SnapshotDatabaseHandle> {
        unimplemented!()
    }
    fn open_existing( &self, local_path: &Path, ) -> SnapshotDatabaseResult<SnapshotDatabaseHandle> {
        unimplemented!()
    }
    fn lookup_row( &self, database: &SnapshotDatabaseHandle, id: &str, ) -> SnapshotDatabaseResult<Option<SnapshotRow>> {
        unimplemented!()
    }
    fn list_child_rows( &self, database: &SnapshotDatabaseHandle, parent_id: &str, ) -> SnapshotDatabaseResult<Vec<SnapshotRow>> {
        unimplemented!()
    }
    fn confirm_present( &self, database: &SnapshotDatabaseHandle, facts: &SnapshotRowFacts, last_seen: &str, ) -> SnapshotDatabaseResult<()> {
        unimplemented!()
    }
    fn confirm_absent( &self, database: &SnapshotDatabaseHandle, identity: &SnapshotRowIdentity, ) -> SnapshotDatabaseResult<()> {
        unimplemented!()
    }
    fn record_intended_file_copy( &self, database: &SnapshotDatabaseHandle, facts: &SnapshotRowFacts, ) -> SnapshotDatabaseResult<()> {
        unimplemented!()
    }
    fn complete_file_copy( &self, database: &SnapshotDatabaseHandle, identity: &SnapshotRowIdentity, last_seen: &str, ) -> SnapshotDatabaseResult<()> {
        unimplemented!()
    }
    fn complete_directory_creation( &self, database: &SnapshotDatabaseHandle, identity: &SnapshotRowIdentity, mod_time: &str, last_seen: &str, ) -> SnapshotDatabaseResult<()> {
        unimplemented!()
    }
    fn complete_displacement( &self, database: &SnapshotDatabaseHandle, identity: &SnapshotRowIdentity, ) -> SnapshotDatabaseResult<()> {
        unimplemented!()
    }
    fn complete_directory_displacement_cascade( &self, database: &SnapshotDatabaseHandle, identity: &SnapshotRowIdentity, ) -> SnapshotDatabaseResult<()> {
        unimplemented!()
    }
    fn cleanup_old_rows( &self, database: &SnapshotDatabaseHandle, cutoff: &str, ) -> SnapshotDatabaseResult<()> {
        unimplemented!()
    }
    fn prepare_for_upload( &self, database: SnapshotDatabaseHandle, ) -> SnapshotDatabaseResult<PathBuf> {
        unimplemented!()
    }
}

pub fn new(snapshotcleanup: std::sync::Arc<dyn snapshotstore_snapshotdatabase_snapshotcleanup::SnapshotCleanup>, snapshotfile: std::sync::Arc<dyn snapshotstore_snapshotdatabase_snapshotfile::SnapshotFile>, snapshotrows: std::sync::Arc<dyn snapshotstore_snapshotdatabase_snapshotrows::SnapshotRows>) -> std::sync::Arc<dyn SnapshotDatabase> {
    Arc::new(SnapshotDatabaseImpl { snapshotcleanup, snapshotfile, snapshotrows })
}
