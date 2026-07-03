use std::sync::Arc;
use crate::api::*;

struct SyncTraversalImpl {
    formatrules: std::sync::Arc<dyn formatrules::FormatRules>,
    peertransportsurface: std::sync::Arc<dyn peertransportsurface::PeerTransportSurface>,
    snapshotdatabase: std::sync::Arc<dyn snapshotdatabase::SnapshotDatabase>,
    copystaging: std::sync::Arc<dyn copystaging::CopyStaging>,
}

impl SyncTraversal for SyncTraversalImpl {
    fn traverse(&self, request: SyncTraversalRequest) -> SyncTraversalResult {
        unimplemented!()
    }
}

pub fn new(formatrules: std::sync::Arc<dyn formatrules::FormatRules>, peertransportsurface: std::sync::Arc<dyn peertransportsurface::PeerTransportSurface>, snapshotdatabase: std::sync::Arc<dyn snapshotdatabase::SnapshotDatabase>, copystaging: std::sync::Arc<dyn copystaging::CopyStaging>) -> std::sync::Arc<dyn SyncTraversal> {
    Arc::new(SyncTraversalImpl { formatrules, peertransportsurface, snapshotdatabase, copystaging })
}
