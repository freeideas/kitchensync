use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use kitchensync::{
    EntryKind, PeerUrl, RelPath, Timestamp, TransportError, TransportHandle, TransportRootMode,
    TransportTimeouts,
};

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn next_test_root(name: &str) -> PathBuf {
    let seq = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    let mut path = std::env::temp_dir();
    path.push(format!(
        "kitchensync_transport_{}_{}",
        name.replace(['\\', '/'], "_"),
        seq,
    ));

    if path.exists() {
        let _ = fs::remove_dir_all(&path);
    }

    path
}

fn file_peer_url(root: &Path) -> PeerUrl {
    let path = root.to_string_lossy().replace('\\', "/");
    PeerUrl {
        scheme: "file".to_string(),
        username: None,
        password: None,
        host: None,
        port: None,
        path,
        identity: format!("file:///{}", root.to_string_lossy().replace('\\', "/")),
        timeout_conn: None,
        timeout_idle: None,
    }
}

fn connect_file_transport(root: &Path, root_mode: TransportRootMode) -> TransportHandle {
    kitchensync::transport::factory()
        .connect(
            &file_peer_url(root),
            TransportTimeouts {
                timeout_conn: 1,
                timeout_idle: 1,
            },
            root_mode,
        )
        .expect("transport connect should succeed")
}

#[test]
fn transport_status_and_summary_are_consistent() {
    let status = kitchensync::transport::status();
    let summary = kitchensync::transport::summary();

    assert_eq!(status.name, "transport");
    assert_eq!(
        status.purpose,
        "Local filesystem and SSH/SFTP file tree operations."
    );
    assert!(summary.starts_with("transport:"));
    assert!(summary.contains(status.purpose));
}

#[test]
fn transport_connect_creates_missing_file_root_when_required() {
    let root = next_test_root("connect_creates_root");

    let transport = connect_file_transport(&root, TransportRootMode::CreateMissing);
    assert!(root.exists());
    assert!(root.is_dir());

    assert!(transport
        .list_dir(&RelPath::new("").unwrap())
        .unwrap()
        .is_empty());
}

#[test]
fn transport_connect_requires_existing_root_when_requested() {
    let root = next_test_root("connect_requires_root");

    let error = kitchensync::transport::factory()
        .connect(
            &file_peer_url(&root),
            TransportTimeouts {
                timeout_conn: 1,
                timeout_idle: 1,
            },
            TransportRootMode::RequireExisting,
        )
        .expect_err("missing root should fail in require-existing mode");

    assert_eq!(error, TransportError::NotFound);
}

#[test]
fn transport_list_dir_preserves_names_and_reports_immediate_children() {
    let root = next_test_root("list_dir_children");
    fs::create_dir_all(&root).unwrap();
    let transport = connect_file_transport(&root, TransportRootMode::RequireExisting);

    fs::create_dir_all(root.join("NestedDir")).unwrap();
    fs::write(root.join("upper-Case.txt"), b"hello").unwrap();
    fs::write(root.join("NestedDir/report.txt"), b"skip-me").unwrap();

    let mut entries = transport
        .list_dir(&RelPath::new("").unwrap())
        .expect("listing root");
    entries.sort_by(|a, b| a.name.cmp(&b.name));

    let file_meta = transport
        .stat(&RelPath::new("upper-Case.txt").unwrap())
        .unwrap();
    let dir_meta = transport.stat(&RelPath::new("NestedDir").unwrap()).unwrap();

    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].name, "NestedDir");
    assert_eq!(entries[0].kind, EntryKind::Directory);
    assert_eq!(entries[0].byte_size, -1);
    assert_eq!(entries[0].mod_time, dir_meta.mod_time);

    assert_eq!(entries[1].name, "upper-Case.txt");
    assert_eq!(entries[1].kind, EntryKind::File);
    assert_eq!(entries[1].byte_size, 5);
    assert_eq!(entries[1].mod_time, file_meta.mod_time);

    assert!(!entries.iter().any(|entry| entry.name == "report.txt"));
}

#[test]
fn transport_list_dir_omits_symbolic_links_when_link_can_be_created() {
    let root = next_test_root("list_omit_symlink");
    fs::create_dir_all(&root).unwrap();
    let transport = connect_file_transport(&root, TransportRootMode::RequireExisting);

    fs::write(root.join("target.txt"), b"target").unwrap();
    let link = root.join("link.txt");

    #[cfg(unix)]
    let link_result = std::os::unix::fs::symlink(root.join("target.txt"), &link);

    #[cfg(windows)]
    let link_result = std::os::windows::fs::symlink_file(root.join("target.txt"), &link);

    match link_result {
        Ok(()) => {
            let entries = transport
                .list_dir(&RelPath::new("").unwrap())
                .expect("listing with link");
            assert!(!entries.iter().any(|entry| entry.name == "link.txt"));
        }
        Err(_) => return, // Not reasonably testable: platform/symlink-prerequisite not available in this environment.
    }
}

