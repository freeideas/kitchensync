//! Local filesystem transport: implements `Transport` directly against
//! `std::fs`. See specs/sync.md, section "Peer Transports".

use crate::transport::{Entry, ReadHandle, Result, Transport, TransportError, WriteHandle};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::SystemTime;

#[derive(Debug)]
pub struct LocalTransport {
    root: PathBuf,
}

impl LocalTransport {
    /// Connect to `root` on the local filesystem. If `create` is true, create
    /// the root directory (and any missing parents) when it does not exist.
    /// Otherwise a missing or non-directory root is a NotFound error.
    pub fn connect(root: &str, create: bool) -> Result<LocalTransport> {
        let path = PathBuf::from(root);
        if create {
            fs::create_dir_all(&path)?;
        }
        let meta = fs::metadata(&path).map_err(|_| {
            TransportError::not_found(format!("root does not exist: {root}"))
        })?;
        if !meta.is_dir() {
            return Err(TransportError::not_found(format!("root is not a directory: {root}")));
        }
        Ok(LocalTransport { root: path })
    }

    /// Resolve a slash-separated relative path ("" = root) to an absolute path.
    fn resolve(&self, rel: &str) -> PathBuf {
        if rel.is_empty() {
            return self.root.clone();
        }
        let mut path = self.root.clone();
        for part in rel.split('/') {
            if !part.is_empty() {
                path.push(part);
            }
        }
        path
    }
}

/// Build an `Entry` from a name and its (non-symlink) metadata.
fn entry_from_metadata(name: String, meta: &fs::Metadata) -> Option<Entry> {
    let is_dir = meta.is_dir();
    if !is_dir && !meta.is_file() {
        // Special file (device, FIFO, socket, ...): omit.
        return None;
    }
    let mod_time = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    let byte_size = if is_dir { -1 } else { meta.len() as i64 };
    Some(Entry { name, is_dir, mod_time, byte_size })
}

impl Transport for LocalTransport {
    fn list_dir(&self, path: &str) -> Result<Vec<Entry>> {
        let dir = self.resolve(path);
        let mut out = Vec::new();
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if file_type.is_symlink() {
                // Symlinks are silently omitted.
                continue;
            }
            let Ok(name) = entry.file_name().into_string() else {
                // Non-UTF-8 names are silently skipped.
                continue;
            };
            // A child whose attributes cannot be read makes the whole listing
            // fail as an I/O error, never as "not found": the engine would
            // otherwise treat a missing peer directory as empty, or the child
            // as deleted, and act on that. Seen on macOS exFAT volumes where
            // some non-ASCII names come back from the directory scan but
            // cannot be looked up again (ENOENT).
            let meta = entry.metadata().map_err(|e| {
                TransportError::io(format!("cannot read attributes of {}: {e}", entry.path().display()))
            })?;
            if let Some(e) = entry_from_metadata(name, &meta) {
                out.push(e);
            }
        }
        Ok(out)
    }

    fn stat(&self, path: &str) -> Result<Entry> {
        let full = self.resolve(path);
        let meta = fs::symlink_metadata(&full)?;
        if meta.file_type().is_symlink() {
            return Err(TransportError::not_found(format!("symlink: {path}")));
        }
        let name = full
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        entry_from_metadata(name, &meta)
            .ok_or_else(|| TransportError::not_found(format!("special file: {path}")))
    }

    fn open_read(&self, path: &str) -> Result<Box<dyn ReadHandle>> {
        let file = File::open(self.resolve(path))?;
        Ok(Box::new(LocalReadHandle { file }))
    }

    fn open_write(&self, path: &str) -> Result<Box<dyn WriteHandle>> {
        let full = self.resolve(path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = File::create(&full)?;
        Ok(Box::new(LocalWriteHandle { file }))
    }

    fn rename(&self, src: &str, dst: &str) -> Result<()> {
        let src_full = self.resolve(src);
        let dst_full = self.resolve(dst);
        if fs::symlink_metadata(&dst_full).is_ok() {
            return Err(TransportError::io(format!("destination exists: {dst}")));
        }
        fs::rename(&src_full, &dst_full)?;
        Ok(())
    }

    fn delete_file(&self, path: &str) -> Result<()> {
        fs::remove_file(self.resolve(path))?;
        Ok(())
    }

    fn create_dir(&self, path: &str) -> Result<()> {
        fs::create_dir_all(self.resolve(path))?;
        Ok(())
    }

    fn delete_dir(&self, path: &str) -> Result<()> {
        fs::remove_dir(self.resolve(path))?;
        Ok(())
    }

    fn set_mod_time(&self, path: &str, time: SystemTime) -> Result<()> {
        let full = self.resolve(path);
        filetime::set_file_mtime(&full, filetime::FileTime::from_system_time(time))?;
        Ok(())
    }

}

