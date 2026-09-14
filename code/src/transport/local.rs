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

/// Portable listing: one `readdir` pass plus one `lstat` per entry.
fn list_dir_portable(dir: &std::path::Path) -> Result<Vec<Entry>> {
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

/// macOS listing through `getattrlistbulk`: name, type, size and modification
/// time for a whole directory in one system call. On an external exFAT drive
/// a per-entry `lstat` costs a disk seek and a trip through the user-space
/// filesystem driver (tens of milliseconds each); the bulk call is around a
/// hundred times faster. Returns None when the call is unavailable so the
/// caller can fall back to the portable listing.
#[cfg(target_os = "macos")]
mod bulk {
    use super::*;
    use std::os::unix::io::AsRawFd;

    #[repr(C)]
    struct AttrList {
        bitmapcount: u16,
        reserved: u16,
        commonattr: u32,
        volattr: u32,
        dirattr: u32,
        fileattr: u32,
        forkattr: u32,
    }

    unsafe extern "C" {
        fn getattrlistbulk(fd: i32, attrs: *mut AttrList, buf: *mut u8, size: usize, options: u64) -> i32;
    }

    const ATTR_BIT_MAP_COUNT: u16 = 5;
    const ATTR_CMN_NAME: u32 = 0x0000_0001;
    const ATTR_CMN_OBJTYPE: u32 = 0x0000_0008;
    const ATTR_CMN_MODTIME: u32 = 0x0000_0400;
    const ATTR_CMN_RETURNED_ATTRS: u32 = 0x8000_0000;
    const ATTR_FILE_DATALENGTH: u32 = 0x0000_0200;
    const VREG: u32 = 1;
    const VDIR: u32 = 2;

    fn u32_at(b: &[u8], off: usize) -> Option<u32> {
        b.get(off..off + 4).map(|x| u32::from_ne_bytes(x.try_into().unwrap()))
    }
    fn i64_at(b: &[u8], off: usize) -> Option<i64> {
        b.get(off..off + 8).map(|x| i64::from_ne_bytes(x.try_into().unwrap()))
    }

    /// Parse one record. Layout: u32 length, attribute_set_t (5 x u32) of the
    /// attributes actually returned, then the returned attributes in bitmap
    /// order: name (attrreference), objtype (u32), modtime (timespec), and for
    /// files the data length (i64).
    fn parse(rec: &[u8]) -> Option<Option<Entry>> {
        let ret_common = u32_at(rec, 4)?;
        let ret_file = u32_at(rec, 16)?;
        let mut off = 4 + 20;
        let mut name: Option<String> = None;
        if ret_common & ATTR_CMN_NAME != 0 {
            let data_off = u32_at(rec, off)? as i32;
            let len = u32_at(rec, off + 4)? as usize;
            let start = (off as i64 + data_off as i64) as usize;
            let bytes = rec.get(start..start + len)?;
            let bytes = bytes.split(|b| *b == 0).next().unwrap_or(&[]);
            name = String::from_utf8(bytes.to_vec()).ok();
            off += 8;
        }
        let mut objtype = 0;
        if ret_common & ATTR_CMN_OBJTYPE != 0 {
            objtype = u32_at(rec, off)?;
            off += 4;
        }
        let mut mod_time = SystemTime::UNIX_EPOCH;
        if ret_common & ATTR_CMN_MODTIME != 0 {
            let sec = i64_at(rec, off)?;
            let nsec = i64_at(rec, off + 8)?;
            off += 16;
            let d = std::time::Duration::new(sec.max(0) as u64, nsec.clamp(0, 999_999_999) as u32);
            mod_time = SystemTime::UNIX_EPOCH + d;
        }
        let mut byte_size = -1;
        if ret_file & ATTR_FILE_DATALENGTH != 0 {
            byte_size = i64_at(rec, off)?;
        }
        // Symlinks, special files and non-UTF-8 names are omitted, exactly as
        // in the portable listing.
        let Some(name) = name else { return Some(None) };
        let entry = match objtype {
            VDIR => Some(Entry { name, is_dir: true, mod_time, byte_size: -1 }),
            VREG => Some(Entry { name, is_dir: false, mod_time, byte_size }),
            _ => None,
        };
        Some(entry)
    }

    pub fn list_dir(dir: &std::path::Path) -> Option<Result<Vec<Entry>>> {
        let f = match File::open(dir) {
            Ok(f) => f,
            Err(e) => return Some(Err(e.into())),
        };
        let mut al = AttrList {
            bitmapcount: ATTR_BIT_MAP_COUNT,
            reserved: 0,
            commonattr: ATTR_CMN_RETURNED_ATTRS | ATTR_CMN_NAME | ATTR_CMN_OBJTYPE | ATTR_CMN_MODTIME,
            volattr: 0,
            dirattr: 0,
            fileattr: ATTR_FILE_DATALENGTH,
            forkattr: 0,
        };
        let mut buf = vec![0u8; 256 * 1024];
        let mut out = Vec::new();
        loop {
            let n = unsafe { getattrlistbulk(f.as_raw_fd(), &mut al, buf.as_mut_ptr(), buf.len(), 0) };
            if n < 0 {
                // Unsupported here (or a transient failure): let the caller
                // use the portable listing, which reports its own errors.
                return None;
            }
            if n == 0 {
                break;
            }
            let mut pos = 0usize;
            for _ in 0..n {
                let Some(len) = u32_at(&buf, pos) else { return None };
                let len = len as usize;
                let Some(rec) = buf.get(pos..pos + len) else { return None };
                match parse(rec) {
                    Some(Some(e)) => out.push(e),
                    Some(None) => {}
                    None => return None,
                }
                pos += len;
            }
        }
        Some(Ok(out))
    }
}

impl Transport for LocalTransport {
    fn list_dir(&self, path: &str) -> Result<Vec<Entry>> {
        let dir = self.resolve(path);
        #[cfg(target_os = "macos")]
        if let Some(r) = bulk::list_dir(&dir) {
            return r;
        }
        list_dir_portable(&dir)
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

    /// The macOS bulk listing must report exactly what the portable listing does.
    #[cfg(target_os = "macos")]
    #[test]
    fn bulk_listing_matches_portable_listing() {
        let base = temp_dir("bulk");
        fs::write(base.join("a.txt"), b"hello").unwrap();
        fs::write(base.join("Bigger.bin"), vec![7u8; 70_000]).unwrap();
        fs::write(base.join("naïve ünïcode.txt"), b"x").unwrap();
        fs::create_dir(base.join("subdir")).unwrap();
        std::os::unix::fs::symlink(base.join("a.txt"), base.join("link")).unwrap();
        let t = filetime::FileTime::from_unix_time(1_700_000_000, 123_456_000);
        filetime::set_file_mtime(base.join("a.txt"), t).unwrap();

        let mut portable = list_dir_portable(&base).unwrap();
        let mut fast = bulk::list_dir(&base).expect("bulk listing available on macOS").unwrap();
        portable.sort_by(|a, b| a.name.cmp(&b.name));
        fast.sort_by(|a, b| a.name.cmp(&b.name));
        let key = |e: &Entry| (e.name.clone(), e.is_dir, e.byte_size, e.mod_time.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_micros());
        let p: Vec<_> = portable.iter().map(key).collect();
        let f: Vec<_> = fast.iter().map(key).collect();
        assert_eq!(p, f);
        assert_eq!(p.len(), 4);
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
