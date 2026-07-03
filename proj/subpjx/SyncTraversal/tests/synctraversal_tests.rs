use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use formatrules::FormatRules;
use peertransportsurface::{
    ConnectedPeerRoot, PeerMetadata, PeerReadChunk, PeerTransportError, PeerTransportSurface,
};
use snapshotdatabase::{
    SnapshotDatabase, SnapshotDatabaseConfirmedAbsenceRequest, SnapshotDatabaseEntryIdentity,
    SnapshotDatabaseListedDirectoryRequest, SnapshotDatabaseListedFileRequest,
    SnapshotDatabasePeerDatabase, SnapshotDatabaseRow,
};
use synctraversal::{
    new, SyncTraversal, SyncTraversalDiagnostic, SyncTraversalDiagnosticKind,
    SyncTraversalDiagnosticLevel, SyncTraversalPeer, SyncTraversalPeerRole,
    SyncTraversalRequest,
};

static NEXT_TEST_ROOT: AtomicUsize = AtomicUsize::new(0);

struct TestWorld {
    path: PathBuf,
}

impl TestWorld {
    fn new(name: &str) -> Self {
        let id = NEXT_TEST_ROOT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "kitchensync_synctraversal_{name}_{}_{}",
            std::process::id(),
            id
        ));
        let _ = fs::remove_dir_all(&path);
        let _ = fs::remove_file(&path);
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn peer_path(&self, peer_index: usize) -> PathBuf {
        self.path.join("peers").join(peer_index.to_string())
    }

    fn db_path(&self, peer_index: usize) -> PathBuf {
        self.path.join("dbs").join(format!("{peer_index}.db"))
    }

    fn peer_root(&self, peer_index: usize) -> ConnectedPeerRoot {
        ConnectedPeerRoot {
            handle: Arc::new(self.peer_path(peer_index)),
        }
    }

    fn child(&self, peer_index: usize, relative_path: &str) -> PathBuf {
        relative_path
            .split('/')
            .fold(self.peer_path(peer_index), |path, part| path.join(part))
    }
}

impl Drop for TestWorld {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
        let _ = fs::remove_file(&self.path);
    }
}

struct Subject {
    sync_traversal: Arc<dyn SyncTraversal>,
    format_rules: Arc<dyn FormatRules>,
    peer_transport_surface: Arc<dyn PeerTransportSurface>,
    snapshot_database: Arc<dyn SnapshotDatabase>,
}

fn subject() -> Subject {
    let format_rules = formatrules::new();
    let peer_transport_surface = peertransportsurface::new();
    let snapshot_database =
        snapshotdatabase::new(format_rules.clone(), peer_transport_surface.clone());
    let copy_staging = copystaging::new(format_rules.clone(), peer_transport_surface.clone());
    let sync_traversal = new(
        format_rules.clone(),
        peer_transport_surface.clone(),
        snapshot_database.clone(),
        copy_staging,
    );

    Subject {
        sync_traversal,
        format_rules,
        peer_transport_surface,
        snapshot_database,
    }
}

fn request(peers: Vec<SyncTraversalPeer>, excludes: Vec<&str>) -> SyncTraversalRequest {
    SyncTraversalRequest {
        peers,
        retries_list: 2,
        excludes: excludes.into_iter().map(str::to_string).collect(),
    }
}

fn create_peer(
    subject: &Subject,
    world: &TestWorld,
    peer_index: usize,
    role: SyncTraversalPeerRole,
    had_snapshot_history: bool,
) -> SyncTraversalPeer {
    fs::create_dir_all(world.peer_path(peer_index)).unwrap();
    create_snapshot_database(subject, world, peer_index);
    SyncTraversalPeer {
        peer_index,
        peer_url: format!("peer-{peer_index}"),
        role,
        had_snapshot_history,
        root: world.peer_root(peer_index),
        snapshot_database: database(world, peer_index),
    }
}

