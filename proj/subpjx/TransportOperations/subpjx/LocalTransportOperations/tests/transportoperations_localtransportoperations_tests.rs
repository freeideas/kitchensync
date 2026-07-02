use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use transportoperations_localtransportoperations::{
    self as local_transport, LocalTransportEntryType, LocalTransportErrorCategory,
    LocalTransportOperations, LocalTransportReadResult, LocalTransportRoot,
};

struct TestRoot {
    path: PathBuf,
}

struct TestFile {
    path: PathBuf,
}

impl TestRoot {
    fn new(name: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is before Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "kitchensync-localtransportoperations-{name}-{}-{unique}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create test root");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn transport_root(&self) -> LocalTransportRoot {
        LocalTransportRoot {
            local_peer_root_path: self.path.clone(),
        }
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

impl Drop for TestFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn subject() -> Arc<dyn LocalTransportOperations> {
    local_transport::new()
}

#[test]
fn local_file_peer_streams_bytes_and_reports_directory_metadata() {
    let root = TestRoot::new("stream-and-list");
    let transport_root = root.transport_root();
    let transport = subject();

    let write_handle = transport
        .open_write(&transport_root, "alpha/bravo/file.txt")
        .expect("open write creates the target file and parents");
    transport
        .write(write_handle, b"hello ")
        .expect("write first byte chunk");
    transport
        .write(write_handle, b"world")
        .expect("write second byte chunk");
    transport
        .close_write(write_handle)
        .expect("flush and close written file");

    assert_eq!(
        fs::read(root.path().join("alpha/bravo/file.txt")).expect("read local file"),
        b"hello world"
    );

    let file_metadata = transport
        .stat(&transport_root, "alpha/bravo/file.txt")
        .expect("stat written regular file");
    assert_eq!(LocalTransportEntryType::File, file_metadata.entry_type);
    assert_eq!(11, file_metadata.byte_size);

    let directory_metadata = transport
        .stat(&transport_root, "alpha")
        .expect("stat created directory");
    assert_eq!(LocalTransportEntryType::Directory, directory_metadata.entry_type);
    assert_eq!(-1, directory_metadata.byte_size);

    let mut entries = transport
        .list_dir(&transport_root, "alpha")
        .expect("list immediate directory children");
    entries.sort_by(|left, right| left.child_name.cmp(&right.child_name));
    assert_eq!(1, entries.len());
    assert_eq!("bravo", entries[0].child_name);
    assert_eq!(LocalTransportEntryType::Directory, entries[0].metadata.entry_type);
    assert_eq!(-1, entries[0].metadata.byte_size);

    let entries = transport
        .list_dir(&transport_root, "alpha/bravo")
        .expect("list immediate file child");
    assert_eq!(1, entries.len());
    assert_eq!("file.txt", entries[0].child_name);
    assert_eq!(LocalTransportEntryType::File, entries[0].metadata.entry_type);
    assert_eq!(11, entries[0].metadata.byte_size);

    let read_handle = transport
        .open_read(&transport_root, "alpha/bravo/file.txt")
        .expect("open regular file for reading");
    assert_eq!(
        LocalTransportReadResult::Bytes(b"hello".to_vec()),
        transport
            .read(read_handle, 5)
            .expect("read first bytes from handle")
    );
    assert_eq!(
        LocalTransportReadResult::Bytes(b" world".to_vec()),
        transport
            .read(read_handle, 20)
            .expect("read remaining bytes from handle")
    );
    assert_eq!(
        LocalTransportReadResult::Eof,
        transport
            .read(read_handle, 20)
            .expect("read end of file from handle")
    );
    transport
        .close_read(read_handle)
        .expect("close read handle after streaming");
}

#[test]
fn local_file_peer_mutates_entries_inside_the_connected_root() {
    let root = TestRoot::new("mutations");
    let transport_root = root.transport_root();
    let transport = subject();

    let write_handle = transport
        .open_write(&transport_root, "source.txt")
        .expect("open source file for writing");
    transport
        .write(write_handle, b"move me")
        .expect("write source content");
    transport
        .close_write(write_handle)
        .expect("close source writer");

    transport
        .rename(&transport_root, "source.txt", "renamed.txt")
        .expect("rename file to a missing destination");
    assert_eq!(
        LocalTransportErrorCategory::NotFound,
        transport
            .stat(&transport_root, "source.txt")
            .expect_err("renamed source is absent")
    );
    assert_eq!(
        b"move me",
        fs::read(root.path().join("renamed.txt"))
            .expect("renamed file exists in the local filesystem")
            .as_slice()
    );

    let next_write_handle = transport
        .open_write(&transport_root, "next-source.txt")
        .expect("open second source file for writing");
    transport
        .write(next_write_handle, b"new content")
        .expect("write second source content");
    transport
        .close_write(next_write_handle)
        .expect("close second source writer");
    assert!(
        transport
            .rename(&transport_root, "next-source.txt", "renamed.txt")
            .is_err(),
        "rename rejects an existing destination"
    );
    assert_eq!(
        b"new content",
        fs::read(root.path().join("next-source.txt"))
            .expect("source remains when destination already exists")
            .as_slice()
    );
    assert_eq!(
        b"move me",
        fs::read(root.path().join("renamed.txt"))
            .expect("destination remains unchanged when rename is rejected")
            .as_slice()
    );

    let file_time = UNIX_EPOCH + Duration::from_secs(1_700_000_123);
    transport
        .set_mod_time(&transport_root, "renamed.txt", file_time)
        .expect("set regular file modification time");
    assert_eq!(
        file_time,
        transport
            .stat(&transport_root, "renamed.txt")
            .expect("stat file after setting modification time")
            .modification_time
    );

    transport
        .create_dir(&transport_root, "empty/child")
        .expect("create directory and missing parents");
    let dir_time = UNIX_EPOCH + Duration::from_secs(1_700_000_456);
    transport
        .set_mod_time(&transport_root, "empty/child", dir_time)
        .expect("set directory modification time");
    assert_eq!(
        dir_time,
        transport
            .stat(&transport_root, "empty/child")
            .expect("stat directory after setting modification time")
            .modification_time
    );

    let child_write_handle = transport
        .open_write(&transport_root, "empty/child/file.txt")
        .expect("open file in directory");
    transport
        .write(child_write_handle, b"still here")
        .expect("write file in directory");
    transport
        .close_write(child_write_handle)
        .expect("close file in directory");
    assert_eq!(
        LocalTransportErrorCategory::IoError,
        transport
            .delete_dir(&transport_root, "empty/child")
            .expect_err("non-empty directory is not deleted")
    );
    transport
        .delete_file(&transport_root, "empty/child/file.txt")
        .expect("delete child file before deleting directory");

    transport
        .delete_file(&transport_root, "renamed.txt")
        .expect("delete regular file");
    assert_eq!(
        LocalTransportErrorCategory::NotFound,
        transport
            .stat(&transport_root, "renamed.txt")
            .expect_err("deleted file is absent")
    );

    transport
        .delete_dir(&transport_root, "empty/child")
        .expect("delete empty local directory");
    assert_eq!(
        LocalTransportErrorCategory::NotFound,
        transport
            .stat(&transport_root, "empty/child")
            .expect_err("deleted directory is absent")
    );
}

#[test]
fn local_file_peer_rejects_paths_that_escape_the_connected_root() {
    let root = TestRoot::new("root-containment");
    let outside_name = format!("outside-{}.txt", std::process::id());
    let outside_file = TestFile {
        path: root
            .path()
            .parent()
            .expect("test root has a parent")
            .join(&outside_name),
    };
    let _ = fs::remove_file(&outside_file.path);
    fs::write(&outside_file.path, b"outside").expect("write outside file");
    let escaped_path = format!("../{outside_name}");

    let transport_root = root.transport_root();
    let transport = subject();

    transport
        .stat(&transport_root, &escaped_path)
        .expect_err("parent path cannot be statted through local transport");
    transport
        .open_write(&transport_root, &escaped_path)
        .expect_err("parent path cannot be opened for writing");
    assert_eq!(
        b"outside",
        fs::read(&outside_file.path)
            .expect("outside file was not modified by rejected operations")
            .as_slice()
    );
}
