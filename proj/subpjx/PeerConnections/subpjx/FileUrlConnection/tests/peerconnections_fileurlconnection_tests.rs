use std::fs;
use std::path::{Path, PathBuf};

use peerconnections_fileurlconnection::{
    new, FileUrlConnectionFailureReason, FileUrlConnectionRequest, FileUrlConnectionRunMode,
};

#[test]
fn normal_run_creates_missing_peer_root_and_parents() {
    let subject = new();
    let test_dir = fresh_test_dir("normal_run_creates_missing_peer_root_and_parents");
    let peer_root = test_dir.join("missing").join("parent").join("peer-root");

    let handle = subject
        .establish_file_url(request(
            peer_root.clone(),
            FileUrlConnectionRunMode::Normal,
            19,
            23,
        ))
        .expect("normal run should create a missing peer root");

    assert_eq!(handle.local_peer_root_path, peer_root);
    assert!(
        peer_root.is_dir(),
        "normal-run success must leave the peer root as a directory"
    );

    remove_test_dir(&test_dir);
}

#[test]
fn timeout_and_idle_settings_do_not_change_file_url_establishment() {
    let subject = new();
    let test_dir =
        fresh_test_dir("timeout_and_idle_settings_do_not_change_file_url_establishment");
    let without_settings = test_dir.join("without-settings");
    let with_settings = test_dir.join("with-settings");

    let first = subject.establish_file_url(request(
        without_settings.clone(),
        FileUrlConnectionRunMode::Normal,
        0,
        0,
    ));
    let second = subject.establish_file_url(request(
        with_settings.clone(),
        FileUrlConnectionRunMode::Normal,
        u32::MAX,
        u32::MAX,
    ));

    assert_eq!(first.unwrap().local_peer_root_path, without_settings);
    assert_eq!(second.unwrap().local_peer_root_path, with_settings);
    assert!(without_settings.is_dir());
    assert!(with_settings.is_dir());

    remove_test_dir(&test_dir);
}

#[test]
fn normal_run_reports_failed_url_when_parent_blocks_directory_creation() {
    let subject = new();
    let test_dir =
        fresh_test_dir("normal_run_reports_failed_url_when_parent_blocks_directory_creation");
    let blocking_file = test_dir.join("not-a-directory");
    fs::write(&blocking_file, b"blocks peer root parent")
        .expect("test setup should create the blocking file");
    let peer_root = blocking_file.join("peer-root");

    let failure = subject
        .establish_file_url(request(
            peer_root.clone(),
            FileUrlConnectionRunMode::Normal,
            11,
            13,
        ))
        .expect_err("normal run should fail when directory creation is blocked");

    assert_eq!(failure.local_peer_root_path, peer_root);
    assert_eq!(
        failure.reason,
        FileUrlConnectionFailureReason::DirectoryCreationFailed
    );
    assert!(
        !failure.detail.is_empty(),
        "URL failure should preserve implementation detail for reporting"
    );
    assert!(
        !blocking_file.join("peer-root").is_dir(),
        "failed creation must not report success for a missing peer root"
    );

    remove_test_dir(&test_dir);
}

fn request(
    local_peer_root_path: PathBuf,
    run_mode: FileUrlConnectionRunMode,
    timeout_conn_seconds: u32,
    timeout_idle_seconds: u32,
) -> FileUrlConnectionRequest {
    FileUrlConnectionRequest {
        local_peer_root_path,
        run_mode,
        timeout_conn_seconds,
        timeout_idle_seconds,
    }
}

fn fresh_test_dir(test_name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "kitchensync-fileurlconnection-{}-{}",
        std::process::id(),
        test_name
    ));
    remove_test_dir(&path);
    fs::create_dir_all(&path).expect("test setup should create a fresh temporary directory");
    path
}

fn remove_test_dir(path: &Path) {
    match fs::remove_dir_all(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("failed to remove temporary test directory {path:?}: {error}"),
    }
}
