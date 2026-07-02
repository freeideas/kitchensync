use std::any::Any;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use stagingrecovery_swaprecovery::{
    SwapRecovery, SwapRecoveryFailureKind, SwapRecoveryPeer, SwapRecoveryPeerScheme,
    SwapRecoveryRequest, SwapRecoveryResult,
};

fn subject() -> Arc<dyn SwapRecovery> {
    stagingrecovery_swaprecovery::new()
}

fn file_peer(identity: &str, root: &Path) -> SwapRecoveryPeer {
    SwapRecoveryPeer {
        identity: identity.to_string(),
        scheme: SwapRecoveryPeerScheme::File,
        handle: Arc::new(root.to_path_buf()) as Arc<dyn Any + Send + Sync>,
    }
}

fn recover(
    recovery: &dyn SwapRecovery,
    root: &Path,
    parent_path: &str,
    bak_timestamp: &str,
) -> SwapRecoveryResult {
    recovery.recover_swap(SwapRecoveryRequest {
        peer: file_peer("peer-a", root),
        parent_path: parent_path.to_string(),
        bak_timestamp: bak_timestamp.to_string(),
    })
}

#[test]
fn recovers_each_direct_user_data_swap_child_case() {
    let root = fresh_test_dir("recovers_each_direct_user_data_swap_child_case");
    let recovery = subject();
    let timestamp = "2026-07-02_12-00-00_000000Z";

    write_file(&root, "folder/target-kept.txt", "live target");
    write_file(
        &root,
        "folder/.kitchensync/SWAP/target-kept.txt/old",
        "old archived",
    );

    write_file(
        &root,
        "folder/.kitchensync/SWAP/new-replaces-missing.txt/new",
        "new target",
    );
    write_file(
        &root,
        "folder/.kitchensync/SWAP/new-replaces-missing.txt/old",
        "old archived after new",
    );

    write_file(
        &root,
        "folder/.kitchensync/SWAP/old-restored.txt/old",
        "old target",
    );

    write_file(&root, "folder/new-discarded.txt", "live target");
    write_file(
        &root,
        "folder/.kitchensync/SWAP/new-discarded.txt/new",
        "discarded new",
    );

    write_file(
        &root,
        "folder/.kitchensync/SWAP/new-promoted.txt/new",
        "promoted new",
    );

    write_file(&root, "folder/name with space.txt", "live spaced target");
    write_file(
        &root,
        "folder/.kitchensync/SWAP/name%20with%20space.txt/old",
        "old spaced content",
    );

    let result = recover(&*recovery, &root, "folder", timestamp);

    assert_eq!(result, SwapRecoveryResult::Recovered);
    assert_eq!(read_file(&root, "folder/target-kept.txt"), "live target");
    assert_eq!(
        read_file(
            &root,
            "folder/.kitchensync/BAK/2026-07-02_12-00-00_000000Z/target-kept.txt",
        ),
        "old archived"
    );
    assert_eq!(
        read_file(&root, "folder/new-replaces-missing.txt"),
        "new target"
    );
    assert_eq!(
        read_file(
            &root,
            "folder/.kitchensync/BAK/2026-07-02_12-00-00_000000Z/new-replaces-missing.txt",
        ),
        "old archived after new"
    );
    assert_eq!(read_file(&root, "folder/old-restored.txt"), "old target");
    assert_eq!(read_file(&root, "folder/new-discarded.txt"), "live target");
    assert_eq!(read_file(&root, "folder/new-promoted.txt"), "promoted new");
    assert_eq!(
        read_file(&root, "folder/name with space.txt"),
        "live spaced target"
    );
    assert_eq!(
        read_file(
            &root,
            "folder/.kitchensync/BAK/2026-07-02_12-00-00_000000Z/name with space.txt",
        ),
        "old spaced content"
    );

    assert_missing(&root, "folder/.kitchensync/SWAP/target-kept.txt");
    assert_missing(&root, "folder/.kitchensync/SWAP/new-replaces-missing.txt");
    assert_missing(&root, "folder/.kitchensync/SWAP/old-restored.txt");
    assert_missing(&root, "folder/.kitchensync/SWAP/new-discarded.txt");
    assert_missing(&root, "folder/.kitchensync/SWAP/new-promoted.txt");
    assert_missing(&root, "folder/.kitchensync/SWAP/name%20with%20space.txt");

    let second_result = recover(&*recovery, &root, "folder", timestamp);
    assert_eq!(second_result, SwapRecoveryResult::Recovered);

    remove_test_dir(&root);
}

#[test]
fn missing_swap_directory_succeeds_without_changing_user_data() {
    let root = fresh_test_dir("missing_swap_directory_succeeds_without_changing_user_data");
    let recovery = subject();
    write_file(&root, "folder/live.txt", "unchanged");

    let result = recover(
        &*recovery,
        &root,
        "folder",
        "2026-07-02_12-00-00_000000Z",
    );

    assert_eq!(result, SwapRecoveryResult::Recovered);
    assert_eq!(read_file(&root, "folder/live.txt"), "unchanged");
    assert_missing(&root, "folder/.kitchensync");

    remove_test_dir(&root);
}

#[test]
fn failure_reports_failed_listing_and_leaves_swap_state_for_later_recovery() {
    let root =
        fresh_test_dir("failure_reports_failed_listing_and_leaves_swap_state_for_later_recovery");
    let recovery = subject();

    write_file(&root, "folder/.kitchensync/SWAP/bad%xx/new", "unrecovered new");

    let result = recover(
        &*recovery,
        &root,
        "folder",
        "2026-07-02_12-00-00_000000Z",
    );

    let failure = match result {
        SwapRecoveryResult::FailedListing(failure) => failure,
        SwapRecoveryResult::Recovered => panic!("invalid SWAP basename must fail listing"),
    };

    assert_eq!(failure.kind, SwapRecoveryFailureKind::SwapBasenameDecodeFailed);
    assert_eq!(failure.peer_identity, "peer-a");
    assert_eq!(failure.parent_path, "folder");
    assert_eq!(failure.failed_path, Some("folder/.kitchensync/SWAP/bad%xx".to_string()));
    assert_eq!(
        read_file(&root, "folder/.kitchensync/SWAP/bad%xx/new"),
        "unrecovered new"
    );
    assert!(root.join("folder/.kitchensync/SWAP/bad%xx").is_dir());

    remove_test_dir(&root);
}

fn fresh_test_dir(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "kitchensync-swaprecovery-{}-{}",
        std::process::id(),
        name
    ));
    remove_test_dir(&root);
    fs::create_dir_all(&root).expect("test directory should be created");
    root
}

fn remove_test_dir(path: &Path) {
    match fs::remove_dir_all(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("failed to remove test directory {}: {}", path.display(), error),
    }
}

fn write_file(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("test file parent should be created");
    }
    fs::write(path, contents).expect("test file should be written");
}

fn read_file(root: &Path, relative: &str) -> String {
    fs::read_to_string(root.join(relative)).expect("test file should be readable")
}

fn assert_missing(root: &Path, relative: &str) {
    assert!(
        !root.join(relative).exists(),
        "{} should not exist",
        root.join(relative).display()
    );
}
