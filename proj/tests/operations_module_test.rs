use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Default)]
struct NullSink;

impl kitchensync::DiagnosticSink for NullSink {
    fn publish(&self, _event: kitchensync::DiagnosticEvent) {}
}

impl kitchensync::ProgressSink for NullSink {
    fn publish(&self, _event: kitchensync::ProgressEvent) {}
}

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn next_test_root(test_name: &str) -> PathBuf {
    let seq = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    let normalized = test_name.replace(['\\', '/'], "_");
    let mut path = std::env::temp_dir();
    path.push(format!("kitchensync-operations-test-{normalized}-{seq}"));

    if path.exists() {
        let _ = fs::remove_dir_all(&path);
    }
    fs::create_dir_all(&path).unwrap();
    path
}

fn read_text(root: &Path, rel: &str) -> String {
    fs::read_to_string(root.join(rel)).unwrap()
}

fn read_file_path(path: &Path) -> String {
    fs::read_to_string(path).unwrap()
}

fn write_text(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

fn make_peer_url(root: &Path) -> kitchensync::PeerUrl {
    kitchensync::PeerUrl {
        scheme: "file".to_string(),
        username: None,
        password: None,
        host: None,
        port: None,
        path: root.to_string_lossy().to_string(),
        identity: format!("file://{}", root.to_string_lossy()),
        timeout_conn: None,
        timeout_idle: None,
    }
}

fn make_peer_session(id: kitchensync::PeerId, root: &Path) -> kitchensync::PeerSession {
    let selected_url = make_peer_url(root);
    let normalized_identity = make_peer_url(root);
    let transport = kitchensync::transport::factory()
        .connect(
            &selected_url,
            kitchensync::TransportTimeouts {
                timeout_conn: 1,
                timeout_idle: 1,
            },
            kitchensync::TransportRootMode::CreateMissing,
        )
        .unwrap();

    kitchensync::PeerSession {
        id,
        invocation_index: 0,
        normalized_identity,
        selected_url,
        declared_role: kitchensync::PeerRole::Normal,
        effective_role: kitchensync::EffectivePeerRole::Contributing,
        transport,
        had_startup_snapshot: false,
    }
}

fn make_run_config(dry_run: bool) -> kitchensync::RunConfig {
    kitchensync::RunConfig {
        dry_run,
        max_copies: 1,
        retries_copy: 1,
        retries_list: 1,
        timeout_conn: 1,
        timeout_idle: 1,
        verbosity: kitchensync::Verbosity::Error,
        keep_tmp_days: 2,
        keep_bak_days: 2,
        keep_del_days: 0,
        excludes: Vec::new(),
    }
}

fn entry_meta(name: &str, content: &str) -> kitchensync::EntryMeta {
    kitchensync::EntryMeta {
        name: name.to_string(),
        kind: kitchensync::EntryKind::File,
        mod_time: kitchensync::Timestamp("2024-01-01_00-00-00_000000Z".to_string()),
        byte_size: content.len() as i64,
    }
}

fn immediate_subdirs(path: &Path) -> Vec<PathBuf> {
    if !path.exists() {
        return Vec::new();
    }
    let mut list = Vec::new();
    for entry in fs::read_dir(path).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            list.push(entry.path());
        }
    }
    list.sort();
    list
}

#[test]
fn recover_directory_swaps_replays_staged_new_entry() {
    let root = next_test_root("recover_swaps_new_only");
    let peer = make_peer_session(10, &root);
    let config = make_run_config(false);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);

    write_text(&root, "dir/.kitchensync/SWAP/file%20name.txt/new", "recovered value");

    let directory = kitchensync::RelPath::new("dir").unwrap();
    let report = executor
        .recover_directory_swaps(&peer, &directory)
        .expect("recovery should succeed");

    assert_eq!(report.peer_id, peer.id);
    assert_eq!(report.directory, directory);
    assert_eq!(report.recovered_entries, 1);
    assert!(!report.dry_run);
    assert_eq!(read_text(&root, "dir/file name.txt"), "recovered value");
    assert!(!root.join("dir/.kitchensync/SWAP/file%20name.txt").exists());
}

#[test]
fn recover_directory_swaps_restores_old_entry_when_only_old_exists() {
    let root = next_test_root("recover_swaps_old_only");
    let peer = make_peer_session(11, &root);
    let config = make_run_config(false);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);

    write_text(&root, "dir/.kitchensync/SWAP/file%20name.txt/old", "legacy value");

    let directory = kitchensync::RelPath::new("dir").unwrap();
    executor
        .recover_directory_swaps(&peer, &directory)
        .expect("old-only recovery should restore target");

    assert_eq!(read_text(&root, "dir/file name.txt"), "legacy value");
    assert!(!root.join("dir/.kitchensync/SWAP/file%20name.txt").exists());
}

