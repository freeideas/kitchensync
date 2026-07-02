use std::sync::Arc;
use crate::api::*;

struct SftpTransportOperationsImpl;

impl SftpTransportOperations for SftpTransportOperationsImpl {
    fn list_dir(&self, path: &str) -> Result<Vec<SftpTransportEntry>, SftpTransportError> {
        unimplemented!()
    }
    fn stat(&self, path: &str) -> Result<SftpTransportMetadata, SftpTransportError> {
        unimplemented!()
    }
    fn open_read(&self, path: &str) -> Result<SftpTransportReadHandle, SftpTransportError> {
        unimplemented!()
    }
    fn read( &self, handle: &SftpTransportReadHandle, max_bytes: usize, ) -> Result<SftpTransportReadChunk, SftpTransportError> {
        unimplemented!()
    }
    fn close_read(&self, handle: SftpTransportReadHandle) -> Result<(), SftpTransportError> {
        unimplemented!()
    }
    fn open_write(&self, path: &str) -> Result<SftpTransportWriteHandle, SftpTransportError> {
        unimplemented!()
    }
    fn write( &self, handle: &SftpTransportWriteHandle, bytes: &[u8], ) -> Result<(), SftpTransportError> {
        unimplemented!()
    }
    fn close_write(&self, handle: SftpTransportWriteHandle) -> Result<(), SftpTransportError> {
        unimplemented!()
    }
    fn rename(&self, src: &str, dst: &str) -> Result<(), SftpTransportError> {
        unimplemented!()
    }
    fn delete_file(&self, path: &str) -> Result<(), SftpTransportError> {
        unimplemented!()
    }
    fn create_dir(&self, path: &str) -> Result<(), SftpTransportError> {
        unimplemented!()
    }
    fn delete_dir(&self, path: &str) -> Result<(), SftpTransportError> {
        unimplemented!()
    }
    fn set_mod_time( &self, path: &str, time: SftpTransportModificationTime, ) -> Result<(), SftpTransportError> {
        unimplemented!()
    }
}

pub fn new() -> std::sync::Arc<dyn SftpTransportOperations> {
    Arc::new(SftpTransportOperationsImpl)
}
