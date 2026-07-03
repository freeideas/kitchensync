use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, UNIX_EPOCH};

use localtransport::{new, LocalConnectionRequest, LocalTransport};
use peertransportsurface::{
    new as peer_surface_new, PeerDirectoryEntry, PeerReadChunk, PeerTransportError,
};

static NEXT_TEST_ROOT: AtomicUsize = AtomicUsize::new(0);

struct TestRoot {
    path: PathBuf,
}

impl TestRoot {
    fn new(name: &str) -> Self {
        let id = NEXT_TEST_ROOT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "kitchensync_localtransport_{name}_{}_{}",
            std::process::id(),
            id
        ));
        let _ = fs::remove_dir_all(&path);
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn request(root_path: &Path, create_missing_root: bool) -> LocalConnectionRequest {
    LocalConnectionRequest {
        root_path: root_path.to_path_buf(),
        create_missing_root,
    }
}

fn sort_entries(entries: &mut [PeerDirectoryEntry]) {
    entries.sort_by(|left, right| left.child_name.cmp(&right.child_name));
}

#[test]
fn connect_creates_missing_root_and_parents_when_requested() {
    let temp = TestRoot::new("connect_creates_missing_root");
    let root = temp.path().join("missing").join("parent").join("peer");
    let subject = new(peer_surface_new());

    let peer = subject
        .connect(request(&root, true))
        .expect("missing local root should connect after creation");

    assert!(root.is_dir());
    assert_eq!(
        subject
            .create_dir(&peer, "created-after-connect")
            .expect("connected root should accept later operations"),
        ()
    );
    assert!(root.join("created-after-connect").is_dir());
}

#[test]
fn connect_reports_not_found_when_missing_root_may_not_be_created() {
    let temp = TestRoot::new("connect_missing_root_without_create");
    let root = temp.path().join("missing-peer");
    let subject = new(peer_surface_new());

    assert!(matches!(
        subject.connect(request(&root, false)),
        Err(PeerTransportError::NotFound)
    ));
}

#[test]
fn list_dir_returns_only_immediate_regular_children_with_reported_names() {
    let temp = TestRoot::new("list_dir");
    let root = temp.path();
    fs::create_dir_all(root.join("folder").join("subdir")).unwrap();
    fs::write(root.join("folder").join("MiXeD Name.txt"), b"abc").unwrap();
    fs::write(root.join("folder").join("subdir").join("nested.txt"), b"nested").unwrap();
    let subject = new(peer_surface_new());
    let peer = subject.connect(request(root, false)).unwrap();

    let mut entries = subject.list_dir(&peer, "folder").unwrap();
    sort_entries(&mut entries);

    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].child_name, "MiXeD Name.txt");
    assert!(!entries[0].is_dir);
    assert_eq!(entries[0].byte_size, 3);
    assert_eq!(entries[1].child_name, "subdir");
    assert!(entries[1].is_dir);
    assert_eq!(entries[1].byte_size, -1);
}

#[test]
fn stat_reports_regular_file_and_directory_metadata() {
    let temp = TestRoot::new("stat");
    let root = temp.path();
    fs::create_dir_all(root.join("folder")).unwrap();
    fs::write(root.join("folder").join("file.bin"), b"abcdef").unwrap();
    let subject = new(peer_surface_new());
    let peer = subject.connect(request(root, false)).unwrap();

    let file = subject.stat(&peer, "folder/file.bin").unwrap();
    let directory = subject.stat(&peer, "folder").unwrap();

    assert!(!file.is_dir);
    assert_eq!(file.byte_size, 6);
    assert!(directory.is_dir);
    assert_eq!(directory.byte_size, -1);
}