#[test]
fn recover_directory_swaps_old_new_target_archives_old_to_bak() {
    let root = next_test_root("recover_swaps_old_new_target");
    let peer = make_peer_session(12, &root);
    let config = make_run_config(false);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);

    write_text(&root, "dir/file name.txt", "target");
    write_text(&root, "dir/.kitchensync/SWAP/file%20name.txt/old", "old value");
    write_text(&root, "dir/.kitchensync/SWAP/file%20name.txt/new", "replacement");

    let directory = kitchensync::RelPath::new("dir").unwrap();
    executor
        .recover_directory_swaps(&peer, &directory)
        .expect("old+new+target recovery should archive old");

    assert_eq!(read_text(&root, "dir/file name.txt"), "target");
    let bak_root = root.join("dir/.kitchensync/BAK");
    let bak_dirs = immediate_subdirs(&bak_root);
    assert_eq!(bak_dirs.len(), 1);
    assert_eq!(read_file_path(&bak_dirs[0].join("file name.txt")), "old value");
    assert!(!root.join("dir/.kitchensync/SWAP/file%20name.txt").exists());
}

#[test]
fn recover_directory_swaps_deletes_staged_new_when_target_exists() {
    let root = next_test_root("recover_swaps_new_target");
    let peer = make_peer_session(13, &root);
    let config = make_run_config(false);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);

    write_text(&root, "dir/existing.txt", "current value");
    write_text(&root, "dir/.kitchensync/SWAP/existing.txt/new", "staged replacement");

    let directory = kitchensync::RelPath::new("dir").unwrap();
    let report = executor
        .recover_directory_swaps(&peer, &directory)
        .expect("new+target recovery should discard staged new");

    assert_eq!(report.recovered_entries, 1);
    assert_eq!(read_text(&root, "dir/existing.txt"), "current value");
    assert!(!root.join("dir/.kitchensync/SWAP/existing.txt/new").exists());
    assert!(!root.join("dir/.kitchensync/SWAP/existing.txt").exists());
}

#[test]
fn recover_directory_swaps_is_noop_in_dry_run() {
    let root = next_test_root("recover_swaps_dry");
    let peer = make_peer_session(14, &root);
    let config = make_run_config(true);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);

    write_text(&root, "dir/.kitchensync/SWAP/file%20name.txt/new", "not moved");

    let directory = kitchensync::RelPath::new("dir").unwrap();
    let report = executor
        .recover_directory_swaps(&peer, &directory)
        .expect("dry-run recovery should succeed");

    assert_eq!(report.recovered_entries, 0);
    assert!(report.dry_run);
    assert!(!root.join("dir/file name.txt").exists());
    assert!(root.join("dir/.kitchensync/SWAP/file%20name.txt/new").exists());
}

#[test]
fn recover_directory_swaps_skips_snapshot_db_entry() {
    let root = next_test_root("recover_swaps_snapshot_db");
    let peer = make_peer_session(15, &root);
    let config = make_run_config(false);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);

    write_text(
        &root,
        "dir/.kitchensync/SWAP/snapshot.db/stay.txt",
        "snapshot metadata",
    );

    let directory = kitchensync::RelPath::new("dir").unwrap();
    let report = executor
        .recover_directory_swaps(&peer, &directory)
        .expect("snapshot db entry should be ignored");

    assert_eq!(report.peer_id, peer.id);
    assert_eq!(report.recovered_entries, 0);
    assert!(root.join("dir/.kitchensync/SWAP/snapshot.db/stay.txt").exists());
}

#[test]
fn displace_to_bak_moves_entry_to_nearby_bak() {
    let root = next_test_root("displace_to_bak_file");
    let peer = make_peer_session(20, &root);
    let config = make_run_config(false);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);

    write_text(&root, "sub/obsolete.txt", "to be moved");

    let result = executor
        .displace_to_bak(
            &peer,
            &kitchensync::RelPath::new("sub/obsolete.txt").unwrap(),
            kitchensync::Timestamp("2024-01-01_00-00-00_000000Z".to_string()),
        )
        .expect("displacement should move entry");

    assert_eq!(result.peer_id, peer.id);
    assert_eq!(result.original_path.as_str(), "sub/obsolete.txt");
    assert_eq!(result.bak_path.as_str(), "sub/.kitchensync/BAK/2024-01-01_00-00-00_000000Z/obsolete.txt");
    assert!(!result.dry_run);
    assert!(!root.join("sub/obsolete.txt").exists());
    assert_eq!(read_text(&root, "sub/.kitchensync/BAK/2024-01-01_00-00-00_000000Z/obsolete.txt"), "to be moved");
}

