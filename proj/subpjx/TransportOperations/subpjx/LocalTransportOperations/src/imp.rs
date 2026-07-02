use crate::api::*;
use filetime::{set_file_mtime, FileTime};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

struct LocalTransportOperationsImpl {
    next_handle: AtomicU64,
    read_handles: Mutex<HashMap<u64, File>>,
    write_handles: Mutex<HashMap<u64, File>>,
}

fn map_io_error(error: std::io::Error) -> LocalTransportErrorCategory {
    match error.kind() {
        std::io::ErrorKind::NotFound => LocalTransportErrorCategory::NotFound,
        std::io::ErrorKind::PermissionDenied => LocalTransportErrorCategory::PermissionDenied,
        _ => LocalTransportErrorCategory::IoError,
    }
}

fn resolve_path(root: &LocalTransportRoot, path: &str) -> LocalTransportResult<PathBuf> {
    let path = Path::new(path);
    let mut resolved = root.local_peer_root_path.clone();

    for component in path.components() {
        match component {
            Component::Normal(part) => resolved.push(part),
            Component::CurDir => {}
            _ => return Err(LocalTransportErrorCategory::NotFound),
        }
    }

    Ok(resolved)
}

fn parent_components(path: &str) -> LocalTransportResult<Vec<&std::ffi::OsStr>> {
    let mut components = Vec::new();

    for component in Path::new(path).components() {
        match component {
            Component::Normal(part) => components.push(part),
            Component::CurDir => {}
            _ => return Err(LocalTransportErrorCategory::NotFound),
        }
    }

    components.pop();

    Ok(components)
}

fn ensure_existing_parents(root: &LocalTransportRoot, path: &str) -> LocalTransportResult<()> {
    let mut current = root.local_peer_root_path.clone();

    for component in parent_components(path)? {
        current.push(component);
        let metadata = fs::symlink_metadata(&current).map_err(map_io_error)?;
        if !metadata.is_dir() {
            return Err(LocalTransportErrorCategory::NotFound);
        }
    }

    Ok(())
}

fn ensure_parent_dirs(root: &LocalTransportRoot, path: &str) -> LocalTransportResult<()> {
    let mut current = root.local_peer_root_path.clone();

    for component in parent_components(path)? {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if !metadata.is_dir() {
                    return Err(LocalTransportErrorCategory::NotFound);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current).map_err(map_io_error)?;
            }
            Err(error) => return Err(map_io_error(error)),
        }
    }

    Ok(())
}

fn metadata_from_fs(metadata: fs::Metadata) -> LocalTransportResult<LocalTransportMetadata> {
    let entry_type = if metadata.is_file() {
        LocalTransportEntryType::File
    } else if metadata.is_dir() {
        LocalTransportEntryType::Directory
    } else {
        return Err(LocalTransportErrorCategory::NotFound);
    };

    Ok(LocalTransportMetadata {
        modification_time: metadata.modified().map_err(map_io_error)?,
        byte_size: if entry_type == LocalTransportEntryType::Directory {
            -1
        } else {
            i64::try_from(metadata.len()).map_err(|_| LocalTransportErrorCategory::IoError)?
        },
        entry_type,
    })
}

fn existing_metadata(path: &Path) -> LocalTransportResult<LocalTransportMetadata> {
    metadata_from_fs(fs::symlink_metadata(path).map_err(map_io_error)?)
}

fn system_time_to_file_time(time: SystemTime) -> LocalTransportResult<FileTime> {
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => {
            let seconds = i64::try_from(duration.as_secs())
                .map_err(|_| LocalTransportErrorCategory::IoError)?;
            Ok(FileTime::from_unix_time(seconds, duration.subsec_nanos()))
        }
        Err(error) => {
            let duration = error.duration();
            let seconds = i64::try_from(duration.as_secs())
                .map_err(|_| LocalTransportErrorCategory::IoError)?;
            if duration.subsec_nanos() == 0 {
                Ok(FileTime::from_unix_time(-seconds, 0))
            } else {
                Ok(FileTime::from_unix_time(
                    -seconds - 1,
                    1_000_000_000 - duration.subsec_nanos(),
                ))
            }
        }
    }
}

