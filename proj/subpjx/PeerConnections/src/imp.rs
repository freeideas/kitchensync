use std::sync::Arc;
use crate::api::*;

struct PeerConnectionsImpl {
    fileurlconnection: std::sync::Arc<dyn peerconnections_fileurlconnection::FileUrlConnection>,
    sftpurlconnection: std::sync::Arc<dyn peerconnections_sftpurlconnection::SftpUrlConnection>,
    startupcoordinator: std::sync::Arc<dyn peerconnections_startupcoordinator::StartupCoordinator>,
}

impl PeerConnections for PeerConnectionsImpl {
    fn establish_peer_connections( &self, request: PeerConnectionStartupRequest, ) -> PeerConnectionStartupResult {
        unimplemented!()
    }
}

pub fn new(fileurlconnection: std::sync::Arc<dyn peerconnections_fileurlconnection::FileUrlConnection>, sftpurlconnection: std::sync::Arc<dyn peerconnections_sftpurlconnection::SftpUrlConnection>, startupcoordinator: std::sync::Arc<dyn peerconnections_startupcoordinator::StartupCoordinator>) -> std::sync::Arc<dyn PeerConnections> {
    Arc::new(PeerConnectionsImpl { fileurlconnection, sftpurlconnection, startupcoordinator })
}
