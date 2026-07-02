use std::any::Any;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rusqlite::Connection;
use snapshotstore::{
    SnapshotEntryKind, SnapshotIntendedFileCopy, SnapshotObservedEntry, SnapshotPeerHandle,
    SnapshotPeerRole, SnapshotPeerScheme, SnapshotRunMode, SnapshotStartupFailureKind,
    SnapshotStartupRequest, SnapshotStore, SnapshotStoreError, SNAPSHOT_ROOT_PARENT_ID,
};

const LIVE_MARKER: &str = "2024-01-01_00-00-00_000001Z";
const OLD_MARKER: &str = "2024-01-01_00-00-00_000002Z";
const NEW_MARKER: &str = "2024-01-01_00-00-00_000003Z";

fn subject() -> Arc<dyn SnapshotStore> {
    let database = snapshotstore_snapshotdatabase::new(
        snapshotstore_snapshotdatabase_snapshotcleanup::new(),
        snapshotstore_snapshotdatabase_snapshotfile::new(),
        snapshotstore_snapshotdatabase_snapshotrows::new(),
    );

    snapshotstore::new(
        database.clone(),
        snapshotstore_snapshotidentity::new(),
        snapshotstore_snapshotpeerfiles::new(database),
    )
}

fn temp_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "kitchensync-snapshotstore-tests-{}-{}",
        std::process::id(),
        name
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("create test root");
    root
}

fn local_peer(identity: &str, root: &Path) -> SnapshotPeerHandle {
    SnapshotPeerHandle {
        identity: identity.to_owned(),
        role: SnapshotPeerRole::Normal,
        winning_url: format!("file://{}", root.to_string_lossy()),
        scheme: SnapshotPeerScheme::File,
        handle: Arc::new(root.to_path_buf()) as Arc<dyn Any + Send + Sync>,
    }
}

fn start_one_peer(
    store: &dyn SnapshotStore,
    name: &str,
    peer_root: &Path,
) -> (snapshotstore::SnapshotRunId, PathBuf, bool) {
    let result = store.start_run(SnapshotStartupRequest {
        run_mode: SnapshotRunMode::Normal,
        temporary_root: temp_root(&format!("{name}-tmp")),
        peers: vec![local_peer("peer", peer_root)],
    });

    assert_eq!(result.unavailable_peers, Vec::new());
    assert_eq!(result.available_peers.len(), 1);
    assert_eq!(result.available_peers[0].peer_identity, "peer");

    (
        result.run_id,
        result.available_peers[0].local_snapshot_path.clone(),
        result.available_peers[0].had_snapshot_history,
    )
}

fn is_timestamp(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 27
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'_'
        && bytes[13] == b'-'
        && bytes[16] == b'-'
        && bytes[19] == b'_'
        && bytes[26] == b'Z'
        && bytes.iter().enumerate().all(|(index, byte)| {
            matches!(index, 4 | 7 | 10 | 13 | 16 | 19 | 26)
                || byte.is_ascii_digit()
        })
}

fn create_snapshot_db(path: &Path, mod_time_marker: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create snapshot parent");
    }

    let connection = Connection::open(path).expect("open snapshot db");
    connection
        .execute_batch(
            "
            PRAGMA journal_mode=DELETE;
            CREATE TABLE snapshot (
                id TEXT PRIMARY KEY,
                parent_id TEXT,
                basename TEXT NOT NULL,
                mod_time TEXT NOT NULL,
                byte_size INTEGER NOT NULL,
                last_seen TEXT NULL,
                deleted_time TEXT NULL
            );
            CREATE INDEX snapshot_parent_id_idx ON snapshot(parent_id);
            CREATE INDEX snapshot_last_seen_idx ON snapshot(last_seen);
            CREATE INDEX snapshot_deleted_time_idx ON snapshot(deleted_time);
            ",
        )
        .expect("create snapshot schema");
    connection
        .execute(
            "INSERT INTO snapshot
             (id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time)
             VALUES (?1, ?2, 'docs', ?3, -1, ?4, NULL)",
            (
                "H41WPg3SlMv",
                SNAPSHOT_ROOT_PARENT_ID,
                mod_time_marker,
                "1970-01-02_00-00-00_000000Z",
            ),
        )
        .expect("insert marker row");
}

fn create_invalid_snapshot_db(path: &Path) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create snapshot parent");
    }

    let connection = Connection::open(path).expect("open invalid snapshot db");
    connection
        .execute_batch("CREATE TABLE snapshot (id TEXT PRIMARY KEY);")
        .expect("create invalid snapshot schema");
}

