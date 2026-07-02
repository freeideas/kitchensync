use std::sync::Arc;
use crate::api::*;

struct QueueRunnerImpl {
    stagedtransfer: std::sync::Arc<dyn copyqueue_stagedtransfer::StagedTransfer>,
}

impl QueueRunner for QueueRunnerImpl {
    fn start_run(&self, config: QueueRunnerRunConfig) {
        unimplemented!()
    }
    fn enqueue_copy(&self, copy: QueueRunnerCopyWork) {
        unimplemented!()
    }
    fn close_and_drain(&self) -> QueueRunnerRunResult {
        unimplemented!()
    }
}

pub fn new(stagedtransfer: std::sync::Arc<dyn copyqueue_stagedtransfer::StagedTransfer>) -> std::sync::Arc<dyn QueueRunner> {
    Arc::new(QueueRunnerImpl { stagedtransfer })
}
