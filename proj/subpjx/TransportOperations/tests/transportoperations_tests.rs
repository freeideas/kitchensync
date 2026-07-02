use std::any::Any;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use transportoperations::{
    new, TransportEntryType, TransportErrorCategory, TransportOperations, TransportPeerHandle,
    TransportReadResult,
};
use transportoperations_localtransportoperations::LocalTransportRoot;

struct TestRoot {
    path: PathBuf,
}

impl TestRoot {
    fn new(name: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is before Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "kitchensync-transportoperations-{name}-{}-{unique}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create test root");
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

fn subject() -> Arc<dyn TransportOperations> {
    new(
        transportoperations_localtransportoperations::new(),
        transportoperations_sftptransportoperations::new(),
    )
}

fn file_peer(root: &Path) -> TransportPeerHandle {
    TransportPeerHandle::File {
        root: root.to_path_buf(),
        handle: Arc::new(LocalTransportRoot {
            local_peer_root_path: root.to_path_buf(),
        }) as Arc<dyn Any + Send + Sync>,
    }
}

#[test]
fn file_peer_list_dir_reports_only_immediate_files_and_directories() {
    let root = TestRoot::new("list-dir");
    fs::write(root.path().join("alpha.txt"), b"abc").expect("write child file");
    fs::create_dir_all(root.path().join("bravo").join("nested")).expect("create nested dir");
    fs::write(root.path().join("bravo").join("nested.txt"), b"hidden")
        .expect("write nested file");

    let transport = subject();
    let peer = file_peer(root.path());
    let mut entries = transport
        .list_dir(&peer, "")
        .expect("list file peer root through transport trait");
    entries.sort_by(|left, right| left.name.cmp(&right.name));

    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].name, "alpha.txt");
    assert_eq!(entries[0].metadata.entry_type, TransportEntryType::File);
    assert_eq!(entries[0].metadata.byte_size, 3);
    assert_eq!(entries[1].name, "bravo");
    assert_eq!(entries[1].metadata.entry_type, TransportEntryType::Directory);
    assert_eq!(entries[1].metadata.byte_size, -1);
}

#[test]
fn file_peer_stat_reports_file_directory_and_missing_path_categories() {
    let root = TestRoot::new("stat");
    fs::write(root.path().join("file.txt"), b"content").expect("write file");
    fs::create_dir(root.path().join("directory")).expect("create directory");

    let transport = subject();
    let peer = file_peer(root.path());

    let file = transport.stat(&peer, "file.txt").expect("stat regular file");
    assert_eq!(file.entry_type, TransportEntryType::File);
    assert_eq!(file.byte_size, 7);

    let directory = transport.stat(&peer, "directory").expect("stat directory");
    assert_eq!(directory.entry_type, TransportEntryType::Directory);
    assert_eq!(directory.byte_size, -1);

    let missing = transport
        .stat(&peer, "missing.txt")
        .expect_err("missing path is reported as not found");
    assert_eq!(missing.category, TransportErrorCategory::NotFound);
}

#[test]
fn file_peer_streaming_write_creates_parents_and_streaming_read_returns_eof() {
    let root = TestRoot::new("streaming");
    let transport = subject();
    let peer = file_peer(root.path());

    let writer = transport
        .open_write(&peer, "new/parent/file.txt")
        .expect("open write creates target and parents");
    transport.write(&writer, b"hello ").expect("write first chunk");
    transport.write(&writer, b"world").expect("write second chunk");
    transport.close_write(writer).expect("close write handle");

    let reader = transport
        .open_read(&peer, "new/parent/file.txt")
        .expect("open read for written regular file");
    assert_eq!(
        transport.read(&reader, 5).expect("read first bytes"),
        TransportReadResult::Bytes(b"hello".to_vec())
    );
    assert_eq!(
        transport.read(&reader, 20).expect("read remaining bytes"),
        TransportReadResult::Bytes(b" world".to_vec())
    );
    assert_eq!(
        transport.read(&reader, 20).expect("read eof"),
        TransportReadResult::Eof
    );
    transport.close_read(reader).expect("close read handle");
}

#[test]
fn file_peer_mutating_operations_move_delete_create_and_set_modification_times() {
    let root = TestRoot::new("mutations");
    fs::write(root.path().join("source.txt"), b"move me").expect("write source");

    let transport = subject();
    let peer = file_peer(root.path());

    transport
        .rename(&peer, "source.txt", "renamed.txt")
        .expect("rename to missing destination");
    assert_eq!(
        transport.stat(&peer, "source.txt").expect_err("source moved").category,
        TransportErrorCategory::NotFound
    );
    assert_eq!(
        transport
            .stat(&peer, "renamed.txt")
            .expect("destination exists")
            .byte_size,
        7
    );

    let file_time = UNIX_EPOCH + Duration::from_secs(1_700_000_123);
    transport
        .set_mod_time(&peer, "renamed.txt", file_time)
        .expect("set file modification time");
    assert_eq!(
        transport
            .stat(&peer, "renamed.txt")
            .expect("stat renamed file")
            .modification_time,
        file_time
    );

    transport
        .create_dir(&peer, "created/child")
        .expect("create directory and parents");
    let dir_time = UNIX_EPOCH + Duration::from_secs(1_700_000_456);
    transport
        .set_mod_time(&peer, "created/child", dir_time)
        .expect("set directory modification time");
    assert_eq!(
        transport
            .stat(&peer, "created/child")
            .expect("stat created directory")
            .modification_time,
        dir_time
    );

    transport
        .delete_file(&peer, "renamed.txt")
        .expect("delete regular file");
    assert_eq!(
        transport
            .stat(&peer, "renamed.txt")
            .expect_err("deleted file is absent")
            .category,
        TransportErrorCategory::NotFound
    );

    transport
        .delete_dir(&peer, "created/child")
        .expect("delete empty directory");
    assert_eq!(
        transport
            .stat(&peer, "created/child")
            .expect_err("deleted directory is absent")
            .category,
        TransportErrorCategory::NotFound
    );
}
