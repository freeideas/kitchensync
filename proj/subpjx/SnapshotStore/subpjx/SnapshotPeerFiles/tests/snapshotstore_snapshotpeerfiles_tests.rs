use std::any::Any;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use snapshotstore_snapshotdatabase::SnapshotDatabase;
use snapshotstore_snapshotpeerfiles::{
    new, SnapshotPeerFiles, SnapshotPeerFilesConnectedPeer,
    SnapshotPeerFilesPeerScheme, SnapshotPeerFilesStartupRequest,
    SnapshotPeerFilesStartupResult, SnapshotPeerFilesUploadRequest,
};

const LIVE_TEXT: &str = "live snapshot";
const OLD_TEXT: &str = "swap old snapshot";
const NEW_TEXT: &str = "swap new snapshot";

fn dependencies() -> Arc<dyn SnapshotDatabase> {
    snapshotstore_snapshotdatabase::new(
        snapshotstore_snapshotdatabase_snapshotcleanup::new(),
        snapshotstore_snapshotdatabase_snapshotfile::new(),
        snapshotstore_snapshotdatabase_snapshotrows::new(),
    )
}

fn subject() -> Arc<dyn SnapshotPeerFiles> {
    new(dependencies())
}

fn temp_root(test_name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is before UNIX_EPOCH")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "snapshotpeerfiles-{test_name}-{}-{stamp}",
        std::process::id()
    ));
    fs::remove_dir_all(&root).ok();
    fs::create_dir_all(&root).expect("create test root");
    root
}

fn peer(identity: &str, root: &Path) -> SnapshotPeerFilesConnectedPeer {
    SnapshotPeerFilesConnectedPeer {
        identity: identity.to_string(),
        winning_url: format!("file://{}", root.to_string_lossy()),
        scheme: SnapshotPeerFilesPeerScheme::File,
        handle: Arc::new(root.to_path_buf()) as Arc<dyn Any + Send + Sync>,
    }
}

fn live_path(root: &Path) -> PathBuf {
    root.join(".kitchensync/snapshot.db")
}

fn swap_new_path(root: &Path) -> PathBuf {
    root.join(".kitchensync/SWAP/snapshot.db/new")
}

fn swap_old_path(root: &Path) -> PathBuf {
    root.join(".kitchensync/SWAP/snapshot.db/old")
}

fn write_text(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().expect("test path has parent"))
        .expect("create parent directory");
    fs::write(path, text.as_bytes()).expect("write test file");
}

fn read_text(path: &Path) -> String {
    fs::read_to_string(path).expect("read test file")
}

fn start(
    peer_root: &Path,
    local_snapshot_directory: PathBuf,
) -> SnapshotPeerFilesStartupResult {
    subject().start_normal_peer_snapshot(SnapshotPeerFilesStartupRequest {
        peer: peer("peer-a", peer_root),
        local_snapshot_directory,
    })
}

#[test]
fn normal_startup_recovers_snapshot_swap_state_before_download() {
    let cases = [
        ("old-live-new", true, true, true, LIVE_TEXT),
        ("old-new", false, true, true, NEW_TEXT),
        ("old-only", false, true, false, OLD_TEXT),
        ("live-new", true, false, true, LIVE_TEXT),
        ("new-only", false, false, true, NEW_TEXT),
    ];

    for (name, has_live, has_old, has_new, expected_text) in cases {
        let root = temp_root(name);
        let peer_root = root.join("peer");
        let local_dir = root.join("tmp/peer-a");
        fs::create_dir_all(&local_dir).expect("create local snapshot directory");

        if has_live {
            write_text(&live_path(&peer_root), LIVE_TEXT);
        }
        if has_old {
            write_text(&swap_old_path(&peer_root), OLD_TEXT);
        }
        if has_new {
            write_text(&swap_new_path(&peer_root), NEW_TEXT);
        }

        let result = start(&peer_root, local_dir.clone());
        let local_snapshot_path = local_dir.join("snapshot.db");

        assert_eq!(
            result,
            SnapshotPeerFilesStartupResult::RecoveredAndDownloaded {
                peer_identity: "peer-a".to_string(),
                local_snapshot_path: local_snapshot_path.clone(),
            },
            "{name} should download recovered live snapshot"
        );
        assert_eq!(read_text(&live_path(&peer_root)), expected_text);
        assert_eq!(read_text(&local_snapshot_path), expected_text);
        assert!(!swap_old_path(&peer_root).exists(), "{name} leaves old");
        assert!(!swap_new_path(&peer_root).exists(), "{name} leaves new");
    }
}