#[test]
fn transport_stat_reports_file_and_directory_and_not_found_for_missing_path() {
    let root = next_test_root("stat_paths");
    fs::create_dir_all(&root).unwrap();
    let transport = connect_file_transport(&root, TransportRootMode::RequireExisting);

    fs::create_dir_all(root.join("archive")).unwrap();
    fs::write(root.join("readme.md"), b"notes").unwrap();

    let file_meta = transport
        .stat(&RelPath::new("readme.md").unwrap())
        .expect("file stat");
    assert_eq!(file_meta.name, "readme.md");
    assert_eq!(file_meta.kind, EntryKind::File);
    assert_eq!(file_meta.byte_size, 5);

    let dir_meta = transport
        .stat(&RelPath::new("archive").unwrap())
        .expect("dir stat");
    assert_eq!(dir_meta.kind, EntryKind::Directory);
    assert_eq!(dir_meta.byte_size, -1);

    assert_eq!(
        transport
            .stat(&RelPath::new("missing.txt").unwrap())
            .unwrap_err(),
        TransportError::NotFound,
    );
}

#[test]
fn transport_stat_reports_not_found_for_symbolic_link_if_link_can_be_created() {
    let root = next_test_root("stat_symlink_not_found");
    fs::create_dir_all(&root).unwrap();
    let transport = connect_file_transport(&root, TransportRootMode::RequireExisting);

    fs::write(root.join("target.txt"), b"target").unwrap();
    let link = root.join("link.txt");

    #[cfg(unix)]
    let link_result = std::os::unix::fs::symlink(root.join("target.txt"), &link);

    #[cfg(windows)]
    let link_result = std::os::windows::fs::symlink_file(root.join("target.txt"), &link);

    match link_result {
        Ok(()) => assert_eq!(
            transport
                .stat(&RelPath::new("link.txt").unwrap())
                .unwrap_err(),
            TransportError::NotFound,
        ),
        Err(_) => return, // Not reasonably testable: platform/symlink-prerequisite not available in this environment.
    }
}

#[test]
fn transport_open_write_reports_io_error_for_non_directory_parent_path() {
    let root = next_test_root("io_error_parent");
    fs::create_dir_all(&root).unwrap();
    let transport = connect_file_transport(&root, TransportRootMode::RequireExisting);

    fs::write(root.join("parent.txt"), b"block").unwrap();

    let err = transport
        .open_write(&RelPath::new("parent.txt/child.txt").unwrap())
        .unwrap_err();

    assert_eq!(err, TransportError::IoError);
}

#[cfg(unix)]
#[test]
fn transport_open_write_reports_permission_denied_for_read_only_directory() {
    use std::os::unix::fs::PermissionsExt;

    let root = next_test_root("permission_denied_write");
    fs::create_dir_all(&root).unwrap();
    let transport = connect_file_transport(&root, TransportRootMode::RequireExisting);

    let readonly_dir = root.join("readonly");
    fs::create_dir_all(&readonly_dir).unwrap();

    let mut permissions = fs::metadata(&readonly_dir).unwrap().permissions();
    permissions.set_mode(0o500);
    fs::set_permissions(&readonly_dir, permissions).unwrap();

    let err = transport
        .open_write(&RelPath::new("readonly/out.txt").unwrap())
        .unwrap_err();

    assert_eq!(err, TransportError::PermissionDenied);
}

#[test]
#[ignore = "Not reasonably testable here: requires a live SSH/SFTP endpoint and remote server controls for timeout/keep-alive/error-channel behavior."]
fn transport_sftp_timeout_and_keepalive_semantics_require_integration_fixture() {}

#[test]
fn transport_open_read_reports_not_found_for_missing_file() {
    let root = next_test_root("open_read_missing");
    fs::create_dir_all(&root).unwrap();
    let transport = connect_file_transport(&root, TransportRootMode::RequireExisting);

    assert_eq!(
        transport
            .open_read(&RelPath::new("missing.bin").unwrap())
            .unwrap_err(),
        TransportError::NotFound,
    );
}

