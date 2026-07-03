use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use formatrules::{FormatRules, FormatRulesPeerIdentityRequest};
use peerconnections::{
    new, PeerConnections, PeerConnectionsDiagnostic, PeerConnectionsDiagnosticKind,
    PeerConnectionsDiagnosticLevel, PeerConnectionsPeerRole, PeerConnectionsStartupFailureReason,
    PeerConnectionsStartupRequest, PeerConnectionsStartupResult,
};
use snapshotdatabase::SnapshotDatabase;

static NEXT_TEST_ROOT: AtomicUsize = AtomicUsize::new(0);

struct TestRoot {
    path: PathBuf,
}

impl TestRoot {
    fn new(name: &str) -> Self {
        let id = NEXT_TEST_ROOT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "kitchensync_peerconnections_{name}_{}_{}",
            std::process::id(),
            id
        ));
        let _ = fs::remove_dir_all(&path);
        let _ = fs::remove_file(&path);
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn create_dir(&self) {
        fs::create_dir_all(&self.path).unwrap();
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
        let _ = fs::remove_file(&self.path);
    }
}

struct Subject {
    peer_connections: Arc<dyn PeerConnections>,
    format_rules: Arc<dyn FormatRules>,
    snapshot_database: Arc<dyn SnapshotDatabase>,
}

fn subject() -> Subject {
    let format_rules = formatrules::new();
    let peer_transport_surface = peertransportsurface::new();
    let snapshot_database =
        snapshotdatabase::new(format_rules.clone(), peer_transport_surface.clone());
    let peer_connections = new(
        format_rules.clone(),
        peer_transport_surface,
        snapshot_database.clone(),
    );

    Subject {
        peer_connections,
        format_rules,
        snapshot_database,
    }
}

fn path_arg(path: &Path) -> String {
    path.to_str()
        .expect("test temp paths must be valid UTF-8")
        .to_string()
}

fn expected_url(format_rules: &dyn FormatRules, path: &Path) -> String {
    format_rules
        .normalize_peer_identity(FormatRulesPeerIdentityRequest {
            peer_url: path_arg(path),
            current_working_directory: std::env::current_dir().unwrap(),
            os_username: None,
        })
        .unwrap()
}

fn request(peer_arguments: Vec<String>) -> PeerConnectionsStartupRequest {
    PeerConnectionsStartupRequest {
        dry_run: false,
        timeout_conn_seconds: 1,
        timeout_idle_seconds: 1,
        peer_arguments,
    }
}

fn ready(
    subject: &dyn PeerConnections,
    peer_arguments: Vec<String>,
) -> peerconnections::PeerConnectionsStartup {
    match subject.start(request(peer_arguments)) {
        PeerConnectionsStartupResult::Ready(startup) => startup,
        PeerConnectionsStartupResult::Failed(failure) => {
            panic!("expected ready startup, got {:?}", failure.reason)
        }
    }
}

fn failure(
    subject: &dyn PeerConnections,
    peer_arguments: Vec<String>,
) -> peerconnections::PeerConnectionsStartupFailure {
    match subject.start(request(peer_arguments)) {
        PeerConnectionsStartupResult::Failed(failure) => failure,
        PeerConnectionsStartupResult::Ready(startup) => {
            panic!("expected failed startup, got {} peers", startup.peers.len())
        }
    }
}

fn add_snapshot_history(snapshot_database: &dyn SnapshotDatabase, root: &Path) {
    let snapshot_path = root.join(".kitchensync").join("snapshot.db");
    fs::create_dir_all(snapshot_path.parent().unwrap()).unwrap();
    snapshot_database
        .create_snapshot_database(snapshot_path)
        .unwrap();
}

fn root_with_snapshot(subject: &Subject, name: &str) -> TestRoot {
    let root = TestRoot::new(name);
    root.create_dir();
    add_snapshot_history(subject.snapshot_database.as_ref(), root.path());
    root
}

fn root_without_snapshot(name: &str) -> TestRoot {
    let root = TestRoot::new(name);
    root.create_dir();
    root
}

fn root_that_cannot_be_created(name: &str) -> (TestRoot, PathBuf) {
    let parent_file = TestRoot::new(name);
    fs::write(parent_file.path(), b"not a directory").unwrap();
    let unreachable_root = parent_file.path().join("peer");
    (parent_file, unreachable_root)
}

fn snapshot_path_is_directory(root: &Path) {
    fs::create_dir_all(root.join(".kitchensync").join("snapshot.db")).unwrap();
}

fn diagnostic(
    peer_index: usize,
    kind: PeerConnectionsDiagnosticKind,
) -> PeerConnectionsDiagnostic {
    PeerConnectionsDiagnostic {
        level: PeerConnectionsDiagnosticLevel::Error,
        peer_index,
        kind,
    }
}

