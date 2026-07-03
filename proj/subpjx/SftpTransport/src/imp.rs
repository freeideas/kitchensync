use std::sync::Arc;
use crate::api::*;

struct SftpTransportImpl {
    peertransportsurface: std::sync::Arc<dyn peertransportsurface::PeerTransportSurface>,
}

impl SftpTransport for SftpTransportImpl {
    fn connect( &self, request: SftpConnectionRequest, ) -> Result<ConnectedPeerRoot, PeerTransportError> {
        unimplemented!()
    }
    fn list_dir( &self, peer: &ConnectedPeerRoot, path: &str, ) -> Result<Vec<PeerDirectoryEntry>, PeerTransportError> {
        unimplemented!()
    }
    fn stat( &self, peer: &ConnectedPeerRoot, path: &str, ) -> Result<PeerMetadata, PeerTransportError> {
        unimplemented!()
    }
    fn open_read( &self, peer: &ConnectedPeerRoot, path: &str, ) -> Result<PeerReadHandle, PeerTransportError> {
        unimplemented!()
    }
    fn read( &self, handle: &mut PeerReadHandle, max_bytes: usize, ) -> Result<PeerReadChunk, PeerTransportError> {
        unimplemented!()
    }
    fn close_read(&self, handle: PeerReadHandle) -> Result<(), PeerTransportError> {
        unimplemented!()
    }
    fn open_write( &self, peer: &ConnectedPeerRoot, path: &str, ) -> Result<PeerWriteHandle, PeerTransportError> {
        unimplemented!()
    }
    fn write( &self, handle: &mut PeerWriteHandle, bytes: &[u8], ) -> Result<(), PeerTransportError> {
        unimplemented!()
    }
    fn close_write(&self, handle: PeerWriteHandle) -> Result<(), PeerTransportError> {
        unimplemented!()
    }
    fn rename( &self, peer: &ConnectedPeerRoot, src: &str, dst: &str, ) -> Result<(), PeerTransportError> {
        unimplemented!()
    }
    fn delete_file( &self, peer: &ConnectedPeerRoot, path: &str, ) -> Result<(), PeerTransportError> {
        unimplemented!()
    }
    fn create_dir( &self, peer: &ConnectedPeerRoot, path: &str, ) -> Result<(), PeerTransportError> {
        unimplemented!()
    }
    fn delete_dir( &self, peer: &ConnectedPeerRoot, path: &str, ) -> Result<(), PeerTransportError> {
        unimplemented!()
    }
    fn set_mod_time( &self, peer: &ConnectedPeerRoot, path: &str, mod_time: SystemTime, ) -> Result<(), PeerTransportError> {
        unimplemented!()
    }
}

pub fn new(peertransportsurface: std::sync::Arc<dyn peertransportsurface::PeerTransportSurface>) -> std::sync::Arc<dyn SftpTransport> {
    Arc::new(SftpTransportImpl { peertransportsurface })
}
