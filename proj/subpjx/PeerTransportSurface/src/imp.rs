use crate::api::*;
use std::fs::{self, File};
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

struct PeerTransportSurfaceImpl;

fn map_io_error(error: std::io::Error) -> PeerTransportError {
    match error.kind() {
        ErrorKind::NotFound => PeerTransportError::NotFound,
        ErrorKind::PermissionDenied => PeerTransportError::PermissionDenied,
        _ => PeerTransportError::IoError,
    }
}

fn root_path(peer: &ConnectedPeerRoot) -> Result<&Path, PeerTransportError> {
    peer.handle
        .downcast_ref::<PathBuf>()
        .map(PathBuf::as_path)
        .ok_or(PeerTransportError::IoError)
}

fn peer_path(peer: &ConnectedPeerRoot, path: &str) -> Result<PathBuf, PeerTransportError> {
    Ok(root_path(peer)?.join(path))
}

fn metadata_for(path: &Path) -> Result<PeerMetadata, PeerTransportError> {
    let file_type = fs::symlink_metadata(path)
        .map_err(map_io_error)?
        .file_type();

    if !file_type.is_file() && !file_type.is_dir() {
        return Err(PeerTransportError::NotFound);
    }

    let metadata = fs::metadata(path).map_err(map_io_error)?;
    let is_dir = metadata.is_dir();
    Ok(PeerMetadata {
        is_dir,
        mod_time: metadata.modified().map_err(map_io_error)?,
        byte_size: if is_dir { -1 } else { metadata.len() as i64 },
    })
}

fn read_file(handle: &mut PeerReadHandle) -> Result<&mut File, PeerTransportError> {
    handle
        .handle
        .downcast_mut::<File>()
        .ok_or(PeerTransportError::IoError)
}

fn write_file(handle: &mut PeerWriteHandle) -> Result<&mut File, PeerTransportError> {
    handle
        .handle
        .downcast_mut::<File>()
        .ok_or(PeerTransportError::IoError)
}

#[cfg(unix)]
fn set_path_mod_time(path: &Path, mod_time: SystemTime) -> Result<(), PeerTransportError> {
    use std::ffi::CString;
    use std::os::raw::{c_char, c_int, c_long};
    use std::os::unix::ffi::OsStrExt;

    #[repr(C)]
    struct Timespec {
        tv_sec: c_long,
        tv_nsec: c_long,
    }

    unsafe extern "C" {
        fn utimensat(
            dirfd: c_int,
            pathname: *const c_char,
            times: *const Timespec,
            flags: c_int,
        ) -> c_int;
    }

    const AT_FDCWD: c_int = -100;
    const UTIME_OMIT: c_long = 1_073_741_822;

    let duration = mod_time
        .duration_since(UNIX_EPOCH)
        .map_err(|_| PeerTransportError::IoError)?;
    let path = CString::new(path.as_os_str().as_bytes()).map_err(|_| PeerTransportError::IoError)?;
    let times = [
        Timespec {
            tv_sec: 0,
            tv_nsec: UTIME_OMIT,
        },
        Timespec {
            tv_sec: duration.as_secs().try_into().map_err(|_| PeerTransportError::IoError)?,
            tv_nsec: duration.subsec_nanos().into(),
        },
    ];

    if unsafe { utimensat(AT_FDCWD, path.as_ptr(), times.as_ptr(), 0) } == 0 {
        Ok(())
    } else {
        Err(map_io_error(std::io::Error::last_os_error()))
    }
}

#[cfg(not(unix))]
fn set_path_mod_time(_path: &Path, _mod_time: SystemTime) -> Result<(), PeerTransportError> {
    Err(PeerTransportError::IoError)
}

