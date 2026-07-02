use std::sync::Arc;
use crate::api::*;

struct StartupCoordinatorImpl {
    fileurlconnection: std::sync::Arc<dyn peerconnections_fileurlconnection::FileUrlConnection>,
    sftpurlconnection: std::sync::Arc<dyn peerconnections_sftpurlconnection::SftpUrlConnection>,
}

impl StartupCoordinator for StartupCoordinatorImpl {
    fn coordinate_startup( &self, request: StartupCoordinatorRequest, ) -> StartupCoordinatorResult {
        unimplemented!()
    }
}

pub fn new(fileurlconnection: std::sync::Arc<dyn peerconnections_fileurlconnection::FileUrlConnection>, sftpurlconnection: std::sync::Arc<dyn peerconnections_sftpurlconnection::SftpUrlConnection>) -> std::sync::Arc<dyn StartupCoordinator> {
    Arc::new(StartupCoordinatorImpl { fileurlconnection, sftpurlconnection })
}
