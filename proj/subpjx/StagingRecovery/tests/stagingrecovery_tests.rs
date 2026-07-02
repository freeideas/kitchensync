use std::any::Any;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use stagingrecovery::{
    BakDisplacementRequest, StagingCleanupRequest, StagingRecovery, StagingRecoveryFailureKind,
    StagingRecoveryOperation, StagingRecoveryPeerHandle, StagingRecoveryPeerScheme,
    SwapRecoveryRequest, SwapRecoveryResult, TmpStagingPathRequest,
};

fn subject() -> Arc<dyn StagingRecovery> {
    let transport = transportoperations::new(
        transportoperations_localtransportoperations::new(),
        transportoperations_sftptransportoperations::new(),
    );

    stagingrecovery::new(
        transport,
        stagingrecovery_bakdisplacement::new(),
        stagingrecovery_stagingcleanup::new(),
        stagingrecovery_swaprecovery::new(),
        stagingrecovery_tmpstagingpaths::new(),
    )
}

fn file_peer(identity: &str, root: &Path) -> StagingRecoveryPeerHandle {
    StagingRecoveryPeerHandle {
        identity: identity.to_string(),
        winning_url: format!("file://{}", root.to_string_lossy()),
        scheme: StagingRecoveryPeerScheme::File,
        handle: Arc::new(root.to_path_buf()) as Arc<dyn Any + Send + Sync>,
    }
}

#[test]
fn recover_swap_repairs_direct_children_before_live_listing() {
    let root = fresh_test_dir("recover_swap_repairs_direct_children_before_live_listing");
    let staging_recovery = subject();
    let peer = file_peer("peer-a", &root);
    let timestamp = "2026-07-02_12-00-00_000000Z";

    write_file(&root, "folder/kept-target.txt", "live target");
    write_file(
        &root,
        "folder/.kitchensync/SWAP/kept-target.txt/old",
        "archived old",
    );
    write_file(
        &root,
        "folder/.kitchensync/SWAP/new-wins.txt/new",
        "new content",
    );
    write_file(
        &root,
        "folder/.kitchensync/SWAP/old-restores.txt/old",
        "restored old",
    );
    write_file(
        &root,
        "folder/.kitchensync/SWAP/new-and-old.txt/new",
        "installed new",
    );
    write_file(
        &root,
        "folder/.kitchensync/SWAP/new-and-old.txt/old",
        "archived old",
    );
    write_file(&root, "folder/new-conflict.txt", "live target");
    write_file(
        &root,
        "folder/.kitchensync/SWAP/new-conflict.txt/new",
        "discarded new",
    );

    let result = staging_recovery.recover_swap(SwapRecoveryRequest {
        peer,
        parent_relative_path: "folder".to_string(),
        bak_timestamp: timestamp.to_string(),
    });

    assert_eq!(result, SwapRecoveryResult::Recovered);
    assert_eq!(read_file(&root, "folder/kept-target.txt"), "live target");
    assert_eq!(
        read_file(
            &root,
            "folder/.kitchensync/BAK/2026-07-02_12-00-00_000000Z/kept-target.txt",
        ),
        "archived old"
    );
    assert_eq!(read_file(&root, "folder/new-wins.txt"), "new content");
    assert_eq!(read_file(&root, "folder/old-restores.txt"), "restored old");
    assert_eq!(read_file(&root, "folder/new-and-old.txt"), "installed new");
    assert_eq!(
        read_file(
            &root,
            "folder/.kitchensync/BAK/2026-07-02_12-00-00_000000Z/new-and-old.txt",
        ),
        "archived old"
    );
    assert_eq!(read_file(&root, "folder/new-conflict.txt"), "live target");
    assert!(!root.join("folder/.kitchensync/SWAP/kept-target.txt").exists());
    assert!(!root.join("folder/.kitchensync/SWAP/new-wins.txt").exists());
    assert!(!root.join("folder/.kitchensync/SWAP/old-restores.txt").exists());
    assert!(!root.join("folder/.kitchensync/SWAP/new-and-old.txt").exists());
    assert!(!root.join("folder/.kitchensync/SWAP/new-conflict.txt").exists());

    remove_test_dir(&root);
}

#[test]
fn recover_swap_failure_reports_failed_listing_and_keeps_swap_state() {
    let root = fresh_test_dir("recover_swap_failure_reports_failed_listing_and_keeps_swap_state");
    let staging_recovery = subject();
    let peer = file_peer("peer-a", &root);

    write_file(&root, "folder/.kitchensync/SWAP/bad%xx/new", "not recovered");

    let result = staging_recovery.recover_swap(SwapRecoveryRequest {
        peer,
        parent_relative_path: "folder".to_string(),
        bak_timestamp: "2026-07-02_12-00-00_000000Z".to_string(),
    });

    let failure = match result {
        SwapRecoveryResult::FailedListing(failure) => failure,
        SwapRecoveryResult::Recovered => panic!("invalid SWAP child must fail the directory listing"),
    };

    assert_eq!(failure.peer_identity, "peer-a");
    assert_eq!(failure.parent_relative_path, "folder");
    assert_eq!(failure.operation, StagingRecoveryOperation::SwapRecovery);
    assert_eq!(
        failure.kind,
        StagingRecoveryFailureKind::SwapBasenameDecodeFailed
    );
    assert_eq!(
        read_file(&root, "folder/.kitchensync/SWAP/bad%xx/new"),
        "not recovered"
    );

    remove_test_dir(&root);
}