#[test]
fn open_read_streams_chunks_no_larger_than_requested_then_eof() {
    let temp = TestRoot::new("open_read");
    let root = temp.path();
    fs::create_dir_all(root).unwrap();
    fs::write(root.join("file.bin"), b"abcdef").unwrap();
    let subject = new(peer_surface_new());
    let peer = subject.connect(request(root, false)).unwrap();
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
fn open_write_creates_parents_and_finalizes_bytes_in_write_order() {
    let temp = TestRoot::new("open_write");
    let root = temp.path();
    fs::create_dir_all(root).unwrap();
    let subject = new(peer_surface_new());
    let peer = subject.connect(request(root, false)).unwrap();

    let mut handle = subject.open_write(&peer, "parent/child/file.txt").unwrap();
    subject.write(&mut handle, b"one").unwrap();
    subject.write(&mut handle, b"-two").unwrap();
    subject.close_write(handle).unwrap();

    let mut read_handle = subject.open_read(&peer, "parent/child/file.txt").unwrap();
    assert_eq!(
        subject.read(&mut read_handle, 16).unwrap(),
        PeerReadChunk::Bytes(b"one-two".to_vec())
    );
    assert_eq!(subject.read(&mut read_handle, 16).unwrap(), PeerReadChunk::Eof);
    subject.close_read(read_handle).unwrap();
}

#[test]
fn rename_moves_a_file_to_a_non_existing_destination() {
    let temp = TestRoot::new("rename");
    let root = temp.path();
    fs::create_dir_all(root.join("dst")).unwrap();
    fs::write(root.join("src.txt"), b"contents").unwrap();
    let subject = new(peer_surface_new());
    let peer = subject.connect(request(root, false)).unwrap();

    subject.rename(&peer, "src.txt", "dst/moved.txt").unwrap();

    assert!(!root.join("src.txt").exists());
    assert_eq!(fs::read(root.join("dst").join("moved.txt")).unwrap(), b"contents");
}

#[test]
fn delete_file_removes_a_file_under_the_connected_root() {
    let temp = TestRoot::new("delete_file");
    let root = temp.path();
    fs::create_dir_all(root).unwrap();
    fs::write(root.join("remove.txt"), b"remove").unwrap();
    let subject = new(peer_surface_new());
    let peer = subject.connect(request(root, false)).unwrap();

    subject.delete_file(&peer, "remove.txt").unwrap();

    assert!(!root.join("remove.txt").exists());
}

#[test]
fn create_dir_creates_missing_parents_under_the_connected_root() {
    let temp = TestRoot::new("create_dir");
    let root = temp.path();
    fs::create_dir_all(root).unwrap();
    let subject = new(peer_surface_new());
    let peer = subject.connect(request(root, false)).unwrap();

    subject.create_dir(&peer, "a/b/c").unwrap();

    assert!(root.join("a").join("b").join("c").is_dir());
}

#[test]
fn delete_dir_removes_an_empty_directory_under_the_connected_root() {
    let temp = TestRoot::new("delete_dir");
    let root = temp.path();
    fs::create_dir_all(root.join("empty")).unwrap();
    let subject = new(peer_surface_new());
    let peer = subject.connect(request(root, false)).unwrap();

    subject.delete_dir(&peer, "empty").unwrap();

    assert!(!root.join("empty").exists());
}

#[test]
fn set_mod_time_updates_the_metadata_time_observed_by_stat() {
    let temp = TestRoot::new("set_mod_time");
    let root = temp.path();
    fs::create_dir_all(root).unwrap();
    fs::write(root.join("file.txt"), b"time").unwrap();
    let subject = new(peer_surface_new());
    let peer = subject.connect(request(root, false)).unwrap();
    let mod_time = UNIX_EPOCH + Duration::from_secs(1_700_000_000);

    subject.set_mod_time(&peer, "file.txt", mod_time).unwrap();

    assert_eq!(subject.stat(&peer, "file.txt").unwrap().mod_time, mod_time);
}

#[test]
fn connected_roots_keep_later_operations_scoped_to_their_selected_root() {
    let left = TestRoot::new("connected_root_left");
    let right = TestRoot::new("connected_root_right");
    fs::create_dir_all(left.path()).unwrap();
    fs::create_dir_all(right.path()).unwrap();
    fs::write(left.path().join("same.txt"), b"left").unwrap();
    fs::write(right.path().join("same.txt"), b"right").unwrap();
    let subject = new(peer_surface_new());
    let left_peer = subject.connect(request(left.path(), false)).unwrap();
    let right_peer = subject.connect(request(right.path(), false)).unwrap();

    let mut left_handle = subject.open_read(&left_peer, "same.txt").unwrap();
    let mut right_handle = subject.open_read(&right_peer, "same.txt").unwrap();

    assert_eq!(
        subject.read(&mut left_handle, 16).unwrap(),
        PeerReadChunk::Bytes(b"left".to_vec())
    );
    assert_eq!(
        subject.read(&mut right_handle, 16).unwrap(),
        PeerReadChunk::Bytes(b"right".to_vec())
    );
    subject.close_read(left_handle).unwrap();
    subject.close_read(right_handle).unwrap();
}

#[test]
fn missing_paths_are_reported_as_not_found_for_read_only_operations() {
    let temp = TestRoot::new("missing_read_only");
    let root = temp.path();
    fs::create_dir_all(root).unwrap();
    let subject = new(peer_surface_new());
    let peer = subject.connect(request(root, false)).unwrap();

    assert!(matches!(
        subject.list_dir(&peer, "missing"),
        Err(PeerTransportError::NotFound)
    ));
    assert!(matches!(
        subject.stat(&peer, "missing"),
        Err(PeerTransportError::NotFound)
    ));
    assert!(matches!(
        subject.open_read(&peer, "missing"),
        Err(PeerTransportError::NotFound)
    ));
}