fn create_cleanup_snapshot_db(path: &Path, store: &dyn SnapshotStore) {
    create_snapshot_db(path, LIVE_MARKER);
    let connection = Connection::open(path).expect("open cleanup snapshot db");
    let gone_id = store.path_id("gone.txt").unwrap();
    let orphan_id = store.path_id("lost/child.txt").unwrap();
    let orphan_parent_id = store.parent_path_id("lost/child.txt").unwrap();

    connection
        .execute(
            "INSERT INTO snapshot
             (id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time)
             VALUES (?1, ?2, 'gone.txt', ?3, 9, ?4, ?4)",
            (
                gone_id,
                SNAPSHOT_ROOT_PARENT_ID,
                LIVE_MARKER,
                "1970-01-02_00-00-00_000000Z",
            ),
        )
        .expect("insert old tombstone row");
    connection
        .execute(
            "INSERT INTO snapshot
             (id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time)
             VALUES (?1, ?2, 'child.txt', ?3, 10, ?4, NULL)",
            (
                orphan_id,
                orphan_parent_id,
                LIVE_MARKER,
                "1970-01-02_00-00-00_000000Z",
            ),
        )
        .expect("insert old orphan row");
}

fn marker_mod_time(path: &Path) -> String {
    let connection = Connection::open(path).expect("open snapshot db");
    connection
        .query_row(
            "SELECT mod_time FROM snapshot WHERE basename = 'docs'",
            [],
            |row| row.get::<_, String>(0),
        )
        .expect("read marker row")
}

fn indexed_columns(connection: &Connection) -> Vec<Vec<String>> {
    let index_names: Vec<String> = connection
        .prepare("SELECT name FROM pragma_index_list('snapshot') WHERE origin != 'pk'")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();

    index_names
        .into_iter()
        .map(|name| {
            let escaped_name = name.replace('\'', "''");
            connection
                .prepare(&format!(
                    "SELECT name FROM pragma_index_info('{escaped_name}') ORDER BY seqno"
                ))
                .unwrap()
                .query_map([], |row| row.get(0))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap()
        })
        .collect()
}

#[test]
fn path_ids_and_timestamps_match_the_snapshot_format_rules() {
    let store = subject();

    assert_eq!(store.path_id("docs").unwrap(), "H41WPg3SlMv");
    assert_eq!(store.path_id("docs/readme.txt").unwrap(), "K5EzsWuLZ04");
    assert_eq!(store.path_id("docs/notes").unwrap(), "1pP6ATZM5gH");
    assert_eq!(
        store.parent_path_id("docs/readme.txt").unwrap(),
        "H41WPg3SlMv"
    );
    assert_eq!(store.parent_path_id("docs").unwrap(), SNAPSHOT_ROOT_PARENT_ID);

    for invalid in ["", "/", "/docs", "docs/", "docs//readme", ".", ".."] {
        assert!(matches!(
            store.path_id(invalid),
            Err(SnapshotStoreError::InvalidRelativePath(_))
        ));
    }

    let generated_a = store.generate_timestamp().unwrap();
    let generated_b = store.generate_timestamp().unwrap();
    assert!(is_timestamp(&generated_a));
    assert!(is_timestamp(&generated_b));
    assert!(generated_b > generated_a);
}

#[test]
fn startup_without_live_snapshot_creates_exact_local_database_and_ignores_sidecars() {
    let store = subject();
    let peer_root = temp_root("empty-peer");
    let sidecar = peer_root.join(".kitchensync/snapshot.db-wal");
    fs::create_dir_all(sidecar.parent().unwrap()).expect("create kitchensync dir");
    fs::write(&sidecar, b"not snapshot state").expect("write sidecar");

    let (_run_id, local_snapshot_path, had_history) =
        start_one_peer(&*store, "empty-peer", &peer_root);

    assert!(!had_history);
    assert!(local_snapshot_path.ends_with("snapshot.db"));
    assert!(local_snapshot_path.exists());
    assert!(!local_snapshot_path.with_extension("db-wal").exists());
    assert!(!local_snapshot_path.with_extension("db-shm").exists());

    let connection = Connection::open(&local_snapshot_path).expect("open local db");
    let journal_mode: String = connection
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .expect("read journal mode");
    assert_eq!(journal_mode.to_ascii_lowercase(), "delete");

    let tables: Vec<String> = connection
        .prepare(
            "SELECT name FROM sqlite_master
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
             ORDER BY name",
        )
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(tables, vec!["snapshot"]);

    let views: Vec<String> = connection
        .prepare("SELECT name FROM sqlite_master WHERE type = 'view'")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(views, Vec::<String>::new());

    let columns: Vec<(String, String, i64, i64)> = connection
        .prepare("PRAGMA table_info(snapshot)")
        .unwrap()
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(5)?,
            ))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        columns,
        vec![
            ("id".to_owned(), "TEXT".to_owned(), 0, 1),
            ("parent_id".to_owned(), "TEXT".to_owned(), 0, 0),
            ("basename".to_owned(), "TEXT".to_owned(), 1, 0),
            ("mod_time".to_owned(), "TEXT".to_owned(), 1, 0),
            ("byte_size".to_owned(), "INTEGER".to_owned(), 1, 0),
            ("last_seen".to_owned(), "TEXT".to_owned(), 0, 0),
            ("deleted_time".to_owned(), "TEXT".to_owned(), 0, 0),
        ]
    );

    let indexes = indexed_columns(&connection);
    assert!(indexes.contains(&vec!["parent_id".to_owned()]));
    assert!(indexes.contains(&vec!["last_seen".to_owned()]));
    assert!(indexes.contains(&vec!["deleted_time".to_owned()]));
}

