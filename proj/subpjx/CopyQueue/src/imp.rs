use std::sync::Arc;
use crate::api::*;

struct CopyQueueImpl {
    transportoperations: std::sync::Arc<dyn transportoperations::TransportOperations>,
    snapshotstore: std::sync::Arc<dyn snapshotstore::SnapshotStore>,
    stagingrecovery: std::sync::Arc<dyn stagingrecovery::StagingRecovery>,
    queuerunner: std::sync::Arc<dyn copyqueue_queuerunner::QueueRunner>,
    stagedtransfer: std::sync::Arc<dyn copyqueue_stagedtransfer::StagedTransfer>,
}

impl CopyQueue for CopyQueueImpl {
    fn open_run(&self, request: CopyQueueRunRequest) -> Result<CopyQueueRunId, CopyQueueError> {
        unimplemented!()
    }
    fn enqueue(&self, run_id: CopyQueueRunId, copy: QueuedCopy) -> Result<(), CopyQueueError> {
        unimplemented!()
    }
    fn close_and_drain( &self, run_id: CopyQueueRunId, ) -> Result<CopyQueueDrainResult, CopyQueueError> {
        unimplemented!()
    }
}

pub fn new(transportoperations: std::sync::Arc<dyn transportoperations::TransportOperations>, snapshotstore: std::sync::Arc<dyn snapshotstore::SnapshotStore>, stagingrecovery: std::sync::Arc<dyn stagingrecovery::StagingRecovery>, queuerunner: std::sync::Arc<dyn copyqueue_queuerunner::QueueRunner>, stagedtransfer: std::sync::Arc<dyn copyqueue_stagedtransfer::StagedTransfer>) -> std::sync::Arc<dyn CopyQueue> {
    Arc::new(CopyQueueImpl { transportoperations, snapshotstore, stagingrecovery, queuerunner, stagedtransfer })
}
