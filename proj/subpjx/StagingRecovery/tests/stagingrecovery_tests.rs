use std::any::Any;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use stagingrecovery::{
    BakDisplacementRequest, StagingCleanupRequest, StagingRecovery, StagingRecoveryFailureKind,
    StagingRecoveryOperation, StagingRecoveryPeerHandle, StagingRecoveryPeerScheme,
    SwapRecoveryRequest, SwapRecoveryResult, TmpStagingPathRequest, UserDataSwapRecoveryRequest,
};

struct TestRoot {
    path: PathBuf,
}

impl TestRoot {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "kitchensync-stagingrecovery-{}-{}",
            std::process::id(),
            name
        ));
        remove_test_dir(&path);
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
        identity: identity.to_owned(),
        winning_url: format!("file://{}", root.display()),
        scheme: StagingRecoveryPeerScheme::File,
        handle: Arc::new(root.to_path_buf()) as Arc<dyn Any + Send + Sync>,
    }
}

#[test]
fn directory_swap_recovery_repairs_each_direct_user_data_child() {
    let root = TestRoot::new("directory-swap-recovery");
    let recovery = subject();
    let peer = file_peer("peer-a", root.path());
    let timestamp = "2026-07-02_12-00-00_000000Z";

    write_file(root.path(), "folder/target-kept.txt", "live target");
    write_file(
        root.path(),
        "folder/.kitchensync/SWAP/target-kept.txt/old",
        "old archived",
    );
    write_file(
        root.path(),
        "folder/.kitchensync/SWAP/new-replaces-missing.txt/new",
        "new target",
    );
    write_file(
        root.path(),
        "folder/.kitchensync/SWAP/new-replaces-missing.txt/old",
        "old archived after new",
    );
    write_file(
        root.path(),
        "folder/.kitchensync/SWAP/old-restored.txt/old",
        "old target",
    );
    write_file(root.path(), "folder/new-discarded.txt", "live target");
    write_file(
        root.path(),
        "folder/.kitchensync/SWAP/new-discarded.txt/new",
        "discarded new",
    );
    write_file(
        root.path(),
        "folder/.kitchensync/SWAP/new-promoted.txt/new",
        "promoted new",
    );
    write_file(root.path(), "folder/name with space.txt", "live spaced target");
    write_file(
        root.path(),
        "folder/.kitchensync/SWAP/name%20with%20space.txt/old",
        "old spaced content",
    );
    write_file(root.path(), "folder/all-three.txt", "live wins");
    write_file(
        root.path(),
        "folder/.kitchensync/SWAP/all-three.txt/old",
        "old archived from all three",
    );
    write_file(
        root.path(),
        "folder/.kitchensync/SWAP/all-three.txt/new",
        "new deleted from all three",
    );

    let result = recovery.recover_swap(SwapRecoveryRequest {
        peer,
        parent_relative_path: "folder".to_owned(),
        bak_timestamp: timestamp.to_owned(),
    });

    assert_eq!(result, SwapRecoveryResult::Recovered);
    assert_eq!(read_file(root.path(), "folder/target-kept.txt"), "live target");
    assert_eq!(
        read_file(
            root.path(),
            "folder/.kitchensync/BAK/2026-07-02_12-00-00_000000Z/target-kept.txt",
        ),
        "old archived"
    );
    assert_eq!(
        read_file(root.path(), "folder/new-replaces-missing.txt"),
        "new target"
    );
    assert_eq!(
        read_file(
            root.path(),
            "folder/.kitchensync/BAK/2026-07-02_12-00-00_000000Z/new-replaces-missing.txt",
        ),
        "old archived after new"
    );
    assert_eq!(read_file(root.path(), "folder/old-restored.txt"), "old target");
    assert_eq!(read_file(root.path(), "folder/new-discarded.txt"), "live target");
    assert_eq!(read_file(root.path(), "folder/new-promoted.txt"), "promoted new");
    assert_eq!(
        read_file(root.path(), "folder/name with space.txt"),
        "live spaced target"
    );
    assert_eq!(read_file(root.path(), "folder/all-three.txt"), "live wins");
    assert_eq!(
        read_file(
            root.path(),
            "folder/.kitchensync/BAK/2026-07-02_12-00-00_000000Z/all-three.txt",
        ),
        "old archived from all three"
    );

    assert_missing(root.path(), "folder/.kitchensync/SWAP/target-kept.txt");
    assert_missing(
        root.path(),
        "folder/.kitchensync/SWAP/new-replaces-missing.txt",
    );
    assert_missing(root.path(), "folder/.kitchensync/SWAP/old-restored.txt");
    assert_missing(root.path(), "folder/.kitchensync/SWAP/new-discarded.txt");
    assert_missing(root.path(), "folder/.kitchensync/SWAP/new-promoted.txt");
    assert_missing(
        root.path(),
        "folder/.kitchensync/SWAP/name%20with%20space.txt",
    );
    assert_missing(root.path(), "folder/.kitchensync/SWAP/all-three.txt");
}