#[test]
fn displace_to_bak_and_prepare_tmp_use_nearby_metadata_paths() {
    let root = fresh_test_dir("displace_to_bak_and_prepare_tmp_use_nearby_metadata_paths");
    let staging_recovery = subject();
    let peer = file_peer("peer-a", &root);

    write_file(&root, "nested/parent/tree/child.txt", "preserved subtree");
    write_file(&root, "nested/parent/transfer-uuid", "live user data");

    let displacement = staging_recovery
        .displace_to_bak(BakDisplacementRequest {
            peer: peer.clone(),
            parent_relative_path: "nested/parent".to_string(),
            basename: "tree".to_string(),
            bak_timestamp: "2026-07-02_12-00-00_000000Z".to_string(),
        })
        .expect("displacing an existing directory should succeed");

    assert_eq!(displacement.peer_identity, "peer-a");
    assert_eq!(displacement.original_relative_path, "nested/parent/tree");
    assert_eq!(
        displacement.bak_relative_path,
        "nested/parent/.kitchensync/BAK/2026-07-02_12-00-00_000000Z/tree"
    );
    assert!(!root.join("nested/parent/tree").exists());
    assert_eq!(
        read_file(
            &root,
            "nested/parent/.kitchensync/BAK/2026-07-02_12-00-00_000000Z/tree/child.txt",
        ),
        "preserved subtree"
    );
    assert!(
        !root
            .join(".kitchensync/BAK/2026-07-02_12-00-00_000000Z/tree")
            .exists(),
        "BAK must be under the displaced entry's parent, not the sync root"
    );

    let tmp = staging_recovery
        .prepare_tmp_staging_path(TmpStagingPathRequest {
            peer,
            parent_relative_path: "nested/parent".to_string(),
            tmp_timestamp: "2026-07-02_12-01-00_000000Z".to_string(),
            transfer_uuid: "transfer-uuid".to_string(),
        })
        .expect("TMP staging path should be prepared below metadata");

    assert_eq!(tmp.peer_identity, "peer-a");
    assert_eq!(
        tmp.staging_relative_path,
        "nested/parent/.kitchensync/TMP/2026-07-02_12-01-00_000000Z/transfer-uuid"
    );
    assert!(
        root.join("nested/parent/.kitchensync/TMP/2026-07-02_12-01-00_000000Z/transfer-uuid")
            .is_dir()
    );
    assert_eq!(
        read_file(&root, "nested/parent/transfer-uuid"),
        "live user data"
    );

    remove_test_dir(&root);
}

#[test]
fn cleanup_removes_only_expired_bak_and_tmp_timestamp_directories() {
    let root = fresh_test_dir("cleanup_removes_only_expired_bak_and_tmp_timestamp_directories");
    let staging_recovery = subject();
    let peer = file_peer("peer-a", &root);

    write_file(
        &root,
        "folder/.kitchensync/BAK/1970-01-01_00-00-00_000000Z/old.txt",
        "old bak",
    );
    write_file(
        &root,
        "folder/.kitchensync/BAK/1970-01-08_00-00-00_000000Z/recent.txt",
        "recent bak",
    );
    write_file(
        &root,
        "folder/.kitchensync/TMP/1970-01-01_00-00-00_000000Z/old/tmp.txt",
        "old tmp",
    );
    write_file(
        &root,
        "folder/.kitchensync/TMP/1970-01-08_00-00-00_000000Z/recent/tmp.txt",
        "recent tmp",
    );
    write_file(
        &root,
        "folder/.kitchensync/SWAP/1970-01-01_00-00-00_000000Z/new",
        "swap remains",
    );

    let result = staging_recovery
        .cleanup_staging(StagingCleanupRequest {
            peer,
            parent_relative_path: "folder".to_string(),
            current_time: UNIX_EPOCH + Duration::from_secs(9 * 86_400),
            keep_bak_days: 5,
            keep_tmp_days: 5,
        })
        .expect("cleanup should succeed");

    assert_eq!(result.peer_identity, "peer-a");
    assert_eq!(result.parent_relative_path, "folder");
    assert!(!root
        .join("folder/.kitchensync/BAK/1970-01-01_00-00-00_000000Z")
        .exists());
    assert!(root
        .join("folder/.kitchensync/BAK/1970-01-08_00-00-00_000000Z")
        .is_dir());
    assert!(!root
        .join("folder/.kitchensync/TMP/1970-01-01_00-00-00_000000Z")
        .exists());
    assert!(root
        .join("folder/.kitchensync/TMP/1970-01-08_00-00-00_000000Z")
        .is_dir());
    assert_eq!(
        read_file(
            &root,
            "folder/.kitchensync/SWAP/1970-01-01_00-00-00_000000Z/new",
        ),
        "swap remains"
    );

    remove_test_dir(&root);
}

fn fresh_test_dir(test_name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "kitchensync-stagingrecovery-{}-{}",
        std::process::id(),
        test_name
    ));
    remove_test_dir(&path);
    fs::create_dir_all(&path).expect("test setup should create a fresh temporary directory");
    path
}

fn write_file(root: &Path, relative_path: &str, content: &str) {
    let path = root.join(relative_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("test setup should create parent directories");
    }
    fs::write(path, content).expect("test setup should write file content");
}

fn read_file(root: &Path, relative_path: &str) -> String {
    fs::read_to_string(root.join(relative_path)).expect("test should read file content")
}

fn remove_test_dir(path: &Path) {
    match fs::remove_dir_all(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("failed to remove temporary test directory {path:?}: {error}"),
    }
}
