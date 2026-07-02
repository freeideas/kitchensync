use std::sync::Arc;
use crate::api::*;

struct TransportOperationsImpl {
    localtransportoperations: std::sync::Arc<dyn transportoperations_localtransportoperations::LocalTransportOperations>,
    sftptransportoperations: std::sync::Arc<dyn transportoperations_sftptransportoperations::SftpTransportOperations>,
}

impl TransportOperations for TransportOperationsImpl {
    fn list_dir( &self, peer: &TransportPeerHandle, path: &str, ) -> Result<Vec<TransportDirectoryEntry>, TransportError> {
        unimplemented!()
    }
    fn stat( &self, peer: &TransportPeerHandle, path: &str, ) -> Result<TransportMetadata, TransportError> {
        unimplemented!()
    }
    fn open_read( &self, peer: &TransportPeerHandle, path: &str, ) -> Result<TransportReadHandle, TransportError> {
        unimplemented!()
    }
    fn read( &self, handle: &TransportReadHandle, max_bytes: usize, ) -> Result<TransportReadResult, TransportError> {
        unimplemented!()
    }
    fn close_read(&self, handle: TransportReadHandle) -> Result<(), TransportError> {
        unimplemented!()
    }
    fn open_write( &self, peer: &TransportPeerHandle, path: &str, ) -> Result<TransportWriteHandle, TransportError> {
        unimplemented!()
    }
    fn write(&self, handle: &TransportWriteHandle, bytes: &[u8]) -> Result<(), TransportError> {
        unimplemented!()
    }
    fn close_write(&self, handle: TransportWriteHandle) -> Result<(), TransportError> {
        unimplemented!()
    }
    fn rename( &self, peer: &TransportPeerHandle, src: &str, dst: &str, ) -> Result<(), TransportError> {
        unimplemented!()
    }
    fn delete_file(&self, peer: &TransportPeerHandle, path: &str) -> Result<(), TransportError> {
        unimplemented!()
    }
    fn create_dir(&self, peer: &TransportPeerHandle, path: &str) -> Result<(), TransportError> {
        unimplemented!()
    }
    fn delete_dir(&self, peer: &TransportPeerHandle, path: &str) -> Result<(), TransportError> {
        unimplemented!()
    }
    fn set_mod_time( &self, peer: &TransportPeerHandle, path: &str, time: SystemTime, ) -> Result<(), TransportError> {
        unimplemented!()
    }
}

pub fn new(localtransportoperations: std::sync::Arc<dyn transportoperations_localtransportoperations::LocalTransportOperations>, sftptransportoperations: std::sync::Arc<dyn transportoperations_sftptransportoperations::SftpTransportOperations>) -> std::sync::Arc<dyn TransportOperations> {
    Arc::new(TransportOperationsImpl { localtransportoperations, sftptransportoperations })
}