impl LocalTransportOperationsImpl {
    fn allocate_handle(&self) -> u64 {
        self.next_handle.fetch_add(1, Ordering::Relaxed)
    }
}

impl LocalTransportOperations for LocalTransportOperationsImpl {
    fn list_dir(
        &self,
        root: &LocalTransportRoot,
        path: &str,
    ) -> LocalTransportResult<Vec<LocalTransportDirEntry>> {
        ensure_existing_parents(root, path)?;
        let resolved = resolve_path(root, path)?;
        let directory_metadata = fs::symlink_metadata(&resolved).map_err(map_io_error)?;
        if !directory_metadata.is_dir() {
            return Err(LocalTransportErrorCategory::NotFound);
        }

        let mut entries = Vec::new();
        for entry in fs::read_dir(resolved).map_err(map_io_error)? {
            let entry = entry.map_err(map_io_error)?;
            let metadata = fs::symlink_metadata(entry.path()).map_err(map_io_error)?;
            let metadata = match metadata_from_fs(metadata) {
                Ok(metadata) => metadata,
                Err(LocalTransportErrorCategory::NotFound) => continue,
                Err(error) => return Err(error),
            };

            entries.push(LocalTransportDirEntry {
                child_name: entry.file_name().to_string_lossy().into_owned(),
                metadata,
            });
        }

        Ok(entries)
    }

    fn stat(
        &self,
        root: &LocalTransportRoot,
        path: &str,
    ) -> LocalTransportResult<LocalTransportMetadata> {
        ensure_existing_parents(root, path)?;
        let resolved = resolve_path(root, path)?;
        existing_metadata(&resolved)
    }

    fn open_read(
        &self,
        root: &LocalTransportRoot,
        path: &str,
    ) -> LocalTransportResult<LocalTransportReadHandle> {
        ensure_existing_parents(root, path)?;
        let resolved = resolve_path(root, path)?;
        let metadata = fs::symlink_metadata(&resolved).map_err(map_io_error)?;
        if !metadata.is_file() {
            return Err(LocalTransportErrorCategory::NotFound);
        }

        let file = File::open(resolved).map_err(map_io_error)?;
        let handle = self.allocate_handle();
        self.read_handles
            .lock()
            .map_err(|_| LocalTransportErrorCategory::IoError)?
            .insert(handle, file);
        Ok(LocalTransportReadHandle(handle))
    }

    fn read(
        &self,
        handle: LocalTransportReadHandle,
        max_bytes: usize,
    ) -> LocalTransportResult<LocalTransportReadResult> {
        let mut handles = self
            .read_handles
            .lock()
            .map_err(|_| LocalTransportErrorCategory::IoError)?;
        let file = handles
            .get_mut(&handle.0)
            .ok_or(LocalTransportErrorCategory::IoError)?;
        let mut bytes = vec![0; max_bytes];
        let bytes_read = file.read(&mut bytes).map_err(map_io_error)?;
        if bytes_read == 0 && max_bytes != 0 {
            Ok(LocalTransportReadResult::Eof)
        } else {
            bytes.truncate(bytes_read);
            Ok(LocalTransportReadResult::Bytes(bytes))
        }
    }

    fn close_read(&self, handle: LocalTransportReadHandle) -> LocalTransportResult<()> {
        self.read_handles
            .lock()
            .map_err(|_| LocalTransportErrorCategory::IoError)?
            .remove(&handle.0)
            .ok_or(LocalTransportErrorCategory::IoError)?;
        Ok(())
    }

    fn open_write(
        &self,
        root: &LocalTransportRoot,
        path: &str,
    ) -> LocalTransportResult<LocalTransportWriteHandle> {
        ensure_parent_dirs(root, path)?;
        let resolved = resolve_path(root, path)?;
        match fs::symlink_metadata(&resolved) {
            Ok(metadata) => {
                if !metadata.is_file() {
                    return Err(LocalTransportErrorCategory::NotFound);
                }
            }
            Err(error) => {
                if error.kind() != std::io::ErrorKind::NotFound {
                    return Err(map_io_error(error));
                }
            }
        }

        let file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(resolved)
            .map_err(map_io_error)?;
        let handle = self.allocate_handle();
        self.write_handles
            .lock()
            .map_err(|_| LocalTransportErrorCategory::IoError)?
            .insert(handle, file);
        Ok(LocalTransportWriteHandle(handle))
    }

