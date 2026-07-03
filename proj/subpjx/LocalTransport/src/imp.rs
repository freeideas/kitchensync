use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;

use crate::api::*;
use peertransportsurface::{
    ConnectedPeerRoot, PeerDirectoryEntry, PeerMetadata, PeerReadChunk, PeerReadHandle,
    PeerTransportError, PeerWriteHandle,
};

struct LocalTransportImpl {
    peertransportsurface: std::sync::Arc<dyn peertransportsurface::PeerTransportSurface>,
}

fn map_io_error(error: std::io::Error) -> PeerTransportError {
    match error.kind() {
        ErrorKind::NotFound => PeerTransportError::NotFound,
        ErrorKind::PermissionDenied => PeerTransportError::PermissionDenied,
        _ => PeerTransportError::IoError,
    }
}

fn ensure_root(path: &Path, create_missing_root: bool) -> Result<(), PeerTransportError> {
    if create_missing_root {
        fs::create_dir_all(path).map_err(map_io_error)?;
    }

    let metadata = fs::metadata(path).map_err(map_io_error)?;
    if metadata.is_dir() {
        Ok(())
    } else {
        Err(PeerTransportError::NotFound)
    }
}

impl LocalTransport for LocalTransportImpl {
    fn connect(
        &self,
        request: LocalConnectionRequest,
    ) -> Result<ConnectedPeerRoot, PeerTransportError> {
        ensure_root(&request.root_path, request.create_missing_root)?;
        Ok(ConnectedPeerRoot {
            handle: Arc::new(request.root_path),
        })
    }

    fn list_dir(
        &self,
        peer: &ConnectedPeerRoot,
        path: &str,
    ) -> Result<Vec<PeerDirectoryEntry>, PeerTransportError> {
        self.peertransportsurface.list_dir(peer, path)
    }

    fn stat(
        &self,
        peer: &ConnectedPeerRoot,
        path: &str,
    ) -> Result<PeerMetadata, PeerTransportError> {
        self.peertransportsurface.stat(peer, path)
    }

    fn open_read(
        &self,
        peer: &ConnectedPeerRoot,
        path: &str,
    ) -> Result<PeerReadHandle, PeerTransportError> {
        self.peertransportsurface.open_read(peer, path)
    }

    fn read(
        &self,
        handle: &mut PeerReadHandle,
        max_bytes: usize,
    ) -> Result<PeerReadChunk, PeerTransportError> {
        self.peertransportsurface.read(handle, max_bytes)
    }

    fn close_read(&self, handle: PeerReadHandle) -> Result<(), PeerTransportError> {
        self.peertransportsurface.close_read(handle)
    }

    fn open_write(
        &self,
        peer: &ConnectedPeerRoot,
        path: &str,
    ) -> Result<PeerWriteHandle, PeerTransportError> {
        self.peertransportsurface.open_write(peer, path)
    }

    fn write(
        &self,
        handle: &mut PeerWriteHandle,
        bytes: &[u8],
    ) -> Result<(), PeerTransportError> {
        self.peertransportsurface.write(handle, bytes)
    }

    fn close_write(&self, handle: PeerWriteHandle) -> Result<(), PeerTransportError> {
        self.peertransportsurface.close_write(handle)
    }

    fn rename(
        &self,
        peer: &ConnectedPeerRoot,
        src: &str,
        dst: &str,
    ) -> Result<(), PeerTransportError> {
        self.peertransportsurface.rename(peer, src, dst)
    }

    fn delete_file(
        &self,
        peer: &ConnectedPeerRoot,
        path: &str,
    ) -> Result<(), PeerTransportError> {
        self.peertransportsurface.delete_file(peer, path)
    }

    fn create_dir(
        &self,
        peer: &ConnectedPeerRoot,
        path: &str,
    ) -> Result<(), PeerTransportError> {
        self.peertransportsurface.create_dir(peer, path)
    }

    fn delete_dir(
        &self,
        peer: &ConnectedPeerRoot,
        path: &str,
    ) -> Result<(), PeerTransportError> {
        self.peertransportsurface.delete_dir(peer, path)
    }

    fn set_mod_time(
        &self,
        peer: &ConnectedPeerRoot,
        path: &str,
        mod_time: SystemTime,
    ) -> Result<(), PeerTransportError> {
        self.peertransportsurface
            .set_mod_time(peer, path, mod_time)
    }
}

pub fn new(
    peertransportsurface: std::sync::Arc<dyn peertransportsurface::PeerTransportSurface>,
) -> std::sync::Arc<dyn LocalTransport> {
    Arc::new(LocalTransportImpl {
        peertransportsurface,
    })
}
