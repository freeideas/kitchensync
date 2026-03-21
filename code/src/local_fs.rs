use crate::filesystem::{EntryMeta, FsError, PeerFs};
use filetime::FileTime;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use tokio::io::AsyncRead;

/// Local filesystem implementation of PeerFs.
pub struct LocalFs {
    root: PathBuf,
}

impl LocalFs {
    pub fn new(root: PathBuf) -> Self {
        LocalFs { root }
    }

    fn full_path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }
}

#[async_trait::async_trait]
impl PeerFs for LocalFs {
    async fn list_dir(&self, path: &str) -> Result<Vec<EntryMeta>, FsError> {
        let full = self.full_path(path);
        let mut entries = Vec::new();
        let mut read_dir = tokio::fs::read_dir(&full).await?;
        while let Some(entry) = read_dir.next_entry().await? {
            let name = entry.file_name().to_string_lossy().to_string();

            // Built-in excludes
            if name == ".kitchensync" || name == ".git" {
                continue;
            }

            let meta = entry.metadata().await?;

            // Skip symlinks, special files
            if meta.file_type().is_symlink() {
                continue;
            }
            if !meta.is_file() && !meta.is_dir() {
                continue;
            }

            let mod_time = meta
                .modified()
                .map(|t| {
                    t.duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_micros() as i64
                })
                .unwrap_or(0);

            let byte_size = if meta.is_dir() { -1 } else { meta.len() as i64 };

            entries.push(EntryMeta {
                name,
                is_dir: meta.is_dir(),
                mod_time,
                byte_size,
            });
        }
        Ok(entries)
    }

    async fn stat(&self, path: &str) -> Result<EntryMeta, FsError> {
        let full = self.full_path(path);
        let meta = tokio::fs::metadata(&full).await?;
        let mod_time = meta
            .modified()
            .map(|t| {
                t.duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_micros() as i64
            })
            .unwrap_or(0);
        let byte_size = if meta.is_dir() { -1 } else { meta.len() as i64 };
        let name = full
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        Ok(EntryMeta {
            name,
            is_dir: meta.is_dir(),
            mod_time,
            byte_size,
        })
    }

    async fn read_file(&self, path: &str) -> Result<Pin<Box<dyn AsyncRead + Send>>, FsError> {
        let full = self.full_path(path);
        let file = tokio::fs::File::open(&full).await?;
        Ok(Box::pin(file))
    }

    async fn write_file(&self, path: &str, mut data: Pin<Box<dyn AsyncRead + Send>>) -> Result<(), FsError> {
        let full = self.full_path(path);
        if let Some(parent) = full.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let mut file = tokio::fs::File::create(&full).await?;
        tokio::io::copy(&mut data, &mut file).await?;
        Ok(())
    }

    async fn rename(&self, src: &str, dst: &str) -> Result<(), FsError> {
        let src_full = self.full_path(src);
        let dst_full = self.full_path(dst);
        if let Some(parent) = dst_full.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::rename(&src_full, &dst_full).await?;
        Ok(())
    }

    async fn delete_file(&self, path: &str) -> Result<(), FsError> {
        let full = self.full_path(path);
        tokio::fs::remove_file(&full).await?;
        Ok(())
    }

    async fn create_dir(&self, path: &str) -> Result<(), FsError> {
        let full = self.full_path(path);
        tokio::fs::create_dir_all(&full).await?;
        Ok(())
    }

    async fn delete_dir(&self, path: &str) -> Result<(), FsError> {
        let full = self.full_path(path);
        tokio::fs::remove_dir(&full).await?;
        Ok(())
    }

    async fn set_mod_time(&self, path: &str, time_us: i64) -> Result<(), FsError> {
        let full = self.full_path(path);
        let secs = time_us / 1_000_000;
        let nsecs = (time_us % 1_000_000) * 1000;
        let ft = FileTime::from_unix_time(secs, nsecs as u32);
        filetime::set_file_mtime(&full, ft)
            .map_err(|e| FsError::Io(e.to_string()))?;
        Ok(())
    }
}