fn missing_peer(
    subject: &Subject,
    world: &TestWorld,
    peer_index: usize,
    role: SyncTraversalPeerRole,
) -> SyncTraversalPeer {
    let _ = fs::remove_dir_all(world.peer_path(peer_index));
    let _ = fs::remove_file(world.peer_path(peer_index));
    create_snapshot_database(subject, world, peer_index);
    SyncTraversalPeer {
        peer_index,
        peer_url: format!("peer-{peer_index}"),
        role,
        had_snapshot_history: true,
        root: world.peer_root(peer_index),
        snapshot_database: database(world, peer_index),
    }
}

fn create_snapshot_database(subject: &Subject, world: &TestWorld, peer_index: usize) {
    let path = world.db_path(peer_index);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    subject
        .snapshot_database
        .create_snapshot_database(path)
        .unwrap();
}

fn database(world: &TestWorld, peer_index: usize) -> SnapshotDatabasePeerDatabase {
    SnapshotDatabasePeerDatabase {
        peer_index,
        local_snapshot_path: world.db_path(peer_index),
    }
}

fn identity(format_rules: &dyn FormatRules, relative_path: &str) -> SnapshotDatabaseEntryIdentity {
    let ids = format_rules.snapshot_path_ids(relative_path).unwrap();
    SnapshotDatabaseEntryIdentity {
        id: ids.id,
        parent_id: ids.parent_id,
        basename: relative_path.rsplit('/').next().unwrap().to_string(),
    }
}

fn timestamp(format_rules: &dyn FormatRules, seconds: u64) -> String {
    format_rules.timestamp_text(
        &format_rules.format_timestamp(UNIX_EPOCH + Duration::from_secs(seconds)),
    )
}

fn system_time(seconds: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(seconds)
}

fn write_file(subject: &Subject, world: &TestWorld, peer_index: usize, path: &str, bytes: &[u8]) {
    let full_path = world.child(peer_index, path);
    fs::create_dir_all(full_path.parent().unwrap()).unwrap();
    fs::write(full_path, bytes).unwrap();
    subject
        .peer_transport_surface
        .set_mod_time(&world.peer_root(peer_index), path, system_time(1_700_000_000))
        .unwrap();
}

fn write_file_at(
    subject: &Subject,
    world: &TestWorld,
    peer_index: usize,
    path: &str,
    bytes: &[u8],
    seconds: u64,
) {
    write_file(subject, world, peer_index, path, bytes);
    subject
        .peer_transport_surface
        .set_mod_time(&world.peer_root(peer_index), path, system_time(seconds))
        .unwrap();
}

fn create_dir_at(
    subject: &Subject,
    world: &TestWorld,
    peer_index: usize,
    path: &str,
    seconds: u64,
) {
    fs::create_dir_all(world.child(peer_index, path)).unwrap();
    subject
        .peer_transport_surface
        .set_mod_time(&world.peer_root(peer_index), path, system_time(seconds))
        .unwrap();
}

fn read_all(subject: &Subject, world: &TestWorld, peer_index: usize, path: &str) -> Vec<u8> {
    let peer = world.peer_root(peer_index);
    let mut handle = subject.peer_transport_surface.open_read(&peer, path).unwrap();
    let mut bytes = Vec::new();

    loop {
        match subject
            .peer_transport_surface
            .read(&mut handle, 1024)
            .unwrap()
        {
            PeerReadChunk::Bytes(chunk) => bytes.extend(chunk),
            PeerReadChunk::Eof => break,
        }
    }

    subject.peer_transport_surface.close_read(handle).unwrap();
    bytes
}

fn stat(subject: &Subject, world: &TestWorld, peer_index: usize, path: &str) -> PeerMetadata {
    subject
        .peer_transport_surface
        .stat(&world.peer_root(peer_index), path)
        .unwrap()
}

fn assert_missing(subject: &Subject, world: &TestWorld, peer_index: usize, path: &str) {
    assert_eq!(
        subject
            .peer_transport_surface
            .stat(&world.peer_root(peer_index), path),
        Err(PeerTransportError::NotFound)
    );
}

fn read_row(
    subject: &Subject,
    world: &TestWorld,
    peer_index: usize,
    relative_path: &str,
) -> Option<SnapshotDatabaseRow> {
    let entry_id = identity(subject.format_rules.as_ref(), relative_path).id;
    subject
        .snapshot_database
        .read_snapshot_row(database(world, peer_index), entry_id)
        .unwrap()
}