#[test]
fn row_mutations_preserve_snapshot_facts_and_copy_deletion_estimates() {
    let store = subject();
    let peer_root = temp_root("rows-peer");
    let (run_id, _local_snapshot_path, _) = start_one_peer(&*store, "rows-peer", &peer_root);

    let readme_seen = store
        .confirm_present(
            run_id,
            SnapshotObservedEntry {
                peer_identity: "peer".to_owned(),
                relative_path: "docs/readme.txt".to_owned(),
                mod_time: "2024-01-02_03-04-05_000006Z".to_owned(),
                entry_kind: SnapshotEntryKind::File { byte_size: 12 },
            },
        )
        .unwrap();
    let readme = store
        .lookup_row(run_id, "peer", "docs/readme.txt")
        .unwrap()
        .unwrap();
    assert_eq!(readme.id, "K5EzsWuLZ04");
    assert_eq!(readme.parent_id, "H41WPg3SlMv");
    assert_eq!(readme.basename, "readme.txt");
    assert_eq!(readme.mod_time, "2024-01-02_03-04-05_000006Z");
    assert_eq!(readme.byte_size, 12);
    assert_eq!(readme.last_seen.as_deref(), Some(readme_seen.as_str()));
    assert_eq!(readme.deleted_time, None);
    assert!(is_timestamp(&readme_seen));

    store
        .confirm_absent(run_id, "peer", "docs/readme.txt")
        .unwrap();
    let readme_tombstone = store
        .lookup_row(run_id, "peer", "docs/readme.txt")
        .unwrap()
        .unwrap();
    assert_eq!(readme_tombstone.last_seen, Some(readme_seen.clone()));
    assert_eq!(readme_tombstone.deleted_time, Some(readme_seen.clone()));

    store
        .record_intended_file_copy(
            run_id,
            SnapshotIntendedFileCopy {
                destination_peer_identity: "peer".to_owned(),
                destination_relative_path: "copy/new.txt".to_owned(),
                winning_mod_time: "2024-02-03_04-05-06_000007Z".to_owned(),
                winning_byte_size: 34,
            },
        )
        .unwrap();
    let pending = store
        .lookup_row(run_id, "peer", "copy/new.txt")
        .unwrap()
        .unwrap();
    assert_eq!(pending.mod_time, "2024-02-03_04-05-06_000007Z");
    assert_eq!(pending.byte_size, 34);
    assert_eq!(pending.last_seen, None);
    assert_eq!(pending.deleted_time, None);

    let copy_seen = store
        .complete_file_copy(run_id, "peer", "copy/new.txt")
        .unwrap();
    let completed = store
        .lookup_row(run_id, "peer", "copy/new.txt")
        .unwrap()
        .unwrap();
    assert_eq!(completed.last_seen, Some(copy_seen.clone()));
    assert_eq!(completed.deleted_time, None);
    assert!(copy_seen > readme_seen);

    let docs_seen = store
        .complete_directory_creation(
            run_id,
            "peer",
            "docs",
            "2024-03-04_05-06-07_000008Z".to_owned(),
        )
        .unwrap();
    let notes_seen = store
        .confirm_present(
            run_id,
            SnapshotObservedEntry {
                peer_identity: "peer".to_owned(),
                relative_path: "docs/notes".to_owned(),
                mod_time: "2024-03-04_05-06-08_000009Z".to_owned(),
                entry_kind: SnapshotEntryKind::Directory,
            },
        )
        .unwrap();
    store
        .confirm_present(
            run_id,
            SnapshotObservedEntry {
                peer_identity: "peer".to_owned(),
                relative_path: "outside.txt".to_owned(),
                mod_time: "2024-03-04_05-06-09_000010Z".to_owned(),
                entry_kind: SnapshotEntryKind::File { byte_size: 56 },
            },
        )
        .unwrap();

    store
        .complete_directory_displacement(run_id, "peer", "docs")
        .unwrap();
    let docs = store.lookup_row(run_id, "peer", "docs").unwrap().unwrap();
    let notes = store
        .lookup_row(run_id, "peer", "docs/notes")
        .unwrap()
        .unwrap();
    let readme_after_cascade = store
        .lookup_row(run_id, "peer", "docs/readme.txt")
        .unwrap()
        .unwrap();
    let outside = store
        .lookup_row(run_id, "peer", "outside.txt")
        .unwrap()
        .unwrap();
    assert_eq!(docs.byte_size, -1);
    assert_eq!(docs.deleted_time, Some(docs_seen.clone()));
    assert_eq!(notes.last_seen, Some(notes_seen));
    assert_eq!(notes.deleted_time, Some(docs_seen));
    assert_eq!(outside.deleted_time, None);
    assert_eq!(readme_after_cascade.last_seen, Some(readme_seen.clone()));
    assert_eq!(readme_after_cascade.deleted_time, Some(readme_seen));
}