#[test]
fn startup_downloads_only_live_snapshot_and_ignores_sqlite_sidecars() {
    let root = temp_root("download-live");
    let peer_root = root.join("peer");
    let local_dir = root.join("tmp/peer-a");
    fs::create_dir_all(&local_dir).expect("create local snapshot directory");

    write_text(&live_path(&peer_root), "downloaded bytes");
    write_text(&peer_root.join(".kitchensync/snapshot.db-wal"), "wal bytes");
    write_text(&peer_root.join(".kitchensync/snapshot.db-shm"), "shm bytes");

    let local_snapshot_path = local_dir.join("snapshot.db");
    assert_eq!(
        start(&peer_root, local_dir),
        SnapshotPeerFilesStartupResult::RecoveredAndDownloaded {
            peer_identity: "peer-a".to_string(),
            local_snapshot_path: local_snapshot_path.clone(),
        }
    );

    assert_eq!(read_text(&local_snapshot_path), "downloaded bytes");
    assert!(!local_snapshot_path.with_extension("db-wal").exists());
    assert!(!local_snapshot_path.with_extension("db-shm").exists());
}

#[test]
fn startup_without_live_snapshot_creates_new_empty_local_snapshot() {
    let root = temp_root("missing-live");
    let peer_root = root.join("peer");
    let local_dir = root.join("tmp/peer-a");
    fs::create_dir_all(&local_dir).expect("create local snapshot directory");
    write_text(
        &peer_root.join(".kitchensync/snapshot.db-wal"),
        "sidecar is not snapshot state",
    );

    let local_snapshot_path = local_dir.join("snapshot.db");
    assert_eq!(
        start(&peer_root, local_dir),
        SnapshotPeerFilesStartupResult::RecoveredWithNewEmptyLocalSnapshot {
            peer_identity: "peer-a".to_string(),
            local_snapshot_path: local_snapshot_path.clone(),
        }
    );

    let database = dependencies();
    let handle = database
        .open_existing(&local_snapshot_path)
        .expect("new local snapshot is a valid database");
    assert_eq!(
        database
            .list_child_rows(&handle, "any-parent")
            .expect("list rows in new snapshot"),
        Vec::new()
    );
    assert!(!local_snapshot_path.with_extension("db-wal").exists());
}

#[test]
fn normal_upload_replaces_live_snapshot_through_snapshot_swap_paths() {
    let root = temp_root("upload");
    let peer_root = root.join("peer");
    let live = live_path(&peer_root);
    let old = swap_old_path(&peer_root);
    let new = swap_new_path(&peer_root);
    write_text(&live, "old live bytes");

    let local_dir = root.join("tmp/peer-a");
    fs::create_dir_all(&local_dir).expect("create local snapshot directory");
    let first_local = local_dir.join("snapshot.db");
    write_text(&first_local, "first uploaded bytes");
    write_text(&local_dir.join("snapshot.db-wal"), "local wal bytes");

    subject()
        .upload_normal_peer_snapshot(SnapshotPeerFilesUploadRequest {
            peer: peer("peer-a", &peer_root),
            local_snapshot_path: first_local,
        })
        .expect("first upload succeeds");

    assert_eq!(read_text(&live), "first uploaded bytes");
    assert!(!old.exists());
    assert!(!new.exists());
    assert!(!peer_root.join(".kitchensync/snapshot.db-wal").exists());

    let second_local = local_dir.join("later-snapshot.db");
    write_text(&second_local, "second uploaded bytes");
    subject()
        .upload_normal_peer_snapshot(SnapshotPeerFilesUploadRequest {
            peer: peer("peer-a", &peer_root),
            local_snapshot_path: second_local,
        })
        .expect("second upload succeeds");

    assert_eq!(read_text(&live), "second uploaded bytes");
    assert!(!old.exists());
    assert!(!new.exists());
}