#[test]
fn user_data_swap_recovery_handles_only_the_requested_encoded_basename() {
    let root = TestRoot::new("user-data-swap-recovery");
    let recovery = subject();
    let peer = file_peer("peer-a", root.path());

    write_file(root.path(), "folder/name with space.txt", "live target");
    write_file(
        root.path(),
        "folder/.kitchensync/SWAP/name%20with%20space.txt/old",
        "old archived",
    );
    write_file(
        root.path(),
        "folder/.kitchensync/SWAP/name%20with%20space.txt/new",
        "new discarded",
    );
    write_file(
        root.path(),
        "folder/.kitchensync/SWAP/sibling.txt/new",
        "sibling untouched",
    );

    recovery
        .recover_user_data_swap(UserDataSwapRecoveryRequest {
            peer,
            parent_relative_path: "folder".to_owned(),
            basename: "name with space.txt".to_owned(),
            encoded_basename: "name%20with%20space.txt".to_owned(),
            bak_timestamp: "2026-07-02_12-10-00_000000Z".to_owned(),
        })
        .expect("requested SWAP child should recover");

    assert_eq!(
        read_file(root.path(), "folder/name with space.txt"),
        "live target"
    );
    assert_eq!(
        read_file(
            root.path(),
            "folder/.kitchensync/BAK/2026-07-02_12-10-00_000000Z/name with space.txt",
        ),
        "old archived"
    );
    assert_missing(
        root.path(),
        "folder/.kitchensync/SWAP/name%20with%20space.txt",
    );
    assert_eq!(
        read_file(root.path(), "folder/.kitchensync/SWAP/sibling.txt/new"),
        "sibling untouched"
    );
}

#[test]
fn swap_recovery_failure_reports_failed_listing_and_keeps_swap_state() {
    let root = TestRoot::new("swap-recovery-failure");
    let recovery = subject();
    let peer = file_peer("peer-a", root.path());

    write_file(
        root.path(),
        "folder/.kitchensync/SWAP/bad%xx/new",
        "unrecovered new",
    );

    let result = recovery.recover_swap(SwapRecoveryRequest {
        peer,
        parent_relative_path: "folder".to_owned(),
        bak_timestamp: "2026-07-02_12-00-00_000000Z".to_owned(),
    });

    let failure = match result {
        SwapRecoveryResult::FailedListing(failure) => failure,
        SwapRecoveryResult::Recovered => panic!("invalid SWAP child must fail listing"),
    };

    assert_eq!(failure.peer_identity, "peer-a");
    assert_eq!(failure.parent_relative_path, "folder");
    assert_eq!(failure.operation, StagingRecoveryOperation::SwapRecovery);
    assert_eq!(
        failure.kind,
        StagingRecoveryFailureKind::SwapBasenameDecodeFailed
    );
    assert_eq!(
        read_file(root.path(), "folder/.kitchensync/SWAP/bad%xx/new"),
        "unrecovered new"
    );
    assert!(root.path().join("folder/.kitchensync/SWAP/bad%xx").is_dir());
}