#[test]
fn ready_startup_returns_reachable_peers_with_stable_indices_roles_history_and_winning_urls() {
    let subject = subject();
    let canon = root_without_snapshot("ready_canon");
    let normal = root_with_snapshot(&subject, "ready_normal");
    let subordinate = root_with_snapshot(&subject, "ready_subordinate");

    let startup = ready(
        subject.peer_connections.as_ref(),
        vec![
            format!("+{}", path_arg(canon.path())),
            path_arg(normal.path()),
            format!("-{}", path_arg(subordinate.path())),
        ],
    );

    assert_eq!(startup.diagnostics, Vec::<PeerConnectionsDiagnostic>::new());
    assert_eq!(startup.peers.len(), 3);
    assert_eq!(startup.peers[0].peer_index, 0);
    assert_eq!(startup.peers[0].role, PeerConnectionsPeerRole::Canon);
    assert!(!startup.peers[0].had_snapshot_history);
    assert_eq!(
        startup.peers[0].winning_url,
        expected_url(subject.format_rules.as_ref(), canon.path())
    );
    assert_eq!(startup.peers[1].peer_index, 1);
    assert_eq!(startup.peers[1].role, PeerConnectionsPeerRole::Normal);
    assert!(startup.peers[1].had_snapshot_history);
    assert_eq!(
        startup.peers[1].winning_url,
        expected_url(subject.format_rules.as_ref(), normal.path())
    );
    assert_eq!(startup.peers[2].peer_index, 2);
    assert_eq!(startup.peers[2].role, PeerConnectionsPeerRole::Subordinate);
    assert!(startup.peers[2].had_snapshot_history);
    assert_eq!(
        startup.peers[2].winning_url,
        expected_url(subject.format_rules.as_ref(), subordinate.path())
    );
}

#[test]
fn reachable_non_canon_peer_without_snapshot_history_is_auto_subordinate() {
    let subject = subject();
    let canon = root_without_snapshot("auto_subordinate_canon");
    let normal_without_history = root_without_snapshot("auto_subordinate_normal");

    let startup = ready(
        subject.peer_connections.as_ref(),
        vec![
            format!("+{}", path_arg(canon.path())),
            path_arg(normal_without_history.path()),
        ],
    );

    assert_eq!(startup.peers.len(), 2);
    assert_eq!(startup.peers[0].role, PeerConnectionsPeerRole::Canon);
    assert!(!startup.peers[0].had_snapshot_history);
    assert_eq!(startup.peers[1].role, PeerConnectionsPeerRole::Subordinate);
    assert!(!startup.peers[1].had_snapshot_history);
}

#[test]
fn bracketed_fallbacks_try_candidates_in_order_and_keep_the_first_successful_url() {
    let subject = subject();
    let (_blocked_parent, blocked_candidate) =
        root_that_cannot_be_created("fallback_blocked_parent");
    let winning = root_with_snapshot(&subject, "fallback_winning");
    let later_candidate = TestRoot::new("fallback_later_candidate");
    let other = root_with_snapshot(&subject, "fallback_other");

    let startup = ready(
        subject.peer_connections.as_ref(),
        vec![
            format!(
                "+[{},{},{}]",
                path_arg(&blocked_candidate),
                path_arg(winning.path()),
                path_arg(later_candidate.path())
            ),
            path_arg(other.path()),
        ],
    );

    assert_eq!(startup.peers.len(), 2);
    assert_eq!(startup.peers[0].peer_index, 0);
    assert_eq!(startup.peers[0].role, PeerConnectionsPeerRole::Canon);
    assert_eq!(
        startup.peers[0].winning_url,
        expected_url(subject.format_rules.as_ref(), winning.path())
    );
    assert!(
        !later_candidate.path().exists(),
        "later fallback candidate must not be tried after a winner is selected"
    );
}

#[test]
fn unreachable_non_canon_peer_is_skipped_with_an_error_diagnostic() {
    let subject = subject();
    let reachable = root_with_snapshot(&subject, "unreachable_skip_reachable");
    let (_blocked_parent, unreachable) = root_that_cannot_be_created("unreachable_skip_blocked");
    let canon = root_without_snapshot("unreachable_skip_canon");

    let startup = ready(
        subject.peer_connections.as_ref(),
        vec![
            path_arg(reachable.path()),
            path_arg(&unreachable),
            format!("+{}", path_arg(canon.path())),
        ],
    );

    assert_eq!(startup.peers.len(), 2);
    assert_eq!(startup.peers[0].peer_index, 0);
    assert_eq!(startup.peers[1].peer_index, 2);
    assert_eq!(
        startup.diagnostics,
        vec![diagnostic(
            1,
            PeerConnectionsDiagnosticKind::PeerUnreachable
        )]
    );
}

#[test]
fn startup_fails_when_fewer_than_two_peers_are_reachable() {
    let subject = subject();
    let reachable = root_with_snapshot(&subject, "fewer_than_two_reachable");
    let (_blocked_parent, unreachable) = root_that_cannot_be_created("fewer_than_two_blocked");

    let failure = failure(
        subject.peer_connections.as_ref(),
        vec![path_arg(reachable.path()), path_arg(&unreachable)],
    );

    assert_eq!(
        failure.reason,
        PeerConnectionsStartupFailureReason::FewerThanTwoReachablePeers
    );
    assert_eq!(
        failure.diagnostics,
        vec![diagnostic(
            1,
            PeerConnectionsDiagnosticKind::PeerUnreachable
        )]
    );
}

