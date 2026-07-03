use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use peertransportsurface::{
    new, ConnectedPeerRoot, PeerDirectoryEntry, PeerReadChunk, PeerTransportError,
    PeerTransportSurface,
};

static NEXT_TEST_ROOT: AtomicUsize = AtomicUsize::new(0);

struct TestRoot {
    path: PathBuf,
}

impl TestRoot {
    fn new(name: &str) -> Self {
        let id = NEXT_TEST_ROOT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "kitchensync_peertransportsurface_{name}_{}_{}",
            std::process::id(),
            id
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn peer(&self) -> ConnectedPeerRoot {
        ConnectedPeerRoot {
            handle: Arc::new(self.path.clone()),
        }
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn sort_entries(entries: &mut [PeerDirectoryEntry]) {
    entries.sort_by(|left, right| left.child_name.cmp(&right.child_name));
}

fn assert_not_found<T>(result: Result<T, PeerTransportError>) {
    assert!(matches!(result, Err(PeerTransportError::NotFound)));
}

fn read_all(
    subject: &dyn PeerTransportSurface,
    peer: &ConnectedPeerRoot,
    path: &str,
) -> Vec<u8> {
    let mut handle = subject.open_read(peer, path).unwrap();
    let mut bytes = Vec::new();

    loop {
        match subject.read(&mut handle, 3).unwrap() {
            PeerReadChunk::Bytes(chunk) => {
                assert!(chunk.len() <= 3);
                bytes.extend(chunk);
            }
            PeerReadChunk::Eof => break,
        }
    }

    subject.close_read(handle).unwrap();
    bytes
}

#[test]
fn list_dir_returns_only_immediate_regular_children_with_metadata() {
    let root = TestRoot::new("list_dir");
    fs::create_dir_all(root.path().join("folder").join("nested")).unwrap();
    fs::write(root.path().join("folder").join("MiXeD Name.txt"), b"abc").unwrap();
    fs::write(
        root.path().join("folder").join("nested").join("deep.txt"),
        b"not listed",
    )
    .unwrap();
    let subject = new();
    let peer = root.peer();

    let mut entries = subject.list_dir(&peer, "folder").unwrap();
    sort_entries(&mut entries);

    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].child_name, "MiXeD Name.txt");
    assert!(!entries[0].is_dir);
    assert_eq!(entries[0].byte_size, 3);
    assert_eq!(entries[1].child_name, "nested");
    assert!(entries[1].is_dir);
    assert_eq!(entries[1].byte_size, -1);
}

#[test]
fn stat_reports_regular_file_and_directory_metadata() {
    let root = TestRoot::new("stat");
    fs::create_dir_all(root.path().join("folder")).unwrap();
    fs::write(root.path().join("folder").join("file.bin"), b"abcdef").unwrap();
    let subject = new();
    let peer = root.peer();

    let file = subject.stat(&peer, "folder/file.bin").unwrap();
    let directory = subject.stat(&peer, "folder").unwrap();

    assert!(!file.is_dir);
    assert_eq!(file.byte_size, 6);
    assert!(directory.is_dir);
    assert_eq!(directory.byte_size, -1);
}

#[test]
fn missing_paths_are_reported_as_not_found_for_lookup_and_read() {
    let root = TestRoot::new("missing_paths");
    let subject = new();
    let peer = root.peer();

    assert_not_found(subject.list_dir(&peer, "missing"));
    assert_not_found(subject.stat(&peer, "missing"));
    assert_not_found(subject.open_read(&peer, "missing"));
}

#[test]
fn missing_paths_are_reported_as_not_found_for_mutating_operations() {
    let root = TestRoot::new("missing_mutations");
    let subject = new();
    let peer = root.peer();
    let mod_time = UNIX_EPOCH + Duration::from_secs(1_700_000_000);

    assert_not_found(subject.rename(&peer, "missing", "moved"));
    assert_not_found(subject.delete_file(&peer, "missing"));
    assert_not_found(subject.delete_dir(&peer, "missing"));
    assert_not_found(subject.set_mod_time(&peer, "missing", mod_time));
}

#[cfg(unix)]
#[test]
fn list_dir_omits_symlinks_and_stat_reports_them_as_not_found() {
    use std::os::unix::fs::symlink;

    let root = TestRoot::new("symlink");
    fs::create_dir_all(root.path().join("folder")).unwrap();
    fs::write(root.path().join("folder").join("target.txt"), b"target").unwrap();
    symlink(
        root.path().join("folder").join("target.txt"),
        root.path().join("folder").join("link.txt"),
    )
    .unwrap();
    let subject = new();
    let peer = root.peer();

    let entries = subject.list_dir(&peer, "folder").unwrap();

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].child_name, "target.txt");
    assert_not_found(subject.stat(&peer, "folder/link.txt"));
}

