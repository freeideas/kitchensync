use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::peer::{DirEntry, Peer, PeerError};
use crate::timestamp;

pub struct LocalPeer {
    name: String,
    root: PathBuf,
}

impl LocalPeer {
    pub fn new(name: String, root: PathBuf) -> Self {
        Self { name, root }
    }

    fn full_path(&self, rel: &str) -> PathBuf {
        if rel.is_empty() || rel == "." {
            self.root.clone()
        } else {
            self.root.join(rel)
        }
    }
}

impl Peer for LocalPeer {
    fn name(&self) -> &str {
        &self.name
    }

    fn root_path(&self) -> &Path {
        &self.root
    }

    fn list_dir(&self, path: &str) -> Result<Vec<DirEntry>, PeerError> {
        let full = self.full_path(path);
        let entries = fs::read_dir(&full).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => PeerError::NotFound,
            std::io::ErrorKind::PermissionDenied => PeerError::PermissionDenied,
            _ => PeerError::Io(e.to_string()),
        })?;

        let mut result = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| PeerError::Io(e.to_string()))?;
            let ft = entry.file_type().map_err(|e| PeerError::Io(e.to_string()))?;

            // Skip symlinks and special files
            if ft.is_symlink() {
                continue;
            }
            if !ft.is_file() && !ft.is_dir() {
                continue;
            }

            let meta = entry.metadata().map_err(|e| PeerError::Io(e.to_string()))?;
            let name = entry.file_name().to_string_lossy().to_string();
            let mod_time = system_time_to_timestamp(meta.modified().map_err(|e| PeerError::Io(e.to_string()))?);
            let is_dir = ft.is_dir();
            let byte_size = if is_dir { -1 } else { meta.len() as i64 };

            result.push(DirEntry {
                name,
                is_dir,
                mod_time,
                byte_size,
                is_symlink: false,
            });
        }
        Ok(result)
    }

    fn stat(&self, path: &str) -> Result<DirEntry, PeerError> {
        let full = self.full_path(path);
        // Use symlink_metadata to detect symlinks
        let meta = fs::symlink_metadata(&full).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => PeerError::NotFound,
            std::io::ErrorKind::PermissionDenied => PeerError::PermissionDenied,
            _ => PeerError::Io(e.to_string()),
        })?;

        if meta.file_type().is_symlink() {
            return Err(PeerError::NotFound); // treat symlinks as non-existent
        }

        let is_dir = meta.is_dir();
        let mod_time = system_time_to_timestamp(meta.modified().map_err(|e| PeerError::Io(e.to_string()))?);
        let byte_size = if is_dir { -1 } else { meta.len() as i64 };
        let name = full
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        Ok(DirEntry {
            name,
            is_dir,
            mod_time,
            byte_size,
            is_symlink: false,
        })
    }

    fn read_file(&self, path: &str) -> Result<Box<dyn Read + Send>, PeerError> {
        let full = self.full_path(path);
        let file = fs::File::open(&full).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => PeerError::NotFound,
            std::io::ErrorKind::PermissionDenied => PeerError::PermissionDenied,
            _ => PeerError::Io(e.to_string()),
        })?;
        Ok(Box::new(file))
    }

    fn write_file(&self, path: &str, data: &mut dyn Read) -> Result<(), PeerError> {
        let full = self.full_path(path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).map_err(|e| PeerError::Io(e.to_string()))?;
        }
        let mut file = fs::File::create(&full).map_err(|e| match e.kind() {
            std::io::ErrorKind::PermissionDenied => PeerError::PermissionDenied,
            _ => PeerError::Io(e.to_string()),
        })?;
        std::io::copy(data, &mut file).map_err(|e| PeerError::Io(e.to_string()))?;
        Ok(())
    }

    fn rename(&self, src: &str, dst: &str) -> Result<(), PeerError> {
        let src_full = self.full_path(src);
        let dst_full = self.full_path(dst);
        if let Some(parent) = dst_full.parent() {
            fs::create_dir_all(parent).map_err(|e| PeerError::Io(e.to_string()))?;
        }
        fs::rename(&src_full, &dst_full).map_err(|e| PeerError::Io(e.to_string()))?;
        Ok(())
    }

    fn delete_file(&self, path: &str) -> Result<(), PeerError> {
        let full = self.full_path(path);
        fs::remove_file(&full).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => PeerError::NotFound,
            _ => PeerError::Io(e.to_string()),
        })?;
        Ok(())
    }

    fn create_dir(&self, path: &str) -> Result<(), PeerError> {
        let full = self.full_path(path);
        fs::create_dir_all(&full).map_err(|e| PeerError::Io(e.to_string()))?;
        Ok(())
    }

    fn delete_dir(&self, path: &str) -> Result<(), PeerError> {
        let full = self.full_path(path);
        fs::remove_dir(&full).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => PeerError::NotFound,
            _ => PeerError::Io(e.to_string()),
        })?;
        Ok(())
    }
}

fn system_time_to_timestamp(st: std::time::SystemTime) -> String {
    let dt: chrono::DateTime<chrono::Utc> = st.into();
    timestamp::format_timestamp(dt)
}
