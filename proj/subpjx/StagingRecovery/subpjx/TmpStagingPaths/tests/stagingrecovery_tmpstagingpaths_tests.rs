use std::any::Any;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use stagingrecovery_tmpstagingpaths::{
    TmpStagingPathFailure, TmpStagingPathPeer, TmpStagingPathPeerScheme,
    TmpStagingPathRequest, TmpStagingPaths,
};

fn subject() -> Arc<dyn TmpStagingPaths> {
    stagingrecovery_tmpstagingpaths::new()
}

fn temp_root(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is before UNIX_EPOCH")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "kitchensync-tmpstagingpaths-tests-{name}-{}-{stamp}",
        std::process::id()
    ));
    fs::remove_dir_all(&root).ok();
    fs::create_dir_all(&root).expect("create test root");
    root
}

fn file_peer(root: &Path) -> TmpStagingPathPeer {
    TmpStagingPathPeer {
        identity: "peer-a".to_owned(),
        scheme: TmpStagingPathPeerScheme::File,
        handle: Arc::new(root.to_path_buf()) as Arc<dyn Any + Send + Sync>,
    }
}

fn write_file(path: &Path, content: &str) {
    fs::create_dir_all(path.parent().expect("test path has parent"))
        .expect("create test file parent");
    fs::write(path, content).expect("write test file");
}

#[test]
fn prepares_transfer_tmp_directory_under_timestamp_directory() {
    let root = temp_root("success");
    write_file(&root.join("sync-root/folder/live.txt"), "live contents");

    let prepared = subject()
        .prepare_tmp_staging_path(TmpStagingPathRequest {
            peer: file_peer(&root),
            parent_path: "sync-root/folder".to_owned(),
            tmp_timestamp: "2026-07-02_12-00-00_000001Z".to_owned(),
            transfer_uuid: "11111111-2222-4333-8444-555555555555".to_owned(),
        })
        .expect("prepare TMP staging path");

    assert_eq!(prepared.peer_identity, "peer-a");
    assert_eq!(
        prepared.staging_path,
        "sync-root/folder/.kitchensync/TMP/2026-07-02_12-00-00_000001Z/11111111-2222-4333-8444-555555555555"
    );
    assert!(
        root.join("sync-root/folder/.kitchensync/TMP/2026-07-02_12-00-00_000001Z")
            .is_dir()
    );
    assert!(root.join(&prepared.staging_path).is_dir());
    assert_eq!(
        fs::read_to_string(root.join("sync-root/folder/live.txt"))
            .expect("read live user file"),
        "live contents"
    );
}

#[test]
fn returns_existing_transfer_tmp_directory_on_repeated_call() {
    let root = temp_root("repeat");
    let paths = subject();
    let request = TmpStagingPathRequest {
        peer: file_peer(&root),
        parent_path: "sync-root/folder".to_owned(),
        tmp_timestamp: "2026-07-02_12-00-30_000001Z".to_owned(),
        transfer_uuid: "66666666-7777-4888-8999-000000000000".to_owned(),
    };

    let first = paths
        .prepare_tmp_staging_path(request.clone())
        .expect("create TMP staging path");
    let second = paths
        .prepare_tmp_staging_path(request)
        .expect("return existing TMP staging path");

    assert_eq!(second, first);
    assert!(root.join(&second.staging_path).is_dir());
}

#[test]
fn reports_unusable_tmp_path_without_replacing_existing_paths() {
    let root = temp_root("conflict");
    let conflicting_tmp_path = root.join(
        "sync-root/folder/.kitchensync/TMP/2026-07-02_12-01-00_000002Z/aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee",
    );
    write_file(&conflicting_tmp_path, "existing tmp file");
    write_file(
        &root.join("sync-root/folder/aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee"),
        "live path with same transfer name",
    );

    let error = subject()
        .prepare_tmp_staging_path(TmpStagingPathRequest {
            peer: file_peer(&root),
            parent_path: "sync-root/folder".to_owned(),
            tmp_timestamp: "2026-07-02_12-01-00_000002Z".to_owned(),
            transfer_uuid: "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee".to_owned(),
        })
        .expect_err("conflicting TMP path is not a directory");

    assert_eq!(error.failure, TmpStagingPathFailure::TmpPathNotDirectory);
    assert_eq!(error.peer_identity, "peer-a");
    assert_eq!(error.parent_path, "sync-root/folder");
    assert_eq!(
        error.tmp_timestamp_directory,
        "sync-root/folder/.kitchensync/TMP/2026-07-02_12-01-00_000002Z"
    );
    assert_eq!(
        error.staging_path,
        "sync-root/folder/.kitchensync/TMP/2026-07-02_12-01-00_000002Z/aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee"
    );
    assert_eq!(
        fs::read_to_string(conflicting_tmp_path).expect("read conflicting TMP file"),
        "existing tmp file"
    );
    assert_eq!(
        fs::read_to_string(
            root.join("sync-root/folder/aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee")
        )
        .expect("read live user path"),
        "live path with same transfer name"
    );
}