#[test]
fn displace_to_bak_is_noop_in_dry_run() {
    let root = next_test_root("displace_to_bak_dry");
    let peer = make_peer_session(21, &root);
    let config = make_run_config(true);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);

    write_text(&root, "sub/obsolete.txt", "to be moved");

    let result = executor
        .displace_to_bak(
            &peer,
            &kitchensync::RelPath::new("sub/obsolete.txt").unwrap(),
            kitchensync::Timestamp("2024-01-01_00-00-00_000000Z".to_string()),
        )
        .expect("dry-run displacement should be planned");

    assert!(result.dry_run);
    assert!(root.join("sub/obsolete.txt").exists());
    assert!(!root.join("sub/.kitchensync").exists());
}

#[test]
fn displace_to_bak_moves_directory_without_recursing() {
    let root = next_test_root("displace_to_bak_directory");
    let peer = make_peer_session(22, &root);
    let config = make_run_config(false);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);

    write_text(&root, "tree/child/leaf.txt", "nested content");

    let result = executor
        .displace_to_bak(
            &peer,
            &kitchensync::RelPath::new("tree").unwrap(),
            kitchensync::Timestamp("2024-01-02_00-00-00_000000Z".to_string()),
        )
        .expect("directory displacement should be one rename");

    assert!(!root.join("tree").exists());
    assert_eq!(result.bak_path.as_str(), "tree/.kitchensync/BAK/2024-01-02_00-00-00_000000Z/tree");
    assert_eq!(read_text(&root, "tree/.kitchensync/BAK/2024-01-02_00-00-00_000000Z/tree/child/leaf.txt"), "nested content");
}

#[test]
fn create_directory_creates_nested_directory() {
    let root = next_test_root("create_directory");
    let peer = make_peer_session(30, &root);
    let config = make_run_config(false);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);

    let path = kitchensync::RelPath::new("a/b/c").unwrap();
    let result = executor
        .create_directory(&peer, &path)
        .expect("create directory should succeed");

    assert_eq!(result.peer_id, peer.id);
    assert_eq!(result.path, path);
    assert!(!result.dry_run);
    assert!(root.join("a/b/c").exists());
}

#[test]
fn create_directory_is_noop_in_dry_run() {
    let root = next_test_root("create_directory_dry");
    let peer = make_peer_session(31, &root);
    let config = make_run_config(true);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);

    let path = kitchensync::RelPath::new("a/b/c").unwrap();
    let result = executor
        .create_directory(&peer, &path)
        .expect("dry-run create should be planned");

    assert!(result.dry_run);
    assert!(!root.join("a/b/c").exists());
}

#[test]
fn create_directory_reports_create_failure_as_operation_error() {
    let root = next_test_root("create_directory_failure");
    let peer = make_peer_session(32, &root);
    let config = make_run_config(false);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);

    write_text(&root, "file", "not a directory");

    let path = kitchensync::RelPath::new("file/child").unwrap();
    let error = executor
        .create_directory(&peer, &path)
        .expect_err("create should fail through parent path");

    assert_eq!(error.peer_id, peer.id);
    assert_eq!(
        error.context,
        kitchensync::operations::OperationErrorContext::CreateDirectory { path: path.clone() }
    );
}

#[test]
fn cleanup_retention_removes_expired_items_and_retains_valid() {
    let root = next_test_root("cleanup_retention_expire");
    let peer = make_peer_session(40, &root);
    let config = make_run_config(false);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);

    write_text(&root, "base/.kitchensync/BAK/2023-12-01_00-00-00_000000Z/old.txt", "old");
    write_text(&root, "base/.kitchensync/BAK/2024-01-06_00-00-00_000000Z/new.txt", "keep");
    write_text(&root, "base/.kitchensync/TMP/2023-12-01_00-00-00_000000Z/old.tmp", "old");
    write_text(&root, "base/.kitchensync/TMP/2024-01-06_00-00-00_000000Z/new.tmp", "keep");

    let report = executor
        .cleanup_retention(
            &peer,
            &kitchensync::RelPath::new("base").unwrap(),
            kitchensync::Timestamp("2024-01-06_00-00-00_000000Z".to_string()),
            2,
            2,
        )
        .expect("cleanup should succeed");

    assert!(!report.dry_run);
    assert!(report.nonfatal_failures.is_empty());
    assert!(!root.join("base/.kitchensync/BAK/2023-12-01_00-00-00_000000Z").exists());
    assert!(!root.join("base/.kitchensync/TMP/2023-12-01_00-00-00_000000Z").exists());
    assert!(root.join("base/.kitchensync/BAK/2024-01-06_00-00-00_000000Z/new.txt").exists());
    assert!(root.join("base/.kitchensync/TMP/2024-01-06_00-00-00_000000Z/new.tmp").exists());
}

