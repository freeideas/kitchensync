use std::sync::Arc;
use crate::api::*;

struct DryRunModeImpl;

impl DryRunMode for DryRunModeImpl {
    fn dry_run_output_line(&self) -> String {
        unimplemented!()
    }
    fn startup_root_decision( &self, scheme: DryRunModePeerScheme, root_state: DryRunModeRootState, ) -> DryRunModeStartupRootDecision {
        unimplemented!()
    }
    fn snapshot_startup_decision( &self, outcome: DryRunModeSnapshotDownloadOutcome, ) -> DryRunModeSnapshotStartupDecision {
        unimplemented!()
    }
    fn classify_work(&self, work: DryRunModeWorkKind) -> DryRunModeWorkDecision {
        unimplemented!()
    }
    fn copy_work_policy(&self) -> DryRunModeCopyWorkPolicy {
        unimplemented!()
    }
}

pub fn new() -> std::sync::Arc<dyn DryRunMode> {
    Arc::new(DryRunModeImpl)
}