#[test]
fn startup_fails_when_the_canon_peer_is_unreachable() {
    let subject = subject();
    let (_blocked_parent, unreachable_canon) = root_that_cannot_be_created("canon_blocked");
    let reachable_a = root_with_snapshot(&subject, "canon_unreachable_a");
    let reachable_b = root_with_snapshot(&subject, "canon_unreachable_b");

    let failure = failure(
        subject.peer_connections.as_ref(),
        vec![
            format!("+{}", path_arg(&unreachable_canon)),
            path_arg(reachable_a.path()),
            path_arg(reachable_b.path()),
        ],
    );

    assert_eq!(
        failure.reason,
        PeerConnectionsStartupFailureReason::CanonPeerUnreachable
    );
    assert_eq!(
        failure.diagnostics,
        vec![diagnostic(
            0,
            PeerConnectionsDiagnosticKind::PeerUnreachable
        )]
    );
}

#[test]
fn startup_fails_when_no_reachable_peer_has_snapshot_history_and_no_canon_is_designated() {
    let subject = subject();
    let left = root_without_snapshot("first_sync_left");
    let right = root_without_snapshot("first_sync_right");

    let failure = failure(
        subject.peer_connections.as_ref(),
        vec![path_arg(left.path()), path_arg(right.path())],
    );

    assert_eq!(
        failure.reason,
        PeerConnectionsStartupFailureReason::FirstSyncNeedsCanon
    );
    assert_eq!(failure.diagnostics, Vec::<PeerConnectionsDiagnostic>::new());
}

#[test]
fn startup_fails_when_all_reachable_peers_are_subordinate_after_auto_subordination() {
    let subject = subject();
    let explicit_subordinate = root_with_snapshot(&subject, "all_sub_explicit");
    let auto_subordinate = root_without_snapshot("all_sub_auto");

    let failure = failure(
        subject.peer_connections.as_ref(),
        vec![
            format!("-{}", path_arg(explicit_subordinate.path())),
            path_arg(auto_subordinate.path()),
        ],
    );

    assert_eq!(
        failure.reason,
        PeerConnectionsStartupFailureReason::NoContributingPeerReachable
    );
    assert_eq!(failure.diagnostics, Vec::<PeerConnectionsDiagnostic>::new());
}

#[test]
fn snapshot_startup_failure_excludes_only_that_peer_and_reports_a_diagnostic() {
    let subject = subject();
    let canon = root_with_snapshot(&subject, "snapshot_exclude_canon");
    let excluded = root_without_snapshot("snapshot_exclude_bad");
    snapshot_path_is_directory(excluded.path());
    let remaining = root_with_snapshot(&subject, "snapshot_exclude_remaining");

    let startup = ready(
        subject.peer_connections.as_ref(),
        vec![
            format!("+{}", path_arg(canon.path())),
            path_arg(excluded.path()),
            path_arg(remaining.path()),
        ],
    );

    assert_eq!(startup.peers.len(), 2);
    assert_eq!(startup.peers[0].peer_index, 0);
    assert_eq!(startup.peers[1].peer_index, 2);
    assert_eq!(
        startup.diagnostics,
        vec![diagnostic(
            1,
            PeerConnectionsDiagnosticKind::SnapshotStartupFailed
        )]
    );
}

#[test]
fn snapshot_startup_exclusion_rechecks_that_at_least_two_peers_remain_reachable() {
    let subject = subject();
    let retained = root_with_snapshot(&subject, "snapshot_exclude_count_retained");
    let excluded = root_without_snapshot("snapshot_exclude_count_bad");
    snapshot_path_is_directory(excluded.path());

    let failure = failure(
        subject.peer_connections.as_ref(),
        vec![path_arg(retained.path()), path_arg(excluded.path())],
    );

    assert_eq!(
        failure.reason,
        PeerConnectionsStartupFailureReason::FewerThanTwoReachablePeers
    );
    assert_eq!(
        failure.diagnostics,
        vec![diagnostic(
            1,
            PeerConnectionsDiagnosticKind::SnapshotStartupFailed
        )]
    );
}

#[test]
fn snapshot_startup_exclusion_rechecks_whether_the_canon_peer_remains_reachable() {
    let subject = subject();
    let canon = root_without_snapshot("snapshot_exclude_canon_bad");
    snapshot_path_is_directory(canon.path());
    let reachable_a = root_with_snapshot(&subject, "snapshot_exclude_canon_a");
    let reachable_b = root_with_snapshot(&subject, "snapshot_exclude_canon_b");

    let failure = failure(
        subject.peer_connections.as_ref(),
        vec![
            format!("+{}", path_arg(canon.path())),
            path_arg(reachable_a.path()),
            path_arg(reachable_b.path()),
        ],
    );

    assert_eq!(
        failure.reason,
        PeerConnectionsStartupFailureReason::CanonPeerUnreachable
    );
    assert_eq!(
        failure.diagnostics,
        vec![diagnostic(
            0,
            PeerConnectionsDiagnosticKind::SnapshotStartupFailed
        )]
    );
}