#[test]
fn cleanup_retention_is_noop_in_dry_run() {
    let root = next_test_root("cleanup_retention_dry");
    let peer = make_peer_session(41, &root);
    let config = make_run_config(true);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);

    write_text(&root, "base/.kitchensync/TMP/2023-12-01_00-00-00_000000Z/old.tmp", "old");

    let report = executor
        .cleanup_retention(
            &peer,
            &kitchensync::RelPath::new("base").unwrap(),
            kitchensync::Timestamp("2024-01-06_00-00-00_000000Z".to_string()),
            2,
            2,
        )
        .expect("dry-run cleanup should succeed");

    assert!(report.dry_run);
    assert!(report.removed_targets.is_empty());
    assert!(report.retained_targets.is_empty());
    assert!(report.nonfatal_failures.is_empty());
    assert!(root.join("base/.kitchensync/TMP/2023-12-01_00-00-00_000000Z/old.tmp").exists());
}

#[test]
fn cleanup_retention_reports_nonfatal_failures() {
    let root = next_test_root("cleanup_retention_nonfatal");
    let peer = make_peer_session(42, &root);
    let config = make_run_config(false);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);

    fs::write(root.join("base/.kitchensync/BAK"), "blocked").unwrap();

    let report = executor
        .cleanup_retention(
            &peer,
            &kitchensync::RelPath::new("base").unwrap(),
            kitchensync::Timestamp("2024-01-06_00-00-00_000000Z".to_string()),
            1,
            1,
        )
        .expect("cleanup keeps reporting on nonfatal failure");

    assert!(!report.removed_targets.is_empty() || !report.retained_targets.is_empty() || !report.nonfatal_failures.is_empty());
    assert!(report.nonfatal_failures.iter().any(|failure| failure.target.is_none()));
}

#[test]
fn execute_copy_attempt_replaces_existing_destination_and_archives_old() {
    let source_root = next_test_root("copy_replace_existing");
    let destination_root = next_test_root("copy_replace_existing_dest");
    let source_peer = make_peer_session(50, &source_root);
    let destination_peer = make_peer_session(51, &destination_root);
    let config = make_run_config(false);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);

    write_text(&source_root, "payload.bin", "new payload");
    write_text(&destination_root, "work/final.bin", "old destination");

    let result = executor.execute_copy_attempt(
        &source_peer,
        &kitchensync::RelPath::new("payload.bin").unwrap(),
        &destination_peer,
        &kitchensync::RelPath::new("work/final.bin").unwrap(),
        &entry_meta("payload.bin", "new payload"),
    );

    assert!(result.completed);
    assert_eq!(result.failed_phase, None);
    assert_eq!(result.bytes_copied, 11);
    assert_eq!(read_text(&destination_root, "work/final.bin"), "new payload");
    assert!(destination_root.join("work/.kitchensync/SWAP").exists() == false);

    let bak_root = destination_root.join("work/.kitchensync/BAK");
    let bak_dirs = immediate_subdirs(&bak_root);
    assert_eq!(bak_dirs.len(), 1);
    assert_eq!(read_file_path(&bak_dirs[0].join("final.bin")), "old destination");
}

#[test]
fn execute_copy_attempt_dry_run_reads_source_and_leaves_destination_unchanged() {
    let source_root = next_test_root("copy_dry_source");
    let destination_root = next_test_root("copy_dry_dest");
    let source_peer = make_peer_session(52, &source_root);
    let destination_peer = make_peer_session(53, &destination_root);
    let config = make_run_config(true);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);

    write_text(&source_root, "payload.bin", "payload-value");
    write_text(&destination_root, "work/final.bin", "existing");

    let result = executor.execute_copy_attempt(
        &source_peer,
        &kitchensync::RelPath::new("payload.bin").unwrap(),
        &destination_peer,
        &kitchensync::RelPath::new("work/final.bin").unwrap(),
        &entry_meta("payload.bin", "payload-value"),
    );

    assert!(result.completed);
    assert_eq!(result.failed_phase, None);
    assert_eq!(result.bytes_copied, 13);
    assert_eq!(read_text(&destination_root, "work/final.bin"), "existing");
    assert!(!destination_root.join("work/.kitchensync").exists());
}