#[test]
fn completed_file_displacement_copies_the_previous_last_seen_value() {
    let store = subject();
    let peer_root = temp_root("file-displacement-peer");
    let (run_id, _local_snapshot_path, _) =
        start_one_peer(&*store, "file-displacement-peer", &peer_root);

    let seen = store
        .confirm_present(
            run_id,
            SnapshotObservedEntry {
                peer_identity: "peer".to_owned(),
                relative_path: "old.txt".to_owned(),
                mod_time: "2024-04-05_06-07-08_000009Z".to_owned(),
                entry_kind: SnapshotEntryKind::File { byte_size: 21 },
            },
        )
        .unwrap();

    store
        .complete_file_displacement(run_id, "peer", "old.txt")
        .unwrap();

    let displaced = store
        .lookup_row(run_id, "peer", "old.txt")
        .unwrap()
        .unwrap();
    assert_eq!(displaced.last_seen, Some(seen.clone()));
    assert_eq!(displaced.deleted_time, Some(seen));
}

#[test]
fn cleanup_removes_old_tombstones_and_old_orphan_rows() {
    let store = subject();
    let peer_root = temp_root("cleanup-peer");
    create_cleanup_snapshot_db(&peer_root.join(".kitchensync/snapshot.db"), &*store);

    let (run_id, _local_snapshot_path, had_history) =
        start_one_peer(&*store, "cleanup-peer", &peer_root);
    assert!(had_history);

    store.cleanup_peer(run_id, "peer", 1).unwrap();

    assert!(store.lookup_row(run_id, "peer", "gone.txt").unwrap().is_none());
    assert!(store
        .lookup_row(run_id, "peer", "lost/child.txt")
        .unwrap()
        .is_none());
    assert!(store.lookup_row(run_id, "peer", "docs").unwrap().is_some());
}

#[test]
fn normal_startup_recovers_all_snapshot_swap_states_before_download() {
    let cases = [
        ("old-live-new", true, true, true, LIVE_MARKER),
        ("old-new", false, true, true, NEW_MARKER),
        ("old-only", false, true, false, OLD_MARKER),
        ("live-new", true, false, true, LIVE_MARKER),
        ("new-only", false, false, true, NEW_MARKER),
    ];

    for (name, has_live, has_old, has_new, expected_marker) in cases {
        let store = subject();
        let peer_root = temp_root(name);
        let live = peer_root.join(".kitchensync/snapshot.db");
        let old = peer_root.join(".kitchensync/SWAP/snapshot.db/old");
        let new = peer_root.join(".kitchensync/SWAP/snapshot.db/new");

        if has_live {
            create_snapshot_db(&live, LIVE_MARKER);
        }
        if has_old {
            create_snapshot_db(&old, OLD_MARKER);
        }
        if has_new {
            create_snapshot_db(&new, NEW_MARKER);
        }

        let (_run_id, local_snapshot_path, had_history) =
            start_one_peer(&*store, name, &peer_root);

        assert!(had_history, "{name} should retain snapshot history");
        assert_eq!(marker_mod_time(&live), expected_marker);
        assert_eq!(marker_mod_time(&local_snapshot_path), expected_marker);
        assert!(!old.exists(), "{name} should remove SWAP old");
        assert!(!new.exists(), "{name} should remove SWAP new");
    }
}