fn record_file_row(
    subject: &Subject,
    world: &TestWorld,
    peer_index: usize,
    relative_path: &str,
    seconds: u64,
    byte_size: i64,
) {
    subject
        .snapshot_database
        .record_listed_file(SnapshotDatabaseListedFileRequest {
            database: database(world, peer_index),
            entry: identity(subject.format_rules.as_ref(), relative_path),
            mod_time: timestamp(subject.format_rules.as_ref(), seconds),
            byte_size,
            last_seen: timestamp(subject.format_rules.as_ref(), seconds),
        })
        .unwrap();
}

fn record_directory_row(
    subject: &Subject,
    world: &TestWorld,
    peer_index: usize,
    relative_path: &str,
    seconds: u64,
) {
    subject
        .snapshot_database
        .record_listed_directory(SnapshotDatabaseListedDirectoryRequest {
            database: database(world, peer_index),
            entry: identity(subject.format_rules.as_ref(), relative_path),
            mod_time: timestamp(subject.format_rules.as_ref(), seconds),
            last_seen: timestamp(subject.format_rules.as_ref(), seconds),
        })
        .unwrap();
}

fn tombstone_row(subject: &Subject, world: &TestWorld, peer_index: usize, relative_path: &str) {
    let entry_id = identity(subject.format_rules.as_ref(), relative_path).id;
    subject
        .snapshot_database
        .record_confirmed_absence(SnapshotDatabaseConfirmedAbsenceRequest {
            database: database(world, peer_index),
            entry_id,
        })
        .unwrap();
}

#[test]
fn empty_reachable_peer_set_has_no_work() {
    let subject = subject();

    let result = subject.sync_traversal.traverse(request(Vec::new(), vec![]));

    assert_eq!(result.diagnostics, Vec::<SyncTraversalDiagnostic>::new());
}

#[test]
fn canon_file_is_copied_to_normal_and_subordinate_peers() {
    let subject = subject();
    let world = TestWorld::new("canon_file");
    let canon = create_peer(&subject, &world, 0, SyncTraversalPeerRole::Canon, true);
    let normal = create_peer(&subject, &world, 1, SyncTraversalPeerRole::Normal, true);
    let subordinate = create_peer(&subject, &world, 2, SyncTraversalPeerRole::Subordinate, true);
    write_file_at(&subject, &world, 0, "file.txt", b"canon", 1_700_000_100);
    write_file_at(&subject, &world, 2, "file.txt", b"oldold", 1_700_000_000);

    let result = subject
        .sync_traversal
        .traverse(request(vec![canon, normal, subordinate], vec![]));

    assert_eq!(result.diagnostics, Vec::<SyncTraversalDiagnostic>::new());
    assert_eq!(read_all(&subject, &world, 1, "file.txt"), b"canon");
    assert_eq!(read_all(&subject, &world, 2, "file.txt"), b"canon");
    assert_eq!(read_row(&subject, &world, 0, "file.txt").unwrap().byte_size, 5);
    assert_eq!(read_row(&subject, &world, 1, "file.txt").unwrap().byte_size, 5);
    assert_eq!(read_row(&subject, &world, 2, "file.txt").unwrap().byte_size, 5);
}

#[test]
fn canon_absence_displaces_live_peer_file_and_tombstones_its_snapshot_row() {
    let subject = subject();
    let world = TestWorld::new("canon_absence");
    let canon = create_peer(&subject, &world, 0, SyncTraversalPeerRole::Canon, true);
    let normal = create_peer(&subject, &world, 1, SyncTraversalPeerRole::Normal, true);
    write_file_at(&subject, &world, 1, "gone.txt", b"remove", 1_700_000_100);
    record_file_row(&subject, &world, 1, "gone.txt", 1_700_000_090, 6);

    let result = subject
        .sync_traversal
        .traverse(request(vec![canon, normal], vec![]));

    assert_eq!(result.diagnostics, Vec::<SyncTraversalDiagnostic>::new());
    assert_missing(&subject, &world, 1, "gone.txt");
    let row = read_row(&subject, &world, 1, "gone.txt").unwrap();
    assert_eq!(row.deleted_time, row.last_seen);
}