#[test]
fn transport_open_write_creates_parent_dirs_and_streaming_read_matches() {
    let root = next_test_root("streaming_write_read");
    fs::create_dir_all(&root).unwrap();
    let transport = connect_file_transport(&root, TransportRootMode::RequireExisting);

    let rel_path = RelPath::new("nested/paths/document.txt").unwrap();
    let expected = b"chunk-one-chunk-two";
    let mut writer = transport.open_write(&rel_path).expect("open write");
    writer.write_all(b"chunk-one-").unwrap();
    writer.write_all(b"chunk-two").unwrap();
    writer.close().expect("close write");

    let mut reader = transport.open_read(&rel_path).expect("open read");
    let mut collected = Vec::new();
    let mut chunk = vec![0u8; 4];
    let first_read = reader.read(&mut chunk).expect("first read");
    collected.extend_from_slice(&chunk[..first_read]);

    let mut chunk = vec![0u8; 8];
    let second_read = reader.read(&mut chunk).expect("second read");
    collected.extend_from_slice(&chunk[..second_read]);

    let mut chunk = vec![0u8; 16];
    let third_read = reader.read(&mut chunk).expect("third read");
    collected.extend_from_slice(&chunk[..third_read]);

    assert_eq!(first_read, 4);
    assert_eq!(second_read, 8);
    assert_eq!(third_read, expected.len() - first_read - second_read);
    assert_eq!(collected, expected);
    assert_eq!(reader.read(&mut [0u8; 1]).expect("eof read"), 0);

    let mut replace_writer = transport
        .open_write(&rel_path)
        .expect("reopen write for truncate");
    replace_writer
        .write_all(b"z")
        .expect("overwrite existing file");
    replace_writer.close().expect("close write overwrite");

    let mut replaced = Vec::new();
    transport
        .open_read(&rel_path)
        .expect("reopen read for truncation check")
        .read_to_end(&mut replaced)
        .expect("read truncated file");

    assert_eq!(replaced, b"z");
}

#[test]
fn transport_rename_no_overwrite_prevents_existing_destination_replace() {
    let root = next_test_root("rename_no_overwrite");
    fs::create_dir_all(&root).unwrap();
    let transport = connect_file_transport(&root, TransportRootMode::RequireExisting);

    fs::write(root.join("source.txt"), b"from").unwrap();
    fs::write(root.join("destination.txt"), b"to").unwrap();

    assert!(transport
        .rename_no_overwrite(
            &RelPath::new("source.txt").unwrap(),
            &RelPath::new("destination.txt").unwrap(),
        )
        .is_err());

    fs::remove_file(root.join("destination.txt")).unwrap();
    transport
        .rename_no_overwrite(
            &RelPath::new("source.txt").unwrap(),
            &RelPath::new("destination.txt").unwrap(),
        )
        .expect("rename when destination absent");

    assert!(!root.join("source.txt").exists());
    assert_eq!(fs::read(root.join("destination.txt")).unwrap(), b"from");
}

#[test]
fn transport_create_dir_delete_file_delete_dir_operate_on_expected_paths() {
    let root = next_test_root("create_delete_paths");
    fs::create_dir_all(&root).unwrap();
    let transport = connect_file_transport(&root, TransportRootMode::RequireExisting);

    let nested_dir = RelPath::new("nested/structure").unwrap();
    transport
        .create_dir(&nested_dir)
        .expect("create nested dir");
    assert!(root.join("nested/structure").is_dir());

    let file_path = RelPath::new("nested/file.txt").unwrap();
    fs::write(root.join("nested/file.txt"), b"data").unwrap();

    transport.delete_file(&file_path).expect("delete file");
    assert!(!root.join("nested/file.txt").exists());

    transport.delete_dir(&nested_dir).expect("delete empty dir");
    assert!(!root.join("nested/structure").exists());
}

#[test]
fn transport_operations_do_not_escape_connected_root() {
    let root = next_test_root("root_boundary");
    let outside = root
        .parent()
        .unwrap()
        .join("kitchensync_transport_root_boundary_outside");

    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("sneaky.txt"), b"outside-root").unwrap();

    let transport = connect_file_transport(&root, TransportRootMode::RequireExisting);

    let escape = match RelPath::new("../kitchensync_transport_root_boundary_outside/sneaky.txt") {
        Ok(path) => path,
        Err(_) => {
            // Not reasonably testable: RelPath rejects parent-relative traversals in public parsing.
            return;
        }
    };

    assert!(
        transport.open_read(&escape).is_err(),
        "root-boundary violation: transport read escaped connected root"
    );
}

#[test]
fn transport_set_mod_time_updates_entry_modification_time_for_file() {
    let root = next_test_root("set_mod_time_file");
    fs::create_dir_all(&root).unwrap();
    let transport = connect_file_transport(&root, TransportRootMode::RequireExisting);

    let path = RelPath::new("time.txt").unwrap();
    fs::write(root.join("time.txt"), b"timestamp").unwrap();

    let stamped = Timestamp("2024-02-03_04-05-06_123456Z".to_string());
    transport
        .set_mod_time(&path, stamped.clone())
        .expect("set mod time");

    assert_eq!(
        transport.stat(&path).expect("stat after set").mod_time,
        stamped
    );
}

#[test]
fn transport_set_mod_time_updates_entry_modification_time_for_directory() {
    let root = next_test_root("set_mod_time_dir");
    fs::create_dir_all(&root).unwrap();
    let transport = connect_file_transport(&root, TransportRootMode::RequireExisting);

    let path = RelPath::new("nested").unwrap();
    transport.create_dir(&path).unwrap();

    let stamped = Timestamp("2022-12-31_23-59-59_654321Z".to_string());
    transport
        .set_mod_time(&path, stamped.clone())
        .expect("set mod time dir");

    assert_eq!(
        transport.stat(&path).expect("dir stat after set").mod_time,
        stamped
    );
}
