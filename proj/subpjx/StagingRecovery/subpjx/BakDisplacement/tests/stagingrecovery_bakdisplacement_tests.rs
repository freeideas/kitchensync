use std::any::Any;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use stagingrecovery_bakdisplacement::{
    BakDisplacement, BakDisplacementPeer, BakDisplacementPeerScheme, BakDisplacementRequest,
};

fn subject() -> Arc<dyn BakDisplacement> {
    stagingrecovery_bakdisplacement::new()
}

fn temp_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "kitchensync-bakdisplacement-tests-{}-{}",
        std::process::id(),
        name
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("create test root");
    root
}

fn write_file(root: &Path, relative_path: &str, content: &str) {
    let path = root.join(relative_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create file parent");
    }
    fs::write(path, content).expect("write test file");
}

fn file_peer(root: &Path) -> BakDisplacementPeer {
    BakDisplacementPeer {
        identity: "peer-a".to_owned(),
        scheme: BakDisplacementPeerScheme::File,
        handle: Arc::new(root.to_path_buf()) as Arc<dyn Any + Send + Sync>,
    }
}

#[test]
fn displaces_file_to_bak_directory_under_the_entry_parent() {
    let root = temp_root("file");
    write_file(&root, "sync-root/folder/report.txt", "original contents");

    let displacement = subject();
    let record = displacement
        .displace_to_bak(BakDisplacementRequest {
            peer: file_peer(&root),
            parent_path: "sync-root/folder".to_owned(),
            basename: "report.txt".to_owned(),
            bak_timestamp: "2026-07-02_10-30-00_000001Z".to_owned(),
        })
        .expect("displace file to BAK");

    assert_eq!(record.peer_identity, "peer-a");
    assert_eq!(record.original_path, "sync-root/folder/report.txt");
    assert_eq!(
        record.bak_timestamp_directory,
        "sync-root/folder/.kitchensync/BAK/2026-07-02_10-30-00_000001Z"
    );
    assert_eq!(
        record.bak_destination_path,
        "sync-root/folder/.kitchensync/BAK/2026-07-02_10-30-00_000001Z/report.txt"
    );

    assert!(!root.join("sync-root/folder/report.txt").exists());
    assert!(
        root.join("sync-root/folder/.kitchensync/BAK/2026-07-02_10-30-00_000001Z")
            .is_dir()
    );
    assert_eq!(
        fs::read_to_string(
            root.join("sync-root/folder/.kitchensync/BAK/2026-07-02_10-30-00_000001Z/report.txt")
        )
        .expect("read displaced file"),
        "original contents"
    );
    assert!(!root
        .join("sync-root/.kitchensync/BAK/2026-07-02_10-30-00_000001Z/report.txt")
        .exists());
}

#[test]
fn displacing_directory_preserves_its_subtree_under_bak_destination() {
    let root = temp_root("directory");
    write_file(&root, "peer/docs/readme.txt", "readme");
    write_file(&root, "peer/docs/nested/details.txt", "details");

    let displacement = subject();
    let record = displacement
        .displace_to_bak(BakDisplacementRequest {
            peer: file_peer(&root),
            parent_path: "peer".to_owned(),
            basename: "docs".to_owned(),
            bak_timestamp: "2026-07-02_10-31-00_000002Z".to_owned(),
        })
        .expect("displace directory to BAK");

    assert_eq!(
        record.bak_destination_path,
        "peer/.kitchensync/BAK/2026-07-02_10-31-00_000002Z/docs"
    );
    assert!(!root.join("peer/docs").exists());
    assert_eq!(
        fs::read_to_string(
            root.join("peer/.kitchensync/BAK/2026-07-02_10-31-00_000002Z/docs/readme.txt")
        )
        .expect("read displaced child file"),
        "readme"
    );
    assert_eq!(
        fs::read_to_string(
            root.join("peer/.kitchensync/BAK/2026-07-02_10-31-00_000002Z/docs/nested/details.txt")
        )
        .expect("read displaced nested file"),
        "details"
    );
}