impl PeerTransportSurface for PeerTransportSurfaceImpl {
    fn list_dir( &self, peer: &ConnectedPeerRoot, path: &str, ) -> Result<Vec<PeerDirectoryEntry>, PeerTransportError> {
        let mut entries = Vec::new();

        for entry_result in fs::read_dir(peer_path(peer, path)?).map_err(map_io_error)? {
            let entry = entry_result.map_err(map_io_error)?;
            let file_type = entry.file_type().map_err(map_io_error)?;

            if !file_type.is_file() && !file_type.is_dir() {
                continue;
            }

            let metadata = entry.metadata().map_err(map_io_error)?;
            let is_dir = metadata.is_dir();
            entries.push(PeerDirectoryEntry {
                child_name: entry
                    .file_name()
                    .into_string()
                    .map_err(|_| PeerTransportError::IoError)?,
                is_dir,
                mod_time: metadata.modified().map_err(map_io_error)?,
                byte_size: if is_dir { -1 } else { metadata.len() as i64 },
            });
        }

        Ok(entries)
    }
    fn stat( &self, peer: &ConnectedPeerRoot, path: &str, ) -> Result<PeerMetadata, PeerTransportError> {
        metadata_for(&peer_path(peer, path)?)
    }
    fn open_read( &self, peer: &ConnectedPeerRoot, path: &str, ) -> Result<PeerReadHandle, PeerTransportError> {
        let path = peer_path(peer, path)?;
        let metadata = metadata_for(&path)?;
        if !metadata.is_dir {
            Ok(PeerReadHandle {
                handle: Box::new(File::open(path).map_err(map_io_error)?),
            })
        } else {
            Err(PeerTransportError::NotFound)
        }
    }
    fn read( &self, handle: &mut PeerReadHandle, max_bytes: usize, ) -> Result<PeerReadChunk, PeerTransportError> {
        let mut bytes = vec![0; max_bytes];
        let count = read_file(handle)?
            .read(&mut bytes)
            .map_err(map_io_error)?;

        if count == 0 {
            Ok(PeerReadChunk::Eof)
        } else {
            bytes.truncate(count);
            Ok(PeerReadChunk::Bytes(bytes))
        }
    }
    fn close_read(&self, handle: PeerReadHandle) -> Result<(), PeerTransportError> {
        if handle.handle.is::<File>() {
            Ok(())
        } else {
            Err(PeerTransportError::IoError)
        }
    }
    fn open_write( &self, peer: &ConnectedPeerRoot, path: &str, ) -> Result<PeerWriteHandle, PeerTransportError> {
        let path = peer_path(peer, path)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(map_io_error)?;
        }

        Ok(PeerWriteHandle {
            handle: Box::new(File::create(path).map_err(map_io_error)?),
        })
    }
    fn write( &self, handle: &mut PeerWriteHandle, bytes: &[u8], ) -> Result<(), PeerTransportError> {
        write_file(handle)?.write_all(bytes).map_err(map_io_error)
    }
    fn close_write(&self, handle: PeerWriteHandle) -> Result<(), PeerTransportError> {
        let file = handle
            .handle
            .downcast::<File>()
            .map_err(|_| PeerTransportError::IoError)?;
        file.sync_all().map_err(map_io_error)
    }
    fn rename( &self, peer: &ConnectedPeerRoot, src: &str, dst: &str, ) -> Result<(), PeerTransportError> {
        let dst_path = peer_path(peer, dst)?;
        if dst_path.try_exists().map_err(map_io_error)? {
            return Err(PeerTransportError::IoError);
        }

        fs::rename(peer_path(peer, src)?, dst_path).map_err(map_io_error)
    }
    fn delete_file( &self, peer: &ConnectedPeerRoot, path: &str, ) -> Result<(), PeerTransportError> {
        let path = peer_path(peer, path)?;
        if metadata_for(&path)?.is_dir {
            return Err(PeerTransportError::NotFound);
        }

        fs::remove_file(path).map_err(map_io_error)
    }
    fn create_dir( &self, peer: &ConnectedPeerRoot, path: &str, ) -> Result<(), PeerTransportError> {
        fs::create_dir_all(peer_path(peer, path)?).map_err(map_io_error)
    }
    fn delete_dir( &self, peer: &ConnectedPeerRoot, path: &str, ) -> Result<(), PeerTransportError> {
        fs::remove_dir(peer_path(peer, path)?).map_err(map_io_error)
    }
    fn set_mod_time( &self, peer: &ConnectedPeerRoot, path: &str, mod_time: SystemTime, ) -> Result<(), PeerTransportError> {
        let path = peer_path(peer, path)?;
        metadata_for(&path)?;
        set_path_mod_time(&path, mod_time)
    }
}

pub fn new() -> std::sync::Arc<dyn PeerTransportSurface> {
    Arc::new(PeerTransportSurfaceImpl)
}
