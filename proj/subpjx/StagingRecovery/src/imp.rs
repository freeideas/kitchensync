use std::sync::Arc;
use crate::api::*;

struct StagingRecoveryImpl {
    transportoperations: std::sync::Arc<dyn transportoperations::TransportOperations>,
    bakdisplacement: std::sync::Arc<dyn stagingrecovery_bakdisplacement::BakDisplacement>,
    stagingcleanup: std::sync::Arc<dyn stagingrecovery_stagingcleanup::StagingCleanup>,
    swaprecovery: std::sync::Arc<dyn stagingrecovery_swaprecovery::SwapRecovery>,
    tmpstagingpaths: std::sync::Arc<dyn stagingrecovery_tmpstagingpaths::TmpStagingPaths>,
}

impl StagingRecovery for StagingRecoveryImpl {
    fn recover_swap(&self, request: SwapRecoveryRequest) -> SwapRecoveryResult {
        unimplemented!()
    }
    fn displace_to_bak( &self, request: BakDisplacementRequest, ) -> Result<BakDisplacementResult, StagingRecoveryFailure> {
        unimplemented!()
    }
    fn prepare_tmp_staging_path( &self, request: TmpStagingPathRequest, ) -> Result<TmpStagingPathResult, StagingRecoveryFailure> {
        unimplemented!()
    }
    fn cleanup_staging( &self, request: StagingCleanupRequest, ) -> Result<StagingCleanupResult, StagingRecoveryFailure> {
        unimplemented!()
    }
}

pub fn new(transportoperations: std::sync::Arc<dyn transportoperations::TransportOperations>, bakdisplacement: std::sync::Arc<dyn stagingrecovery_bakdisplacement::BakDisplacement>, stagingcleanup: std::sync::Arc<dyn stagingrecovery_stagingcleanup::StagingCleanup>, swaprecovery: std::sync::Arc<dyn stagingrecovery_swaprecovery::SwapRecovery>, tmpstagingpaths: std::sync::Arc<dyn stagingrecovery_tmpstagingpaths::TmpStagingPaths>) -> std::sync::Arc<dyn StagingRecovery> {
    Arc::new(StagingRecoveryImpl { transportoperations, bakdisplacement, stagingcleanup, swaprecovery, tmpstagingpaths })
}