#[test]
fn invalid_live_snapshot_schema_reports_the_peer_unavailable() {
    let store = subject();
    let peer_root = temp_root("invalid-schema-peer");
    create_invalid_snapshot_db(&peer_root.join(".kitchensync/snapshot.db"));

    let result = store.start_run(SnapshotStartupRequest {
        run_mode: SnapshotRunMode::Normal,
        temporary_root: temp_root("invalid-schema-tmp"),
        peers: vec![local_peer("peer", &peer_root)],
    });

    assert_eq!(result.available_peers, Vec::new());
    assert_eq!(result.unavailable_peers.len(), 1);
    assert_eq!(result.unavailable_peers[0].peer_identity, "peer");
    assert_eq!(
        result.unavailable_peers[0].diagnostic.kind,
        SnapshotStartupFailureKind::LocalDatabaseFailed
    );
}

#[test]
fn normal_upload_replaces_live_snapshot_with_a_closed_self_contained_database() {
    let store = subject();
    let peer_root = temp_root("upload-peer");
    let live = peer_root.join(".kitchensync/snapshot.db");
    let old = peer_root.join(".kitchensync/SWAP/snapshot.db/old");
    let new = peer_root.join(".kitchensync/SWAP/snapshot.db/new");
    create_snapshot_db(&live, LIVE_MARKER);

    let (run_id, _local_snapshot_path, had_history) =
        start_one_peer(&*store, "upload-peer", &peer_root);
    assert!(had_history);

    store
        .confirm_present(
            run_id,
            SnapshotObservedEntry {
                peer_identity: "peer".to_owned(),
                relative_path: "docs/readme.txt".to_owned(),
                mod_time: "2025-01-02_03-04-05_000006Z".to_owned(),
                entry_kind: SnapshotEntryKind::File { byte_size: 99 },
            },
        )
        .unwrap();

    let peer_side_before_upload: i64 = Connection::open(&live)
        .expect("open live db before upload")
        .query_row(
            "SELECT COUNT(*) FROM snapshot WHERE id = 'K5EzsWuLZ04'",
            [],
            |row| row.get(0),
        )
        .expect("count live rows before upload");
    assert_eq!(peer_side_before_upload, 0);

    let upload = store.upload_snapshots(run_id).expect("upload snapshots");
    assert_eq!(upload.uploaded_peers, vec!["peer".to_owned()]);
    assert_eq!(upload.failed_peers, Vec::new());

    assert!(live.exists());
    assert!(!old.exists());
    assert!(!new.exists());
    assert!(!live.with_extension("db-wal").exists());
    assert!(!live.with_extension("db-shm").exists());
    assert!(!live.with_extension("db-journal").exists());

    let connection = Connection::open(&live).expect("open uploaded db");
    let uploaded: (String, i64, Option<String>, Option<String>) = connection
        .query_row(
            "SELECT mod_time, byte_size, last_seen, deleted_time
             FROM snapshot WHERE id = 'K5EzsWuLZ04'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("read uploaded row");
    assert_eq!(uploaded.0, "2025-01-02_03-04-05_000006Z");
    assert_eq!(uploaded.1, 99);
    assert!(uploaded.2.as_deref().is_some_and(is_timestamp));
    assert_eq!(uploaded.3, None);
}

#[test]
fn dry_run_upload_is_rejected_without_peer_mutation() {
    let store = subject();
    let peer_root = temp_root("dry-run-peer");
    let live = peer_root.join(".kitchensync/snapshot.db");
    let old = peer_root.join(".kitchensync/SWAP/snapshot.db/old");
    let new = peer_root.join(".kitchensync/SWAP/snapshot.db/new");
    create_snapshot_db(&live, LIVE_MARKER);
    create_snapshot_db(&old, OLD_MARKER);
    create_snapshot_db(&new, NEW_MARKER);

    let result = store.start_run(SnapshotStartupRequest {
        run_mode: SnapshotRunMode::DryRun,
        temporary_root: temp_root("dry-run-tmp"),
        peers: vec![local_peer("peer", &peer_root)],
    });
    assert_eq!(result.unavailable_peers, Vec::new());
    assert_eq!(result.available_peers.len(), 1);

    assert!(matches!(
        store.upload_snapshots(result.run_id),
        Err(SnapshotStoreError::DryRunUploadForbidden)
    ));
    assert_eq!(marker_mod_time(&live), LIVE_MARKER);
    assert_eq!(marker_mod_time(&old), OLD_MARKER);
    assert_eq!(marker_mod_time(&new), NEW_MARKER);
}
