use std::sync::Arc;
use crate::api::*;

struct CopyStagingImpl {
    formatrules: std::sync::Arc<dyn formatrules::FormatRules>,
    peertransportsurface: std::sync::Arc<dyn peertransportsurface::PeerTransportSurface>,
}

impl CopyStaging for CopyStagingImpl {
    fn copy_file(&self, request: CopyStagingCopyRequest) -> CopyStagingCopyResult {
        unimplemented!()
    }
    fn recover_user_swap( &self, request: CopyStagingDirectoryRequest, ) -> CopyStagingSwapRecoveryResult {
        unimplemented!()
    }
    fn displace_to_bak( &self, request: CopyStagingDisplacementRequest, ) -> CopyStagingDisplacementResult {
        unimplemented!()
    }
    fn cleanup_metadata( &self, request: CopyStagingDirectoryRequest, ) -> CopyStagingCleanupResult {
        unimplemented!()
    }
}

pub fn new(formatrules: std::sync::Arc<dyn formatrules::FormatRules>, peertransportsurface: std::sync::Arc<dyn peertransportsurface::PeerTransportSurface>) -> std::sync::Arc<dyn CopyStaging> {
    Arc::new(CopyStagingImpl { formatrules, peertransportsurface })
}