#[test]
fn displacement_and_tmp_paths_use_the_entry_parent_metadata_tree() {
    let root = TestRoot::new("displacement-and-tmp");
    let recovery = subject();
    let peer = file_peer("peer-a", root.path());

    write_file(root.path(), "nested/parent/tree/child.txt", "preserved subtree");
    write_file(root.path(), "nested/parent/transfer-uuid", "live user data");

    let displacement = recovery
        .displace_to_bak(BakDisplacementRequest {
            peer: peer.clone(),
            parent_relative_path: "nested/parent".to_owned(),
            basename: "tree".to_owned(),
            bak_timestamp: "2026-07-02_12-20-00_000000Z".to_owned(),
        })
        .expect("existing directory should displace to BAK");

    assert_eq!(displacement.peer_identity, "peer-a");
    assert_eq!(displacement.original_relative_path, "nested/parent/tree");
    assert_eq!(
        displacement.bak_relative_path,
        "nested/parent/.kitchensync/BAK/2026-07-02_12-20-00_000000Z/tree"
    );
    assert_missing(root.path(), "nested/parent/tree");
    assert_eq!(
        read_file(
            root.path(),
            "nested/parent/.kitchensync/BAK/2026-07-02_12-20-00_000000Z/tree/child.txt",
        ),
        "preserved subtree"
    );
    assert_missing(
        root.path(),
        ".kitchensync/BAK/2026-07-02_12-20-00_000000Z/tree",
    );

    let tmp = recovery
        .prepare_tmp_staging_path(TmpStagingPathRequest {
            peer,
            parent_relative_path: "nested/parent".to_owned(),
            tmp_timestamp: "2026-07-02_12-21-00_000000Z".to_owned(),
            transfer_uuid: "transfer-uuid".to_owned(),
        })
        .expect("TMP staging path should be prepared below metadata");

    assert_eq!(tmp.peer_identity, "peer-a");
    assert_eq!(
        tmp.staging_relative_path,
        "nested/parent/.kitchensync/TMP/2026-07-02_12-21-00_000000Z/transfer-uuid"
    );
    assert!(
        root.path()
            .join("nested/parent/.kitchensync/TMP/2026-07-02_12-21-00_000000Z/transfer-uuid")
            .is_dir()
    );
    assert_eq!(
        read_file(root.path(), "nested/parent/transfer-uuid"),
        "live user data"
    );
}

#[test]
fn cleanup_removes_only_expired_bak_and_tmp_timestamp_directories() {
    let root = TestRoot::new("cleanup");
    let recovery = subject();
    let peer = file_peer("peer-a", root.path());

    write_file(
        root.path(),
        "folder/.kitchensync/BAK/1970-01-01_00-00-00_000000Z/old.txt",
        "old bak",
    );
    write_file(
        root.path(),
        "folder/.kitchensync/BAK/1970-01-08_00-00-00_000000Z/recent.txt",
        "recent bak",
    );
    write_file(
        root.path(),
        "folder/.kitchensync/TMP/1970-01-01_00-00-00_000000Z/old/tmp.txt",
        "old tmp",
    );
    write_file(
        root.path(),
        "folder/.kitchensync/TMP/1970-01-08_00-00-00_000000Z/recent/tmp.txt",
        "recent tmp",
    );
    write_file(
        root.path(),
        "folder/.kitchensync/SWAP/1970-01-01_00-00-00_000000Z/new",
        "swap remains",
    );

    let result = recovery
        .cleanup_staging(StagingCleanupRequest {
            peer,
            parent_relative_path: "folder".to_owned(),
            current_time: UNIX_EPOCH + Duration::from_secs(9 * 86_400),
            keep_bak_days: 5,
            keep_tmp_days: 5,
        })
        .expect("cleanup should succeed");

    assert_eq!(result.peer_identity, "peer-a");
    assert_eq!(result.parent_relative_path, "folder");
    assert_missing(
        root.path(),
        "folder/.kitchensync/BAK/1970-01-01_00-00-00_000000Z",
    );
    assert!(
        root.path()
            .join("folder/.kitchensync/BAK/1970-01-08_00-00-00_000000Z")
            .is_dir()
    );
    assert_missing(
        root.path(),
        "folder/.kitchensync/TMP/1970-01-01_00-00-00_000000Z",
    );
    assert!(
        root.path()
            .join("folder/.kitchensync/TMP/1970-01-08_00-00-00_000000Z")
            .is_dir()
    );
    assert_eq!(
        read_file(
            root.path(),
            "folder/.kitchensync/SWAP/1970-01-01_00-00-00_000000Z/new",
        ),
        "swap remains"
    );
}

fn write_file(root: &Path, relative_path: &str, content: &str) {
    let path = root.join(relative_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create test file parent");
    }
    fs::write(path, content).expect("write test file");
}

fn read_file(root: &Path, relative_path: &str) -> String {
    fs::read_to_string(root.join(relative_path)).expect("read test file")
}

fn assert_missing(root: &Path, relative_path: &str) {
    assert!(
        !root.join(relative_path).exists(),
        "{} should not exist",
        root.join(relative_path).display()
    );
}

fn remove_test_dir(path: &Path) {
    match fs::remove_dir_all(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("failed to remove test directory {}: {}", path.display(), error),
    }
}
