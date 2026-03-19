use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;

use crate::peer::{DirEntry, FileStat, PeerError, PeerFs};
use crate::timestamp;

pub struct LocalConnection {
    root: PathBuf,
}

impl LocalConnection {
    pub fn new(root: &str) -> Result<Self, PeerError> {
        let p = PathBuf::from(root);
        if !p.exists() {
            return Err(PeerError::NotFound(format!("Root path not found: {}", root)));
        }
        Ok(LocalConnection {
            root: p.canonicalize().unwrap_or(p),
        })
    }

    fn full_path(&self, path: &str) -> PathBuf {
        if path.is_empty() || path == "." || path == "./" {
            self.root.clone()
        } else {
            self.root.join(path.trim_start_matches('/'))
        }
    }
}

impl PeerFs for LocalConnection {
    fn list_dir(&self, path: &str) -> Result<Vec<DirEntry>, PeerError> {
        let full = self.full_path(path);
        let rd = fs::read_dir(&full).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                PeerError::NotFound(format!("{}", full.display()))
            } else {
                PeerError::from(e)
            }
        })?;

        let mut entries = Vec::new();
        for entry in rd {
            let entry = entry.map_err(PeerError::from)?;
            let name = entry.file_name().to_string_lossy().to_string();

            // Built-in excludes
            if name == ".kitchensync" || name == ".git" {
                continue;
            }

            let ft = entry.file_type().map_err(PeerError::from)?;

            // Skip symlinks, special files
            if ft.is_symlink() {
                continue;
            }
            if !ft.is_file() && !ft.is_dir() {
                continue;
            }

            let meta = entry.metadata().map_err(PeerError::from)?;
            let mod_time = meta
                .modified()
                .map(|t| timestamp::from_system_time(t))
                .unwrap_or_else(|_| timestamp::now());

            let byte_size = if ft.is_dir() { -1 } else { meta.len() as i64 };

            entries.push(DirEntry {
                name,
                is_dir: ft.is_dir(),
                mod_time,
                byte_size,
            });
        }
        Ok(entries)
    }

    fn stat(&self, path: &str) -> Result<Option<FileStat>, PeerError> {
        let full = self.full_path(path);
        match fs::metadata(&full) {
            Ok(meta) => {
                let mod_time = meta
                    .modified()
                    .map(|t| timestamp::from_system_time(t))
                    .unwrap_or_else(|_| timestamp::now());
                Ok(Some(FileStat {
                    mod_time,
                    byte_size: if meta.is_dir() {
                        -1
                    } else {
                        meta.len() as i64
                    },
                    is_dir: meta.is_dir(),
                }))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(PeerError::from(e)),
        }
    }

    fn read_file_to(&self, path: &str, writer: &mut dyn Write) -> Result<u64, PeerError> {
        let full = self.full_path(path);
        let mut file = fs::File::open(&full).map_err(PeerError::from)?;
        let copied = std::io::copy(&mut file, writer).map_err(PeerError::from)?;
        Ok(copied)
    }

    fn write_file_from(&self, path: &str, reader: &mut dyn Read) -> Result<(), PeerError> {
        let full = self.full_path(path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).map_err(PeerError::from)?;
        }
        let mut file = fs::File::create(&full).map_err(PeerError::from)?;
        std::io::copy(reader, &mut file).map_err(PeerError::from)?;
        file.flush().map_err(PeerError::from)?;
        Ok(())
    }

    fn rename(&self, src: &str, dst: &str) -> Result<(), PeerError> {
        let src_full = self.full_path(src);
        let dst_full = self.full_path(dst);
        if let Some(parent) = dst_full.parent() {
            fs::create_dir_all(parent).map_err(PeerError::from)?;
        }
        fs::rename(&src_full, &dst_full).map_err(PeerError::from)?;
        Ok(())
    }

    fn delete_file(&self, path: &str) -> Result<(), PeerError> {
        let full = self.full_path(path);
        fs::remove_file(&full).map_err(PeerError::from)?;
        Ok(())
    }

    fn create_dir(&self, path: &str) -> Result<(), PeerError> {
        let full = self.full_path(path);
        fs::create_dir_all(&full).map_err(PeerError::from)?;
        Ok(())
    }

    fn delete_dir(&self, path: &str) -> Result<(), PeerError> {
        let full = self.full_path(path);
        fs::remove_dir(&full).map_err(PeerError::from)?;
        Ok(())
    }

    fn set_mod_time(&self, path: &str, time: &str) -> Result<(), PeerError> {
        let full = self.full_path(path);
        if let Some(st) = timestamp::to_system_time(time) {
            let ft = filetime::FileTime::from_system_time(st);
            filetime::set_file_mtime(&full, ft).map_err(PeerError::from)?;
        }
        Ok(())
    }
}
