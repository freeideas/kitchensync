use std::sync::Arc;
use crate::api::*;

struct PeerConnectionsImpl {
    formatrules: std::sync::Arc<dyn formatrules::FormatRules>,
    peertransportsurface: std::sync::Arc<dyn peertransportsurface::PeerTransportSurface>,
    snapshotdatabase: std::sync::Arc<dyn snapshotdatabase::SnapshotDatabase>,
}

impl PeerConnections for PeerConnectionsImpl {
    fn start( &self, request: PeerConnectionsStartupRequest, ) -> PeerConnectionsStartupResult {
        unimplemented!()
    }
}

pub fn new(formatrules: std::sync::Arc<dyn formatrules::FormatRules>, peertransportsurface: std::sync::Arc<dyn peertransportsurface::PeerTransportSurface>, snapshotdatabase: std::sync::Arc<dyn snapshotdatabase::SnapshotDatabase>) -> std::sync::Arc<dyn PeerConnections> {
    Arc::new(PeerConnectionsImpl { formatrules, peertransportsurface, snapshotdatabase })
}
