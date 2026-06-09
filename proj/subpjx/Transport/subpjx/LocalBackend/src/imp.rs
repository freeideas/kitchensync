use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;
use crate::api::*;

impl std::fmt::Debug for LocalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LocalError::NotFound => write!(f, "LocalError::NotFound"),
            LocalError::PermissionDenied => write!(f, "LocalError::PermissionDenied"),
            LocalError::Io => write!(f, "LocalError::Io"),
        }
    }
}

fn map_io_err(e: std::io::Error) -> LocalError {
    match e.kind() {
        std::io::ErrorKind::NotFound => LocalError::NotFound,
        std::io::ErrorKind::PermissionDenied => LocalError::PermissionDenied,
        _ => LocalError::Io,
    }
}

fn url_to_path(url: &str) -> std::path::PathBuf {
    let after_scheme = url.strip_prefix("file://").unwrap_or(url);
    let path_str = if after_scheme.starts_with('/') {
        after_scheme
    } else {
        after_scheme.find('/').map(|i| &after_scheme[i..]).unwrap_or("")
    };
    std::path::PathBuf::from(path_str)
}

fn resolve(root: &str, path: &str) -> std::path::PathBuf {
    url_to_path(root).join(path.trim_start_matches('/'))
}

struct LocalBackendImpl {
    next_id: AtomicU64,
    read_handles: Mutex<HashMap<u64, File>>,
    write_handles: Mutex<HashMap<u64, File>>,
}

impl LocalBackend for LocalBackendImpl {
    fn open_root(&self, root: &str, dry_run: bool) -> Result<(), LocalError> {
        let path = url_to_path(root);
        if dry_run {
            if path.is_dir() { Ok(()) } else { Err(LocalError::NotFound) }
        } else {
            fs::create_dir_all(&path).map_err(map_io_err)
        }
    }

    fn list_dir(&self, root: &str, path: &str) -> Result<Vec<DirEntry>, LocalError> {
        let dir_path = resolve(root, path);
        let rd = fs::read_dir(&dir_path).map_err(map_io_err)?;
        let mut result = Vec::new();
        for entry in rd {
            let entry = entry.map_err(map_io_err)?;
            let file_type = match entry.file_type() {
                Ok(ft) => ft,
                Err(_) => continue,
            };
            // Omit symlinks and special files; keep only regular files and dirs (022.15)
            if !file_type.is_file() && !file_type.is_dir() {
                continue;
            }
            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let name = entry.file_name().to_string_lossy().into_owned();
            let is_dir = file_type.is_dir();
            let mod_time = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            let byte_size = if is_dir { -1 } else { meta.len() as i64 };
            result.push(DirEntry { name, is_dir, mod_time, byte_size });
        }
        Ok(result)
    }

    fn stat(&self, root: &str, path: &str) -> Result<Stat, LocalError> {
        let full = resolve(root, path);
        // symlink_metadata does not follow symlinks, so symlinks surface as neither file nor dir
        let meta = full.symlink_metadata().map_err(map_io_err)?;
        if !meta.is_file() && !meta.is_dir() {
            return Err(LocalError::NotFound); // symlink or special file (022.16)
        }
        let is_dir = meta.is_dir();
        let mod_time = meta.modified().map_err(map_io_err)?;
        let byte_size = if is_dir { -1 } else { meta.len() as i64 };
        Ok(Stat { mod_time, byte_size, is_dir })
    }

    fn open_read(&self, root: &str, path: &str) -> Result<ReadHandle, LocalError> {
        let full = resolve(root, path);
        let file = File::open(&full).map_err(map_io_err)?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.read_handles.lock().unwrap().insert(id, file);
        Ok(ReadHandle(id))
    }

    fn read(&self, handle: &ReadHandle, max_bytes: usize) -> Result<Option<Vec<u8>>, LocalError> {
        let mut buf = vec![0u8; max_bytes];
        let mut guards = self.read_handles.lock().unwrap();
        let file = guards.get_mut(&handle.0).ok_or(LocalError::Io)?;
        let n = file.read(&mut buf).map_err(map_io_err)?;
        if n == 0 {
            Ok(None)
        } else {
            buf.truncate(n);
            Ok(Some(buf))
        }
    }

    fn close_read(&self, handle: ReadHandle) -> Result<(), LocalError> {
        self.read_handles.lock().unwrap().remove(&handle.0);
        Ok(())
    }

    fn open_write(&self, root: &str, path: &str) -> Result<WriteHandle, LocalError> {
        let full = resolve(root, path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).map_err(map_io_err)?;
        }
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&full)
            .map_err(map_io_err)?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.write_handles.lock().unwrap().insert(id, file);
        Ok(WriteHandle(id))
    }

    fn write(&self, handle: &WriteHandle, bytes: &[u8]) -> Result<(), LocalError> {
        let mut guards = self.write_handles.lock().unwrap();
        let file = guards.get_mut(&handle.0).ok_or(LocalError::Io)?;
        file.write_all(bytes).map_err(map_io_err)
    }

    fn close_write(&self, handle: WriteHandle) -> Result<(), LocalError> {
        if let Some(mut f) = self.write_handles.lock().unwrap().remove(&handle.0) {
            f.flush().map_err(map_io_err)?;
        }
        Ok(())
    }

    fn create_dir(&self, root: &str, path: &str) -> Result<(), LocalError> {
        fs::create_dir_all(resolve(root, path)).map_err(map_io_err)
    }

    fn rename(&self, root: &str, src: &str, dst: &str) -> Result<(), LocalError> {
        let src_path = resolve(root, src);
        let dst_path = resolve(root, dst);
        // Fail when dst already exists; never overwrite (022.11)
        if dst_path.exists() {
            return Err(LocalError::Io);
        }
        fs::rename(&src_path, &dst_path).map_err(map_io_err)
    }

    fn delete_file(&self, root: &str, path: &str) -> Result<(), LocalError> {
        fs::remove_file(resolve(root, path)).map_err(map_io_err)
    }

    fn delete_dir(&self, root: &str, path: &str) -> Result<(), LocalError> {
        fs::remove_dir(resolve(root, path)).map_err(map_io_err)
    }

    fn set_mod_time(&self, root: &str, path: &str, time: SystemTime) -> Result<(), LocalError> {
        let full = resolve(root, path);
        let file = OpenOptions::new().read(true).open(&full).map_err(map_io_err)?;
        let times = fs::FileTimes::new().set_modified(time);
        file.set_times(times).map_err(map_io_err)
    }
}

pub fn new() -> std::sync::Arc<dyn LocalBackend> {
    Arc::new(LocalBackendImpl {
        next_id: AtomicU64::new(0),
        read_handles: Mutex::new(HashMap::new()),
        write_handles: Mutex::new(HashMap::new()),
    })
}