    fn write(
        &self,
        handle: LocalTransportWriteHandle,
        bytes: &[u8],
    ) -> LocalTransportResult<()> {
        let mut handles = self
            .write_handles
            .lock()
            .map_err(|_| LocalTransportErrorCategory::IoError)?;
        let file = handles
            .get_mut(&handle.0)
            .ok_or(LocalTransportErrorCategory::IoError)?;
        file.write_all(bytes).map_err(map_io_error)
    }

    fn close_write(&self, handle: LocalTransportWriteHandle) -> LocalTransportResult<()> {
        let mut handles = self
            .write_handles
            .lock()
            .map_err(|_| LocalTransportErrorCategory::IoError)?;
        let file = handles
            .get_mut(&handle.0)
            .ok_or(LocalTransportErrorCategory::IoError)?;
        file.flush().map_err(map_io_error)?;
        handles.remove(&handle.0);
        Ok(())
    }

    fn rename(
        &self,
        root: &LocalTransportRoot,
        src: &str,
        dst: &str,
    ) -> LocalTransportResult<()> {
        ensure_existing_parents(root, src)?;
        ensure_existing_parents(root, dst)?;
        let resolved_src = resolve_path(root, src)?;
        let resolved_dst = resolve_path(root, dst)?;
        existing_metadata(&resolved_src)?;
        match fs::symlink_metadata(&resolved_dst) {
            Ok(_) => return Err(LocalTransportErrorCategory::IoError),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(map_io_error(error)),
        }

        fs::rename(resolved_src, resolved_dst).map_err(map_io_error)
    }

    fn delete_file(&self, root: &LocalTransportRoot, path: &str) -> LocalTransportResult<()> {
        ensure_existing_parents(root, path)?;
        let resolved = resolve_path(root, path)?;
        let metadata = fs::symlink_metadata(&resolved).map_err(map_io_error)?;
        if !metadata.is_file() {
            return Err(LocalTransportErrorCategory::NotFound);
        }

        fs::remove_file(resolved).map_err(map_io_error)
    }

    fn create_dir(&self, root: &LocalTransportRoot, path: &str) -> LocalTransportResult<()> {
        let mut current = root.local_peer_root_path.clone();

        for component in Path::new(path).components() {
            match component {
                Component::Normal(part) => {
                    current.push(part);
                    match fs::symlink_metadata(&current) {
                        Ok(metadata) => {
                            if !metadata.is_dir() {
                                return Err(LocalTransportErrorCategory::NotFound);
                            }
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                            fs::create_dir(&current).map_err(map_io_error)?;
                        }
                        Err(error) => return Err(map_io_error(error)),
                    }
                }
                Component::CurDir => {}
                _ => return Err(LocalTransportErrorCategory::NotFound),
            }
        }

        Ok(())
    }

    fn delete_dir(&self, root: &LocalTransportRoot, path: &str) -> LocalTransportResult<()> {
        ensure_existing_parents(root, path)?;
        let resolved = resolve_path(root, path)?;
        let metadata = fs::symlink_metadata(&resolved).map_err(map_io_error)?;
        if !metadata.is_dir() {
            return Err(LocalTransportErrorCategory::NotFound);
        }

        fs::remove_dir(resolved).map_err(map_io_error)
    }

    fn set_mod_time(
        &self,
        root: &LocalTransportRoot,
        path: &str,
        time: SystemTime,
    ) -> LocalTransportResult<()> {
        ensure_existing_parents(root, path)?;
        let resolved = resolve_path(root, path)?;
        existing_metadata(&resolved)?;
        set_file_mtime(resolved, system_time_to_file_time(time)?).map_err(map_io_error)
    }
}

pub fn new() -> std::sync::Arc<dyn LocalTransportOperations> {
    Arc::new(LocalTransportOperationsImpl {
        next_handle: AtomicU64::new(1),
        read_handles: Mutex::new(HashMap::new()),
        write_handles: Mutex::new(HashMap::new()),
    })
}