#[test]
fn command_line_file_exclude_leaves_live_files_and_snapshot_rows_unchanged() {
    let subject = subject();
    let world = TestWorld::new("file_exclude");
    let peer0 = create_peer(&subject, &world, 0, SyncTraversalPeerRole::Normal, true);
    let peer1 = create_peer(&subject, &world, 1, SyncTraversalPeerRole::Normal, true);
    write_file_at(&subject, &world, 0, "skip.txt", b"new", 1_700_000_200);
    write_file_at(&subject, &world, 1, "skip.txt", b"old", 1_700_000_000);
    record_file_row(&subject, &world, 1, "skip.txt", 1_700_000_000, 3);
    let row_before = read_row(&subject, &world, 1, "skip.txt");

    let result = subject
        .sync_traversal
        .traverse(request(vec![peer0, peer1], vec!["skip.txt"]));

    assert_eq!(result.diagnostics, Vec::<SyncTraversalDiagnostic>::new());
    assert_eq!(read_all(&subject, &world, 1, "skip.txt"), b"old");
    assert_eq!(read_row(&subject, &world, 0, "skip.txt"), None);
    assert_eq!(read_row(&subject, &world, 1, "skip.txt"), row_before);
}

#[test]
fn command_line_directory_exclude_leaves_descendants_unchanged() {
    let subject = subject();
    let world = TestWorld::new("directory_exclude");
    let peer0 = create_peer(&subject, &world, 0, SyncTraversalPeerRole::Normal, true);
    let peer1 = create_peer(&subject, &world, 1, SyncTraversalPeerRole::Normal, true);
    write_file_at(
        &subject,
        &world,
        0,
        "skip/child.txt",
        b"new",
        1_700_000_200,
    );
    write_file_at(
        &subject,
        &world,
        1,
        "skip/child.txt",
        b"old",
        1_700_000_000,
    );
    record_file_row(&subject, &world, 1, "skip/child.txt", 1_700_000_000, 3);
    let row_before = read_row(&subject, &world, 1, "skip/child.txt");

    let result = subject
        .sync_traversal
        .traverse(request(vec![peer0, peer1], vec!["skip"]));

    assert_eq!(result.diagnostics, Vec::<SyncTraversalDiagnostic>::new());
    assert_eq!(read_all(&subject, &world, 1, "skip/child.txt"), b"old");
    assert_eq!(read_row(&subject, &world, 0, "skip/child.txt"), None);
    assert_eq!(read_row(&subject, &world, 1, "skip/child.txt"), row_before);
}

#[test]
fn builtin_metadata_directories_are_excluded_from_the_user_tree() {
    let subject = subject();
    let world = TestWorld::new("builtin_excludes");
    let peer0 = create_peer(&subject, &world, 0, SyncTraversalPeerRole::Canon, true);
    let peer1 = create_peer(&subject, &world, 1, SyncTraversalPeerRole::Normal, true);
    write_file(&subject, &world, 0, ".kitchensync/user.txt", b"metadata");
    write_file(&subject, &world, 0, ".git/config", b"git");

    let result = subject
        .sync_traversal
        .traverse(request(vec![peer0, peer1], vec![]));

    assert_eq!(result.diagnostics, Vec::<SyncTraversalDiagnostic>::new());
    assert_missing(&subject, &world, 1, ".kitchensync/user.txt");
    assert_missing(&subject, &world, 1, ".git/config");
}

#[test]
fn snapshot_rows_do_not_add_entries_to_the_traversal() {
    let subject = subject();
    let world = TestWorld::new("snapshot_rows_do_not_add");
    let peer0 = create_peer(&subject, &world, 0, SyncTraversalPeerRole::Normal, true);
    let peer1 = create_peer(&subject, &world, 1, SyncTraversalPeerRole::Normal, true);
    record_file_row(&subject, &world, 0, "ghost.txt", 1_700_000_000, 5);
    let row_before = read_row(&subject, &world, 0, "ghost.txt");

    let result = subject
        .sync_traversal
        .traverse(request(vec![peer0, peer1], vec![]));

    assert_eq!(result.diagnostics, Vec::<SyncTraversalDiagnostic>::new());
    assert_missing(&subject, &world, 0, "ghost.txt");
    assert_missing(&subject, &world, 1, "ghost.txt");
    assert_eq!(read_row(&subject, &world, 0, "ghost.txt"), row_before);
    assert_eq!(read_row(&subject, &world, 1, "ghost.txt"), None);
}

