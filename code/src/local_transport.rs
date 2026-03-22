use crate::entry::DirEntry;
use crate::transport::Transport;
use filetime::FileTime;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::FileTypeExt;

pub struct LocalTransport {
    root: PathBuf,
}

impl LocalTransport {
    pub fn new(root: &str) -> io::Result<Self> {
        let root = PathBuf::from(root);
        if !root.exists() {
            fs::create_dir_all(&root)?;
        }
        Ok(Self {
            root: root.canonicalize().unwrap_or(root),
        })
    }

    fn full_path(&self, rel_path: &str) -> PathBuf {
        if rel_path.is_empty() || rel_path == "." {
            self.root.clone()
        } else {
            self.root.join(rel_path)
        }
    }
}

fn get_mod_time(meta: &fs::Metadata) -> i64 {
    meta.modified()
        .map(|t| {
            t.duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64
        })
        .unwrap_or(0)
}

impl Transport for LocalTransport {
    fn list_dir(&self, rel_path: &str) -> io::Result<Vec<DirEntry>> {
        let dir = self.full_path(rel_path);
        let mut entries = Vec::new();

        for item in fs::read_dir(&dir)? {
            let item = item?;
            let ft = item.file_type()?;
            let name = item.file_name().to_string_lossy().to_string();

            if name == ".kitchensync" {
                continue;
            }

            // REQ_IGN_007: Skip symlinks
            if ft.is_symlink() {
                continue;
            }

            // REQ_IGN_009: Skip special files (FIFOs, sockets, devices)
            #[cfg(unix)]
            {
                if ft.is_fifo() || ft.is_socket() || ft.is_block_device() || ft.is_char_device() {
                    continue;
                }
            }

            let meta = item.metadata()?;
            entries.push(DirEntry {
                name,
                is_dir: meta.is_dir(),
                mod_time: get_mod_time(&meta),
                size: meta.len(),
            });
        }

        Ok(entries)
    }

    fn read_file(&self, rel_path: &str) -> io::Result<Vec<u8>> {
        fs::read(self.full_path(rel_path))
    }

    fn write_file(&self, rel_path: &str, data: &[u8]) -> io::Result<()> {
        let path = self.full_path(rel_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, data)
    }

    fn stat(&self, rel_path: &str) -> io::Result<Option<DirEntry>> {
        let path = self.full_path(rel_path);
        match fs::metadata(&path) {
            Ok(meta) => {
                let name = Path::new(rel_path)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                Ok(Some(DirEntry {
                    name,
                    is_dir: meta.is_dir(),
                    mod_time: get_mod_time(&meta),
                    size: meta.len(),
                }))
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn delete_file(&self, rel_path: &str) -> io::Result<()> {
        fs::remove_file(self.full_path(rel_path))
    }

    fn remove_dir(&self, rel_path: &str) -> io::Result<()> {
        fs::remove_dir(self.full_path(rel_path))
    }

    fn mkdir(&self, rel_path: &str) -> io::Result<()> {
        fs::create_dir_all(self.full_path(rel_path))
    }

    fn rename(&self, from: &str, to: &str) -> io::Result<()> {
        let from_path = self.full_path(from);
        let to_path = self.full_path(to);
        if let Some(parent) = to_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(from_path, to_path)
    }

    fn set_mod_time(&self, rel_path: &str, mod_time: i64) -> io::Result<()> {
        let path = self.full_path(rel_path);
        let ft = FileTime::from_unix_time(mod_time, 0);
        filetime::set_file_mtime(&path, ft)
    }
}