#[test]
fn open_read_streams_chunks_no_larger_than_requested_then_eof() {
    let root = TestRoot::new("open_read");
    fs::write(root.path().join("file.bin"), b"abcdef").unwrap();
    let subject = new();
    let peer = root.peer();
    let mut handle = subject.open_read(&peer, "file.bin").unwrap();

    assert_eq!(
        subject.read(&mut handle, 2).unwrap(),
        PeerReadChunk::Bytes(b"ab".to_vec())
    );
    assert_eq!(
        subject.read(&mut handle, 2).unwrap(),
        PeerReadChunk::Bytes(b"cd".to_vec())
    );
    assert_eq!(
        subject.read(&mut handle, 2).unwrap(),
        PeerReadChunk::Bytes(b"ef".to_vec())
    );
    assert_eq!(subject.read(&mut handle, 2).unwrap(), PeerReadChunk::Eof);
    subject.close_read(handle).unwrap();
}

#[test]
fn open_write_creates_parents_and_close_write_finalizes_bytes() {
    let root = TestRoot::new("open_write");
    let subject = new();
    let peer = root.peer();

    let mut handle = subject.open_write(&peer, "parent/child/file.txt").unwrap();
    subject.write(&mut handle, b"one").unwrap();
    subject.write(&mut handle, b"-two").unwrap();
    subject.close_write(handle).unwrap();

    assert_eq!(
        read_all(subject.as_ref(), &peer, "parent/child/file.txt"),
        b"one-two"
    );
}

#[test]
fn rename_moves_a_file_to_a_non_existing_destination() {
    let root = TestRoot::new("rename");
    fs::create_dir_all(root.path().join("dst")).unwrap();
    fs::write(root.path().join("src.txt"), b"contents").unwrap();
    let subject = new();
    let peer = root.peer();

    subject.rename(&peer, "src.txt", "dst/moved.txt").unwrap();

    assert_not_found(subject.stat(&peer, "src.txt"));
    assert_eq!(read_all(subject.as_ref(), &peer, "dst/moved.txt"), b"contents");
}

#[test]
fn delete_file_removes_a_file() {
    let root = TestRoot::new("delete_file");
    fs::write(root.path().join("remove.txt"), b"remove").unwrap();
    let subject = new();
    let peer = root.peer();

    subject.delete_file(&peer, "remove.txt").unwrap();

    assert_not_found(subject.stat(&peer, "remove.txt"));
}

#[test]
fn delete_file_does_not_remove_a_directory() {
    let root = TestRoot::new("delete_file_directory");
    fs::create_dir_all(root.path().join("directory")).unwrap();
    let subject = new();
    let peer = root.peer();

    assert!(subject.delete_file(&peer, "directory").is_err());

    let metadata = subject.stat(&peer, "directory").unwrap();
    assert!(metadata.is_dir);
    assert_eq!(metadata.byte_size, -1);
}

#[test]
fn create_dir_creates_missing_parents() {
    let root = TestRoot::new("create_dir");
    let subject = new();
    let peer = root.peer();

    subject.create_dir(&peer, "a/b/c").unwrap();

    let metadata = subject.stat(&peer, "a/b/c").unwrap();
    assert!(metadata.is_dir);
    assert_eq!(metadata.byte_size, -1);
}

#[test]
fn delete_dir_removes_an_empty_directory() {
    let root = TestRoot::new("delete_dir");
    fs::create_dir_all(root.path().join("empty")).unwrap();
    let subject = new();
    let peer = root.peer();

    subject.delete_dir(&peer, "empty").unwrap();

    assert_not_found(subject.stat(&peer, "empty"));
}

#[test]
fn delete_dir_does_not_remove_a_non_empty_directory() {
    let root = TestRoot::new("delete_dir_non_empty");
    fs::create_dir_all(root.path().join("directory")).unwrap();
    fs::write(root.path().join("directory").join("file.txt"), b"contents").unwrap();
    let subject = new();
    let peer = root.peer();

    assert!(subject.delete_dir(&peer, "directory").is_err());

    assert_eq!(read_all(subject.as_ref(), &peer, "directory/file.txt"), b"contents");
}

#[test]
fn set_mod_time_updates_the_time_returned_by_stat() {
    let root = TestRoot::new("set_mod_time");
    fs::write(root.path().join("file.txt"), b"time").unwrap();
    let subject = new();
    let peer = root.peer();
    let mod_time = UNIX_EPOCH + Duration::from_secs(1_700_000_000);

    subject.set_mod_time(&peer, "file.txt", mod_time).unwrap();

    assert_eq!(subject.stat(&peer, "file.txt").unwrap().mod_time, mod_time);
}

#[test]
fn operations_are_scoped_to_the_connected_root() {
    let left = TestRoot::new("scoped_left");
    let right = TestRoot::new("scoped_right");
    fs::write(left.path().join("same.txt"), b"left").unwrap();
    fs::write(right.path().join("same.txt"), b"right").unwrap();
    let subject = new();
    let left_peer = left.peer();
    let right_peer = right.peer();

    assert_eq!(read_all(subject.as_ref(), &left_peer, "same.txt"), b"left");
    assert_eq!(read_all(subject.as_ref(), &right_peer, "same.txt"), b"right");
}