#[test]
fn listing_failure_reports_one_error_diagnostic_for_the_failed_root() {
    let subject = subject();
    let world = TestWorld::new("listing_failure");
    let failed = missing_peer(&subject, &world, 0, SyncTraversalPeerRole::Normal);

    let result = subject.sync_traversal.traverse(request(vec![failed], vec![]));

    assert_eq!(
        result.diagnostics,
        vec![SyncTraversalDiagnostic {
            level: SyncTraversalDiagnosticLevel::Error,
            peer_index: 0,
            path: None,
            kind: SyncTraversalDiagnosticKind::DirectoryListingFailed(
                PeerTransportError::NotFound,
            ),
        }]
    );
}

#[test]
fn canon_listing_failure_skips_changes_for_all_peers_under_that_subtree() {
    let subject = subject();
    let world = TestWorld::new("canon_listing_failure");
    let canon = missing_peer(&subject, &world, 0, SyncTraversalPeerRole::Canon);
    let normal = create_peer(&subject, &world, 1, SyncTraversalPeerRole::Normal, true);
    write_file_at(&subject, &world, 1, "survivor.txt", b"keep", 1_700_000_000);
    record_file_row(&subject, &world, 1, "survivor.txt", 1_700_000_000, 4);
    let row_before = read_row(&subject, &world, 1, "survivor.txt");

    let result = subject
        .sync_traversal
        .traverse(request(vec![canon, normal], vec![]));

    assert_eq!(
        result.diagnostics,
        vec![SyncTraversalDiagnostic {
            level: SyncTraversalDiagnosticLevel::Error,
            peer_index: 0,
            path: None,
            kind: SyncTraversalDiagnosticKind::DirectoryListingFailed(
                PeerTransportError::NotFound,
            ),
        }]
    );
    assert_eq!(read_all(&subject, &world, 1, "survivor.txt"), b"keep");
    assert_eq!(read_row(&subject, &world, 1, "survivor.txt"), row_before);
}

#[test]
fn peer_without_snapshot_history_receives_but_does_not_contribute_to_decisions() {
    let subject = subject();
    let world = TestWorld::new("no_history_peer");
    let history_peer = create_peer(&subject, &world, 0, SyncTraversalPeerRole::Normal, true);
    let no_history_peer = create_peer(&subject, &world, 1, SyncTraversalPeerRole::Normal, false);
    write_file_at(&subject, &world, 0, "file.txt", b"older", 1_700_000_000);
    write_file_at(&subject, &world, 1, "file.txt", b"newer", 1_700_000_200);

    let result = subject
        .sync_traversal
        .traverse(request(vec![history_peer, no_history_peer], vec![]));

    assert_eq!(result.diagnostics, Vec::<SyncTraversalDiagnostic>::new());
    assert_eq!(read_all(&subject, &world, 0, "file.txt"), b"older");
    assert_eq!(read_all(&subject, &world, 1, "file.txt"), b"older");
}

#[test]
fn subordinate_file_does_not_make_file_type_win_over_contributing_directory() {
    let subject = subject();
    let world = TestWorld::new("subordinate_type_conflict");
    let normal = create_peer(&subject, &world, 0, SyncTraversalPeerRole::Normal, true);
    let subordinate = create_peer(&subject, &world, 1, SyncTraversalPeerRole::Subordinate, true);
    create_dir_at(&subject, &world, 0, "thing", 1_700_000_000);
    write_file_at(&subject, &world, 0, "thing/child.txt", b"child", 1_700_000_100);
    write_file_at(&subject, &world, 1, "thing", b"wrong", 1_700_000_200);

    let result = subject
        .sync_traversal
        .traverse(request(vec![normal, subordinate], vec![]));

    assert_eq!(result.diagnostics, Vec::<SyncTraversalDiagnostic>::new());
    assert!(stat(&subject, &world, 1, "thing").is_dir);
    assert_eq!(read_all(&subject, &world, 1, "thing/child.txt"), b"child");
}