struct LocalReadHandle {
    file: File,
}

impl ReadHandle for LocalReadHandle {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        Ok(self.file.read(buf)?)
    }
}

struct LocalWriteHandle {
    file: File,
}

impl WriteHandle for LocalWriteHandle {
    fn write_all(&mut self, buf: &[u8]) -> Result<()> {
        self.file.write_all(buf)?;
        Ok(())
    }

    fn close(mut self: Box<Self>) -> Result<()> {
        self.file.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    /// Create a fresh temp dir under the system temp dir with a unique name.
    fn temp_dir(tag: &str) -> PathBuf {
        let unique = format!(
            "kitchensync-local-transport-test-{tag}-{}-{}",
            std::process::id(),
            crate::util::now_micros()
        );
        let dir = std::env::temp_dir().join(unique);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn connect_requires_existing_dir_unless_create() {
        let base = temp_dir("connect");
        let missing = base.join("does-not-exist");
        let missing_str = missing.to_str().unwrap();

        let err = LocalTransport::connect(missing_str, false).unwrap_err();
        assert!(err.is_not_found());

        let t = LocalTransport::connect(missing_str, true).unwrap();
        assert!(missing.is_dir());
        // Reconnecting without create should now succeed.
        let _ = LocalTransport::connect(missing_str, false).unwrap();
        drop(t);

        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn list_dir_returns_files_and_dirs_and_skips_symlinks() {
        let base = temp_dir("list");
        fs::write(base.join("a.txt"), b"hello").unwrap();
        fs::create_dir(base.join("subdir")).unwrap();

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(base.join("a.txt"), base.join("link")).unwrap();
        }

        let t = LocalTransport::connect(base.to_str().unwrap(), false).unwrap();
        let mut entries = t.list_dir("").unwrap();
        entries.sort_by(|a, b| a.name.cmp(&b.name));

        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["a.txt", "subdir"]);

        let file_entry = entries.iter().find(|e| e.name == "a.txt").unwrap();
        assert!(!file_entry.is_dir);
        assert_eq!(file_entry.byte_size, 5);

        let dir_entry = entries.iter().find(|e| e.name == "subdir").unwrap();
        assert!(dir_entry.is_dir);
        assert_eq!(dir_entry.byte_size, -1);

        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn rename_refuses_to_overwrite_existing_dst() {
        let base = temp_dir("rename");
        fs::write(base.join("src.txt"), b"one").unwrap();
        fs::write(base.join("dst.txt"), b"two").unwrap();

        let t = LocalTransport::connect(base.to_str().unwrap(), false).unwrap();
        let err = t.rename("src.txt", "dst.txt").unwrap_err();
        assert_eq!(err.kind, crate::transport::ErrorKind::Io);
        // Neither file should have been touched.
        assert_eq!(fs::read(base.join("src.txt")).unwrap(), b"one");
        assert_eq!(fs::read(base.join("dst.txt")).unwrap(), b"two");

        t.rename("src.txt", "moved.txt").unwrap();
        assert!(base.join("moved.txt").is_file());

        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn set_mod_time_round_trips_to_the_second() {
        let base = temp_dir("mtime");
        fs::write(base.join("f.txt"), b"data").unwrap();

        let t = LocalTransport::connect(base.to_str().unwrap(), false).unwrap();
        let target = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        t.set_mod_time("f.txt", target).unwrap();

        let entry = t.stat("f.txt").unwrap();
        let got_secs = entry
            .mod_time
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert_eq!(got_secs, 1_700_000_000);

        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn open_write_creates_missing_parent_dirs() {
        let base = temp_dir("write-parents");
        let t = LocalTransport::connect(base.to_str().unwrap(), false).unwrap();

        let mut handle = t.open_write("a/b/c.txt").unwrap();
        handle.write_all(b"payload").unwrap();
        handle.close().unwrap();

        let full = base.join("a").join("b").join("c.txt");
        assert_eq!(fs::read(full).unwrap(), b"payload");

        fs::remove_dir_all(&base).ok();
    }
}
