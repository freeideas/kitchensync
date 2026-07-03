use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use copystaging::{
    new, CopyStaging, CopyStagingCleanupStatus, CopyStagingCopyRequest, CopyStagingCopyStatus,
    CopyStagingDiagnosticKind, CopyStagingDirectoryRequest, CopyStagingDisplacementRequest,
    CopyStagingDisplacementStatus, CopyStagingFailurePhase, CopyStagingPeer,
    CopyStagingRunMode, CopyStagingRunOptions, CopyStagingSwapRecoveryStatus,
    CopyStagingVerbosity,
};
use formatrules::FormatRules;
use peertransportsurface::{
    ConnectedPeerRoot, PeerReadChunk, PeerTransportError, PeerTransportSurface,
};

static NEXT_TEST_ROOT: AtomicUsize = AtomicUsize::new(0);

struct TestRoot {
    path: PathBuf,
}

impl TestRoot {
    fn new(name: &str) -> Self {
        let id = NEXT_TEST_ROOT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "kitchensync_copystaging_{name}_{}_{}",
            std::process::id(),
            id
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn peer(&self) -> ConnectedPeerRoot {
        ConnectedPeerRoot {
            handle: Arc::new(self.path.clone()),
        }
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn subject() -> Arc<dyn CopyStaging> {
    new(formatrules::new(), peertransportsurface::new())
}

fn default_options(verbosity: CopyStagingVerbosity) -> CopyStagingRunOptions {
    CopyStagingRunOptions {
        mode: CopyStagingRunMode::Normal,
        max_copies: 1,
        retries_copy: 1,
        keep_bak_days: 90,
        keep_tmp_days: 2,
        verbosity,
    }
}

fn dry_run_options(verbosity: CopyStagingVerbosity) -> CopyStagingRunOptions {
    CopyStagingRunOptions {
        mode: CopyStagingRunMode::DryRun,
        max_copies: 1,
        retries_copy: 1,
        keep_bak_days: 90,
        keep_tmp_days: 2,
        verbosity,
    }
}

fn staging_peer(peer_index: usize, peer_url: &str, root: ConnectedPeerRoot) -> CopyStagingPeer {
    CopyStagingPeer {
        peer_index,
        peer_url: peer_url.to_string(),
        root,
    }
}

fn copy_request(
    options: CopyStagingRunOptions,
    source_peer: CopyStagingPeer,
    destination_peer: CopyStagingPeer,
    source_path: &str,
    destination_path: &str,
    relative_path: &str,
    winning_mod_time: std::time::SystemTime,
    winning_byte_size: i64,
) -> CopyStagingCopyRequest {
    CopyStagingCopyRequest {
        options,
        source_peer,
        destination_peer,
        source_path: source_path.to_string(),
        destination_path: destination_path.to_string(),
        relative_path: relative_path.to_string(),
        winning_mod_time,
        winning_byte_size,
    }
}

fn directory_request(
    options: CopyStagingRunOptions,
    peer: CopyStagingPeer,
    directory_relative_path: Option<&str>,
) -> CopyStagingDirectoryRequest {
    CopyStagingDirectoryRequest {
        options,
        peer,
        directory_relative_path: directory_relative_path.map(str::to_string),
    }
}

fn read_all(
    transport: &dyn PeerTransportSurface,
    peer: &ConnectedPeerRoot,
    path: &str,
) -> Vec<u8> {
    let mut handle = transport.open_read(peer, path).unwrap();
    let mut bytes = Vec::new();

    loop {
        match transport.read(&mut handle, 4).unwrap() {
            PeerReadChunk::Bytes(chunk) => bytes.extend(chunk),
            PeerReadChunk::Eof => break,
        }
    }

    transport.close_read(handle).unwrap();
    bytes
}

fn assert_not_found<T>(result: Result<T, PeerTransportError>) {
    assert!(matches!(result, Err(PeerTransportError::NotFound)));
}

fn bak_child_paths(root: &Path, parent: &str, basename: &str) -> Vec<PathBuf> {
    let bak_root = root.join(parent).join(".kitchensync").join("BAK");
    if !bak_root.exists() {
        return Vec::new();
    }

    let mut paths = fs::read_dir(bak_root)
        .unwrap()
        .map(|entry| entry.unwrap().path().join(basename))
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

fn write_path(root: &Path, relative_path: &str, bytes: &[u8]) {
    let path = root.join(relative_path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

#[test]
fn copy_file_writes_source_bytes_mod_time_progress_and_trace_lines() {
    let source_root = TestRoot::new("copy_source");
    let destination_root = TestRoot::new("copy_destination");
    write_path(source_root.path(), "from/file.txt", b"selected bytes");
    let transport = peertransportsurface::new();
    let winning_mod_time = UNIX_EPOCH + Duration::from_secs(1_700_000_001);
    let source_peer = staging_peer(0, "file:///source", source_root.peer());
    let destination_peer = staging_peer(1, "file:///destination", destination_root.peer());

    let result = subject().copy_file(copy_request(
        default_options(CopyStagingVerbosity::Trace),
        source_peer,
        destination_peer.clone(),
        "from/file.txt",
        "to/file.txt",
        "to/file.txt",
        winning_mod_time,
        14,
    ));

    assert_eq!(result.status, CopyStagingCopyStatus::Completed);
    assert_eq!(result.destination_peer_index, 1);
    assert_eq!(result.destination_peer_url, "file:///destination");
    assert_eq!(result.relative_path, "to/file.txt");
    assert_eq!(result.attempts, 1);
    assert!(result.diagnostics.is_empty());
    assert_eq!(
        read_all(transport.as_ref(), &destination_peer.root, "to/file.txt"),
        b"selected bytes"
    );
    assert_eq!(
        transport
            .stat(&destination_peer.root, "to/file.txt")
            .unwrap()
            .mod_time,
        winning_mod_time
    );
    assert!(result
        .output_lines
        .contains(&"copy-slots active=1/1".to_string()));
    assert!(result
        .output_lines
        .contains(&"copy-slots active=0/1".to_string()));
    assert!(result.output_lines.contains(&"C to/file.txt".to_string()));
}

#[test]
fn replacing_existing_file_leaves_new_live_file_and_old_file_in_bak() {
    let source_root = TestRoot::new("replace_source");
    let destination_root = TestRoot::new("replace_destination");
    write_path(source_root.path(), "incoming/name.txt", b"new contents");
    write_path(destination_root.path(), "folder/name.txt", b"old contents");
    let transport = peertransportsurface::new();
    let rules = formatrules::new();
    let winning_mod_time = UNIX_EPOCH + Duration::from_secs(1_700_000_002);
    let source_peer = staging_peer(0, "file:///source", source_root.peer());
    let destination_peer = staging_peer(1, "file:///destination", destination_root.peer());

    let result = subject().copy_file(copy_request(
        default_options(CopyStagingVerbosity::Info),
        source_peer,
        destination_peer.clone(),
        "incoming/name.txt",
        "folder/name.txt",
        "folder/name.txt",
        winning_mod_time,
        12,
    ));

    assert_eq!(result.status, CopyStagingCopyStatus::Completed);
    assert_eq!(
        read_all(transport.as_ref(), &destination_peer.root, "folder/name.txt"),
        b"new contents"
    );
    assert_eq!(
        transport
            .stat(&destination_peer.root, "folder/name.txt")
            .unwrap()
            .mod_time,
        winning_mod_time
    );
    let bak_paths = bak_child_paths(destination_root.path(), "folder", "name.txt");
    assert_eq!(bak_paths.len(), 1);
    assert_eq!(fs::read(&bak_paths[0]).unwrap(), b"old contents");
    let swap_paths = rules.user_swap_paths(Some("folder"), "name.txt").unwrap();
    assert!(!destination_root.path().join(swap_paths.directory_path).exists());
    assert_eq!(result.output_lines, vec!["C folder/name.txt".to_string()]);
}

#[test]
fn dry_run_copy_reports_planned_copy_without_changing_destination() {
    let source_root = TestRoot::new("dry_copy_source");
    let destination_root = TestRoot::new("dry_copy_destination");
    write_path(source_root.path(), "source.txt", b"new");
    write_path(destination_root.path(), "target.txt", b"old");
    let transport = peertransportsurface::new();
    let destination_peer = staging_peer(1, "file:///destination", destination_root.peer());

    let result = subject().copy_file(copy_request(
        dry_run_options(CopyStagingVerbosity::Info),
        staging_peer(0, "file:///source", source_root.peer()),
        destination_peer.clone(),
        "source.txt",
        "target.txt",
        "target.txt",
        UNIX_EPOCH + Duration::from_secs(1_700_000_003),
        3,
    ));

    assert_eq!(result.status, CopyStagingCopyStatus::PlannedDryRun);
    assert_eq!(result.attempts, 1);
    assert!(result.diagnostics.is_empty());
    assert_eq!(
        read_all(transport.as_ref(), &destination_peer.root, "target.txt"),
        b"old"
    );
    assert_eq!(result.output_lines, vec!["C target.txt".to_string()]);
}

#[test]
fn missing_source_copy_retries_to_the_limit_and_reports_read_source_failure() {
    let source_root = TestRoot::new("missing_source");
    let destination_root = TestRoot::new("missing_destination");
    let mut options = default_options(CopyStagingVerbosity::Error);
    options.retries_copy = 2;
    let destination_peer = staging_peer(1, "file:///destination", destination_root.peer());

    let result = subject().copy_file(copy_request(
        options,
        staging_peer(0, "file:///source", source_root.peer()),
        destination_peer,
        "missing.txt",
        "target.txt",
        "target.txt",
        UNIX_EPOCH + Duration::from_secs(1_700_000_004),
        0,
    ));

    assert_eq!(result.status, CopyStagingCopyStatus::Failed);
    assert_eq!(result.attempts, 2);
    assert!(result.output_lines.is_empty());
    assert!(result.diagnostics.iter().any(|diagnostic| {
        diagnostic.peer_index == 1
            && diagnostic.peer_url == "file:///destination"
            && diagnostic.relative_path.as_deref() == Some("target.txt")
            && matches!(
                diagnostic.kind,
                CopyStagingDiagnosticKind::TransferFailed {
                    phase: CopyStagingFailurePhase::ReadSource,
                    transport_error: Some(PeerTransportError::NotFound)
                }
            )
    }));
    assert!(result
        .diagnostics
        .iter()
        .any(|diagnostic| matches!(diagnostic.kind, CopyStagingDiagnosticKind::CopyTriesExhausted)));
}

#[test]
fn recover_swap_with_old_and_live_archives_old_and_removes_new() {
    let root = TestRoot::new("recover_old_live");
    let rules = formatrules::new();
    let swap = rules.user_swap_paths(None, "file.txt").unwrap();
    write_path(root.path(), "file.txt", b"live");
    write_path(root.path(), &swap.old_path, b"old");
    write_path(root.path(), &swap.new_path, b"new");
    let transport = peertransportsurface::new();
    let peer = staging_peer(0, "file:///peer", root.peer());

    let result = subject().recover_user_swap(directory_request(
        default_options(CopyStagingVerbosity::Info),
        peer.clone(),
        None,
    ));

    assert_eq!(result.status, CopyStagingSwapRecoveryStatus::Recovered);
    assert!(result.output_lines.is_empty());
    assert!(result.diagnostics.is_empty());
    assert_eq!(read_all(transport.as_ref(), &peer.root, "file.txt"), b"live");
    assert_eq!(bak_child_paths(root.path(), "", "file.txt").len(), 1);
    assert!(!root.path().join(swap.directory_path).exists());
}

#[test]
fn recover_swap_with_old_and_new_without_live_moves_new_live_and_archives_old() {
    let root = TestRoot::new("recover_old_new");
    let rules = formatrules::new();
    let swap = rules.user_swap_paths(None, "file.txt").unwrap();
    write_path(root.path(), &swap.old_path, b"old");
    write_path(root.path(), &swap.new_path, b"new");
    let transport = peertransportsurface::new();
    let peer = staging_peer(0, "file:///peer", root.peer());

    let result = subject().recover_user_swap(directory_request(
        default_options(CopyStagingVerbosity::Info),
        peer.clone(),
        None,
    ));

    assert_eq!(result.status, CopyStagingSwapRecoveryStatus::Recovered);
    assert_eq!(read_all(transport.as_ref(), &peer.root, "file.txt"), b"new");
    let bak_paths = bak_child_paths(root.path(), "", "file.txt");
    assert_eq!(bak_paths.len(), 1);
    assert_eq!(fs::read(&bak_paths[0]).unwrap(), b"old");
    assert!(!root.path().join(swap.directory_path).exists());
}

#[test]
fn recover_swap_with_only_old_without_live_restores_old_live() {
    let root = TestRoot::new("recover_only_old");
    let rules = formatrules::new();
    let swap = rules.user_swap_paths(None, "file.txt").unwrap();
    write_path(root.path(), &swap.old_path, b"old");
    let transport = peertransportsurface::new();
    let peer = staging_peer(0, "file:///peer", root.peer());

    let result = subject().recover_user_swap(directory_request(
        default_options(CopyStagingVerbosity::Info),
        peer.clone(),
        None,
    ));

    assert_eq!(result.status, CopyStagingSwapRecoveryStatus::Recovered);
    assert_eq!(read_all(transport.as_ref(), &peer.root, "file.txt"), b"old");
    assert!(bak_child_paths(root.path(), "", "file.txt").is_empty());
    assert!(!root.path().join(swap.directory_path).exists());
}

#[test]
fn recover_swap_with_only_new_obeys_live_target_presence() {
    let root = TestRoot::new("recover_only_new");
    let rules = formatrules::new();
    let with_live = rules.user_swap_paths(None, "kept.txt").unwrap();
    let without_live = rules.user_swap_paths(None, "restored.txt").unwrap();
    write_path(root.path(), "kept.txt", b"live");
    write_path(root.path(), &with_live.new_path, b"stray");
    write_path(root.path(), &without_live.new_path, b"new");
    let transport = peertransportsurface::new();
    let peer = staging_peer(0, "file:///peer", root.peer());

    let result = subject().recover_user_swap(directory_request(
        default_options(CopyStagingVerbosity::Info),
        peer.clone(),
        None,
    ));

    assert_eq!(result.status, CopyStagingSwapRecoveryStatus::Recovered);
    assert_eq!(read_all(transport.as_ref(), &peer.root, "kept.txt"), b"live");
    assert_eq!(
        read_all(transport.as_ref(), &peer.root, "restored.txt"),
        b"new"
    );
    assert!(!root.path().join(with_live.directory_path).exists());
    assert!(!root.path().join(without_live.directory_path).exists());
}

#[test]
fn dry_run_swap_recovery_leaves_peer_swap_state_untouched() {
    let root = TestRoot::new("dry_swap");
    let rules = formatrules::new();
    let swap = rules.user_swap_paths(None, "file.txt").unwrap();
    write_path(root.path(), &swap.new_path, b"new");
    let peer = staging_peer(0, "file:///peer", root.peer());

    let result = subject().recover_user_swap(directory_request(
        dry_run_options(CopyStagingVerbosity::Trace),
        peer,
        None,
    ));

    assert_eq!(result.status, CopyStagingSwapRecoveryStatus::SkippedDryRun);
    assert!(result.output_lines.is_empty());
    assert!(result.diagnostics.is_empty());
    assert!(root.path().join(swap.new_path).exists());
}

#[test]
fn displace_file_to_bak_moves_live_file_and_emits_delete_progress_without_slots() {
    let root = TestRoot::new("displace_file");
    write_path(root.path(), "folder/remove.txt", b"remove me");
    let transport = peertransportsurface::new();
    let peer = staging_peer(0, "file:///peer", root.peer());

    let result = subject().displace_to_bak(CopyStagingDisplacementRequest {
        options: default_options(CopyStagingVerbosity::Trace),
        peer: peer.clone(),
        relative_path: "folder/remove.txt".to_string(),
        is_directory: false,
    });

    assert_eq!(result.status, CopyStagingDisplacementStatus::Displaced);
    assert_eq!(
        result.output_lines,
        vec!["X folder/remove.txt".to_string()]
    );
    assert!(result.diagnostics.is_empty());
    assert_not_found(transport.stat(&peer.root, "folder/remove.txt"));
    let bak_paths = bak_child_paths(root.path(), "folder", "remove.txt");
    assert_eq!(bak_paths.len(), 1);
    assert_eq!(fs::read(&bak_paths[0]).unwrap(), b"remove me");
}

#[test]
fn displace_directory_to_bak_moves_the_directory_tree() {
    let root = TestRoot::new("displace_directory");
    write_path(root.path(), "folder/remove/child.txt", b"child");
    write_path(root.path(), "folder/remove/nested/grandchild.txt", b"grandchild");
    let transport = peertransportsurface::new();
    let peer = staging_peer(0, "file:///peer", root.peer());

    let result = subject().displace_to_bak(CopyStagingDisplacementRequest {
        options: default_options(CopyStagingVerbosity::Info),
        peer: peer.clone(),
        relative_path: "folder/remove".to_string(),
        is_directory: true,
    });

    assert_eq!(result.status, CopyStagingDisplacementStatus::Displaced);
    assert_eq!(result.output_lines, vec!["X folder/remove".to_string()]);
    assert_not_found(transport.stat(&peer.root, "folder/remove"));
    let bak_paths = bak_child_paths(root.path(), "folder", "remove");
    assert_eq!(bak_paths.len(), 1);
    assert_eq!(fs::read(bak_paths[0].join("child.txt")).unwrap(), b"child");
    assert_eq!(
        fs::read(bak_paths[0].join("nested").join("grandchild.txt")).unwrap(),
        b"grandchild"
    );
}

#[test]
fn dry_run_displacement_reports_plan_without_moving_live_path() {
    let root = TestRoot::new("dry_displace");
    write_path(root.path(), "remove.txt", b"keep");
    let transport = peertransportsurface::new();
    let peer = staging_peer(0, "file:///peer", root.peer());

    let result = subject().displace_to_bak(CopyStagingDisplacementRequest {
        options: dry_run_options(CopyStagingVerbosity::Info),
        peer: peer.clone(),
        relative_path: "remove.txt".to_string(),
        is_directory: false,
    });

    assert_eq!(result.status, CopyStagingDisplacementStatus::PlannedDryRun);
    assert_eq!(result.output_lines, vec!["X remove.txt".to_string()]);
    assert_eq!(read_all(transport.as_ref(), &peer.root, "remove.txt"), b"keep");
    assert!(bak_child_paths(root.path(), "", "remove.txt").is_empty());
}

#[test]
fn error_verbosity_suppresses_copy_and_delete_progress_lines() {
    let source_root = TestRoot::new("quiet_source");
    let destination_root = TestRoot::new("quiet_destination");
    write_path(source_root.path(), "source.txt", b"bytes");
    write_path(destination_root.path(), "remove.txt", b"remove");
    let copy_result = subject().copy_file(copy_request(
        default_options(CopyStagingVerbosity::Error),
        staging_peer(0, "file:///source", source_root.peer()),
        staging_peer(1, "file:///destination", destination_root.peer()),
        "source.txt",
        "target.txt",
        "target.txt",
        UNIX_EPOCH + Duration::from_secs(1_700_000_005),
        5,
    ));
    let displace_result = subject().displace_to_bak(CopyStagingDisplacementRequest {
        options: default_options(CopyStagingVerbosity::Error),
        peer: staging_peer(1, "file:///destination", destination_root.peer()),
        relative_path: "remove.txt".to_string(),
        is_directory: false,
    });

    assert!(copy_result.output_lines.is_empty());
    assert!(displace_result.output_lines.is_empty());
}

#[test]
fn cleanup_removes_only_expired_bak_and_tmp_timestamp_directories() {
    let root = TestRoot::new("cleanup");
    fs::create_dir_all(
        root.path()
            .join(".kitchensync")
            .join("BAK")
            .join("2000-01-01_00-00-00_000000Z"),
    )
    .unwrap();
    fs::create_dir_all(
        root.path()
            .join(".kitchensync")
            .join("BAK")
            .join("2999-01-01_00-00-00_000000Z"),
    )
    .unwrap();
    fs::create_dir_all(root.path().join(".kitchensync").join("BAK").join("not-a-time")).unwrap();
    fs::create_dir_all(
        root.path()
            .join(".kitchensync")
            .join("TMP")
            .join("2000-01-01_00-00-00_000000Z"),
    )
    .unwrap();
    fs::create_dir_all(
        root.path()
            .join(".kitchensync")
            .join("TMP")
            .join("2999-01-01_00-00-00_000000Z"),
    )
    .unwrap();
    fs::create_dir_all(
        root.path()
            .join(".kitchensync")
            .join("SWAP")
            .join("2000-01-01_00-00-00_000000Z"),
    )
    .unwrap();
    let peer = staging_peer(0, "file:///peer", root.peer());

    let result = subject().cleanup_metadata(directory_request(
        default_options(CopyStagingVerbosity::Trace),
        peer,
        None,
    ));

    assert_eq!(result.status, CopyStagingCleanupStatus::Completed);
    assert!(result.output_lines.is_empty());
    assert!(result.diagnostics.is_empty());
    assert!(!root
        .path()
        .join(".kitchensync/BAK/2000-01-01_00-00-00_000000Z")
        .exists());
    assert!(root
        .path()
        .join(".kitchensync/BAK/2999-01-01_00-00-00_000000Z")
        .exists());
    assert!(root.path().join(".kitchensync/BAK/not-a-time").exists());
    assert!(!root
        .path()
        .join(".kitchensync/TMP/2000-01-01_00-00-00_000000Z")
        .exists());
    assert!(root
        .path()
        .join(".kitchensync/TMP/2999-01-01_00-00-00_000000Z")
        .exists());
    assert!(root
        .path()
        .join(".kitchensync/SWAP/2000-01-01_00-00-00_000000Z")
        .exists());
}

#[test]
fn dry_run_cleanup_leaves_metadata_untouched() {
    let root = TestRoot::new("dry_cleanup");
    fs::create_dir_all(
        root.path()
            .join(".kitchensync")
            .join("BAK")
            .join("2000-01-01_00-00-00_000000Z"),
    )
    .unwrap();
    let peer = staging_peer(0, "file:///peer", root.peer());

    let result = subject().cleanup_metadata(directory_request(
        dry_run_options(CopyStagingVerbosity::Info),
        peer,
        None,
    ));

    assert_eq!(result.status, CopyStagingCleanupStatus::SkippedDryRun);
    assert!(result.output_lines.is_empty());
    assert!(result.diagnostics.is_empty());
    assert!(root
        .path()
        .join(".kitchensync/BAK/2000-01-01_00-00-00_000000Z")
        .exists());
}