#[test]
fn file_type_wins_type_conflict_between_contributing_peers() {
    let subject = subject();
    let world = TestWorld::new("contributing_type_conflict");
    let peer0 = create_peer(&subject, &world, 0, SyncTraversalPeerRole::Normal, true);
    let peer1 = create_peer(&subject, &world, 1, SyncTraversalPeerRole::Normal, true);
    write_file_at(&subject, &world, 0, "thing", b"file", 1_700_000_100);
    create_dir_at(&subject, &world, 1, "thing", 1_700_000_000);
    write_file_at(&subject, &world, 1, "thing/child.txt", b"child", 1_700_000_000);

    let result = subject
        .sync_traversal
        .traverse(request(vec![peer0, peer1], vec![]));

    assert_eq!(result.diagnostics, Vec::<SyncTraversalDiagnostic>::new());
    let metadata = stat(&subject, &world, 1, "thing");
    assert!(!metadata.is_dir);
    assert_eq!(read_all(&subject, &world, 1, "thing"), b"file");
}

#[test]
fn newer_file_more_than_five_seconds_newer_than_every_live_file_wins() {
    let subject = subject();
    let world = TestWorld::new("newer_file_wins");
    let peer0 = create_peer(&subject, &world, 0, SyncTraversalPeerRole::Normal, true);
    let peer1 = create_peer(&subject, &world, 1, SyncTraversalPeerRole::Normal, true);
    write_file_at(&subject, &world, 0, "file.txt", b"old", 1_700_000_000);
    write_file_at(&subject, &world, 1, "file.txt", b"new", 1_700_000_006);

    let result = subject
        .sync_traversal
        .traverse(request(vec![peer0, peer1], vec![]));

    assert_eq!(result.diagnostics, Vec::<SyncTraversalDiagnostic>::new());
    assert_eq!(read_all(&subject, &world, 0, "file.txt"), b"new");
    assert_eq!(read_all(&subject, &world, 1, "file.txt"), b"new");
}

#[test]
fn larger_file_wins_when_live_mod_times_are_tied() {
    let subject = subject();
    let world = TestWorld::new("larger_file_wins");
    let peer0 = create_peer(&subject, &world, 0, SyncTraversalPeerRole::Normal, true);
    let peer1 = create_peer(&subject, &world, 1, SyncTraversalPeerRole::Normal, true);
    write_file_at(&subject, &world, 0, "file.txt", b"small", 1_700_000_000);
    write_file_at(
        &subject,
        &world,
        1,
        "file.txt",
        b"larger",
        1_700_000_003,
    );

    let result = subject
        .sync_traversal
        .traverse(request(vec![peer0, peer1], vec![]));

    assert_eq!(result.diagnostics, Vec::<SyncTraversalDiagnostic>::new());
    assert_eq!(read_all(&subject, &world, 0, "file.txt"), b"larger");
    assert_eq!(read_all(&subject, &world, 1, "file.txt"), b"larger");
}

#[test]
fn exactly_tied_file_versions_keep_each_source_bytes() {
    let subject = subject();
    let world = TestWorld::new("exact_file_tie");
    let peer0 = create_peer(&subject, &world, 0, SyncTraversalPeerRole::Normal, true);
    let peer1 = create_peer(&subject, &world, 1, SyncTraversalPeerRole::Normal, true);
    let peer2 = create_peer(&subject, &world, 2, SyncTraversalPeerRole::Normal, true);
    write_file_at(&subject, &world, 0, "file.txt", b"left", 1_700_000_000);
    write_file_at(&subject, &world, 1, "file.txt", b"rght", 1_700_000_000);

    let result = subject
        .sync_traversal
        .traverse(request(vec![peer0, peer1, peer2], vec![]));

    assert_eq!(result.diagnostics, Vec::<SyncTraversalDiagnostic>::new());
    assert_eq!(read_all(&subject, &world, 0, "file.txt"), b"left");
    assert_eq!(read_all(&subject, &world, 1, "file.txt"), b"rght");
    let received = read_all(&subject, &world, 2, "file.txt");
    assert!(received == b"left" || received == b"rght");
    assert_eq!(stat(&subject, &world, 2, "file.txt").byte_size, 4);
}