#[test]
fn execute_copy_attempt_reports_read_source_when_missing_source() {
    let source_root = next_test_root("copy_read_missing_source");
    let destination_root = next_test_root("copy_read_missing_dest");
    let source_peer = make_peer_session(54, &source_root);
    let destination_peer = make_peer_session(55, &destination_root);
    let config = make_run_config(false);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);

    let result = executor.execute_copy_attempt(
        &source_peer,
        &kitchensync::RelPath::new("missing.bin").unwrap(),
        &destination_peer,
        &kitchensync::RelPath::new("work/final.bin").unwrap(),
        &entry_meta("missing.bin", ""),
    );

    assert!(!result.completed);
    assert_eq!(result.failed_phase, Some(kitchensync::TransferPhase::ReadSource));
    assert_eq!(result.error, Some(kitchensync::TransportError::NotFound));
    assert_eq!(result.bytes_copied, 0);
}

#[test]
fn execute_copy_attempt_reports_rename_final_when_destination_is_directory() {
    let source_root = next_test_root("copy_rename_final_source");
    let destination_root = next_test_root("copy_rename_final_dest");
    let source_peer = make_peer_session(56, &source_root);
    let destination_peer = make_peer_session(57, &destination_root);
    let config = make_run_config(false);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);

    write_text(&source_root, "payload.bin", "new payload");
    fs::create_dir_all(destination_root.join("work/blocked")).unwrap();

    let result = executor.execute_copy_attempt(
        &source_peer,
        &kitchensync::RelPath::new("payload.bin").unwrap(),
        &destination_peer,
        &kitchensync::RelPath::new("work/blocked").unwrap(),
        &entry_meta("payload.bin", "new payload"),
    );

    assert!(!result.completed);
    assert_eq!(result.failed_phase, Some(kitchensync::TransferPhase::RenameFinal));
    assert_eq!(result.bytes_copied, 11);
    assert!(destination_root.join("work/blocked").is_dir());
}

#[test]
fn execute_copy_attempt_reports_set_mod_time_failure() {
    let source_root = next_test_root("copy_set_mod_time");
    let destination_root = next_test_root("copy_set_mod_time_dest");
    let source_peer = make_peer_session(58, &source_root);
    let destination_peer = make_peer_session(59, &destination_root);
    let config = make_run_config(false);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);

    write_text(&source_root, "payload.bin", "fresh payload");
    write_text(&destination_root, "work/final.bin", "old destination");

    let mut meta = entry_meta("payload.bin", "fresh payload");
    meta.mod_time = kitchensync::Timestamp("bad-timestamp".to_string());

    let result = executor.execute_copy_attempt(
        &source_peer,
        &kitchensync::RelPath::new("payload.bin").unwrap(),
        &destination_peer,
        &kitchensync::RelPath::new("work/final.bin").unwrap(),
        &meta,
    );

    assert!(result.completed);
    assert_eq!(result.failed_phase, Some(kitchensync::TransferPhase::SetModTime));
    assert_eq!(read_text(&destination_root, "work/final.bin"), "fresh payload");
}

#[test]
fn execute_copy_attempt_reports_archive_old_when_bak_root_is_file() {
    let source_root = next_test_root("copy_archive_old_source");
    let destination_root = next_test_root("copy_archive_old_dest");
    let source_peer = make_peer_session(60, &source_root);
    let destination_peer = make_peer_session(61, &destination_root);
    let config = make_run_config(false);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);

    write_text(&source_root, "payload.bin", "new payload");
    write_text(&destination_root, "work/final.bin", "old destination");
    fs::write(destination_root.join("work/.kitchensync/BAK"), "blocked").unwrap();

    let result = executor.execute_copy_attempt(
        &source_peer,
        &kitchensync::RelPath::new("payload.bin").unwrap(),
        &destination_peer,
        &kitchensync::RelPath::new("work/final.bin").unwrap(),
        &entry_meta("payload.bin", "new payload"),
    );

    assert!(result.completed);
    assert_eq!(result.failed_phase, Some(kitchensync::TransferPhase::ArchiveOld));
    assert_eq!(read_text(&destination_root, "work/final.bin"), "new payload");
    assert!(destination_root
        .join("work/.kitchensync/SWAP/final.bin/old")
        .exists());
}