#[test]
fn deletion_evidence_more_than_five_seconds_newer_than_live_file_wins() {
    let subject = subject();
    let world = TestWorld::new("file_deletion_wins");
    let peer0 = create_peer(&subject, &world, 0, SyncTraversalPeerRole::Normal, true);
    let peer1 = create_peer(&subject, &world, 1, SyncTraversalPeerRole::Normal, true);
    record_file_row(&subject, &world, 0, "file.txt", 1_700_000_010, 4);
    tombstone_row(&subject, &world, 0, "file.txt");
    write_file_at(&subject, &world, 1, "file.txt", b"live", 1_700_000_004);

    let result = subject
        .sync_traversal
        .traverse(request(vec![peer0, peer1], vec![]));

    assert_eq!(result.diagnostics, Vec::<SyncTraversalDiagnostic>::new());
    assert_missing(&subject, &world, 1, "file.txt");
}

#[test]
fn live_file_wins_when_deletion_evidence_is_within_five_seconds() {
    let subject = subject();
    let world = TestWorld::new("file_live_ties_deletion");
    let peer0 = create_peer(&subject, &world, 0, SyncTraversalPeerRole::Normal, true);
    let peer1 = create_peer(&subject, &world, 1, SyncTraversalPeerRole::Normal, true);
    record_file_row(&subject, &world, 0, "file.txt", 1_700_000_010, 4);
    tombstone_row(&subject, &world, 0, "file.txt");
    write_file_at(&subject, &world, 1, "file.txt", b"live", 1_700_000_005);

    let result = subject
        .sync_traversal
        .traverse(request(vec![peer0, peer1], vec![]));

    assert_eq!(result.diagnostics, Vec::<SyncTraversalDiagnostic>::new());
    assert_eq!(read_all(&subject, &world, 0, "file.txt"), b"live");
    assert_eq!(read_all(&subject, &world, 1, "file.txt"), b"live");
}

#[test]
fn live_directory_with_no_files_loses_to_directory_deletion_evidence() {
    let subject = subject();
    let world = TestWorld::new("empty_directory_deletion");
    let peer0 = create_peer(&subject, &world, 0, SyncTraversalPeerRole::Normal, true);
    let peer1 = create_peer(&subject, &world, 1, SyncTraversalPeerRole::Normal, true);
    record_directory_row(&subject, &world, 0, "dir", 1_700_000_010);
    tombstone_row(&subject, &world, 0, "dir");
    create_dir_at(&subject, &world, 1, "dir", 1_700_000_000);

    let result = subject
        .sync_traversal
        .traverse(request(vec![peer0, peer1], vec![]));

    assert_eq!(result.diagnostics, Vec::<SyncTraversalDiagnostic>::new());
    assert_missing(&subject, &world, 1, "dir");
}

#[test]
fn live_subtree_file_within_five_seconds_of_deletion_evidence_keeps_directory() {
    let subject = subject();
    let world = TestWorld::new("directory_survives");
    let peer0 = create_peer(&subject, &world, 0, SyncTraversalPeerRole::Normal, true);
    let peer1 = create_peer(&subject, &world, 1, SyncTraversalPeerRole::Normal, true);
    record_directory_row(&subject, &world, 0, "dir", 1_700_000_010);
    tombstone_row(&subject, &world, 0, "dir");
    create_dir_at(&subject, &world, 1, "dir", 1_700_000_000);
    write_file_at(&subject, &world, 1, "dir/live.txt", b"live", 1_700_000_005);

    let result = subject
        .sync_traversal
        .traverse(request(vec![peer0, peer1], vec![]));

    assert_eq!(result.diagnostics, Vec::<SyncTraversalDiagnostic>::new());
    assert!(stat(&subject, &world, 0, "dir").is_dir);
    assert_eq!(read_all(&subject, &world, 0, "dir/live.txt"), b"live");
    assert!(stat(&subject, &world, 1, "dir").is_dir);
}
