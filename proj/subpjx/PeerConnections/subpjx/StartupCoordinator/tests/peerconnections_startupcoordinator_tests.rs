use std::fs;
use std::path::{Path, PathBuf};

use peerconnections_startupcoordinator::{
    new, StartupCoordinatorConnection, StartupCoordinatorErrorDiagnosticKind,
    StartupCoordinatorFatalReason, StartupCoordinatorFileConnection, StartupCoordinatorFileUrl,
    StartupCoordinatorGlobalSettings, StartupCoordinatorLocalEnvironment, StartupCoordinatorPeer,
    StartupCoordinatorPeerRole, StartupCoordinatorRequest, StartupCoordinatorRunMode,
    StartupCoordinatorStatus, StartupCoordinatorUrl, StartupCoordinatorUrlLocation,
    StartupCoordinatorUrlSettings,
};

#[test]
fn primary_url_wins_before_any_fallback_url_is_attempted() {
    let test_dir = fresh_test_dir("primary_url_wins_before_any_fallback_url_is_attempted");
    let subject = subject();

    let primary_path = test_dir.join("primary-created-by-normal-mode");
    let fallback_path = test_dir.join("fallback-must-not-be-created");
    let primary = file_url("primary", primary_path.clone());

    let result = subject.coordinate_startup(request(
        vec![
            peer(
                "canon",
                StartupCoordinatorPeerRole::Canon,
                primary.clone(),
                vec![file_url("fallback", fallback_path.clone())],
            ),
            peer(
                "second",
                StartupCoordinatorPeerRole::Normal,
                file_url("second-primary", existing_dir(&test_dir, "second-root")),
                Vec::new(),
            ),
        ],
        StartupCoordinatorRunMode::Normal,
        &test_dir,
    ));

    assert_eq!(result.status, StartupCoordinatorStatus::Ready);
    assert_eq!(reachable_peer(&result, "canon").winning_url, primary);
    assert!(primary_path.is_dir());
    assert!(
        !fallback_path.exists(),
        "fallback URLs must not be established when the primary URL succeeds"
    );

    remove_test_dir(&test_dir);
}

#[test]
fn fallback_urls_are_tried_in_order_until_the_first_successful_winner() {
    let test_dir =
        fresh_test_dir("fallback_urls_are_tried_in_order_until_the_first_successful_winner");
    let subject = subject();

    let primary_blocker = test_dir.join("primary-blocker");
    fs::write(&primary_blocker, b"not a directory")
        .expect("test setup should create a blocking file");
    let primary = file_url("peer-primary", primary_blocker.join("peer-root"));
    let first_fallback = file_url("peer-first-fallback", test_dir.join("first-fallback"));
    let untried_fallback_path = test_dir.join("untried-fallback");
    let untried_fallback = file_url("peer-untried-fallback", untried_fallback_path.clone());

    let canon_root = existing_dir(&test_dir, "canon-root");
    let result = subject.coordinate_startup(request(
        vec![
            peer(
                "canon",
                StartupCoordinatorPeerRole::Canon,
                file_url("canon-primary", canon_root.clone()),
                Vec::new(),
            ),
            peer(
                "with-fallbacks",
                StartupCoordinatorPeerRole::Normal,
                primary,
                vec![first_fallback.clone(), untried_fallback],
            ),
        ],
        StartupCoordinatorRunMode::Normal,
        &test_dir,
    ));

    assert_eq!(result.status, StartupCoordinatorStatus::Ready);

    let reachable = reachable_peer(&result, "with-fallbacks");
    assert_eq!(reachable.winning_url, first_fallback);
    assert_eq!(
        reachable.connection,
        StartupCoordinatorConnection::File(StartupCoordinatorFileConnection {
            local_peer_root_path: test_dir.join("first-fallback"),
        })
    );
    assert!(
        !untried_fallback_path.exists(),
        "later fallback URLs must not be established after a peer has a winner"
    );

    remove_test_dir(&test_dir);
}

#[test]
fn unreachable_peer_returns_one_structured_diagnostic_without_stopping_reachable_peers() {
    let test_dir = fresh_test_dir(
        "unreachable_peer_returns_one_structured_diagnostic_without_stopping_reachable_peers",
    );
    let subject = subject();

    let result = subject.coordinate_startup(request(
        vec![
            peer(
                "offline",
                StartupCoordinatorPeerRole::Subordinate,
                file_url("offline-primary", test_dir.join("missing-primary")),
                vec![file_url("offline-fallback", test_dir.join("missing-fallback"))],
            ),
            peer(
                "canon",
                StartupCoordinatorPeerRole::Canon,
                file_url("canon-primary", existing_dir(&test_dir, "canon-root")),
                Vec::new(),
            ),
            peer(
                "second",
                StartupCoordinatorPeerRole::Normal,
                file_url("second-primary", existing_dir(&test_dir, "second-root")),
                Vec::new(),
            ),
        ],
        StartupCoordinatorRunMode::DryRun,
        &test_dir,
    ));

    assert_eq!(result.status, StartupCoordinatorStatus::Ready);
    assert_eq!(reachable_peer(&result, "canon").peer_identity, "canon");
    assert_eq!(reachable_peer(&result, "second").peer_identity, "second");
    assert_eq!(result.unreachable_peers.len(), 1);

    let unreachable = &result.unreachable_peers[0];
    assert_eq!(unreachable.peer_identity, "offline");
    assert_eq!(unreachable.role, StartupCoordinatorPeerRole::Subordinate);
    assert_eq!(unreachable.diagnostic.kind, StartupCoordinatorErrorDiagnosticKind::UnreachablePeer);
    assert_eq!(unreachable.diagnostic.peer_identity, "offline");
    assert!(
        !unreachable.diagnostic.details.is_empty(),
        "an unreachable-peer diagnostic should include reportable detail"
    );

    remove_test_dir(&test_dir);
}

#[test]
fn startup_is_fatal_when_fewer_than_two_peers_are_reachable() {
    let test_dir = fresh_test_dir("startup_is_fatal_when_fewer_than_two_peers_are_reachable");
    let subject = subject();

    let result = subject.coordinate_startup(request(
        vec![
            peer(
                "canon",
                StartupCoordinatorPeerRole::Canon,
                file_url("canon-primary", existing_dir(&test_dir, "canon-root")),
                Vec::new(),
            ),
            peer(
                "offline",
                StartupCoordinatorPeerRole::Normal,
                file_url("offline-primary", test_dir.join("missing-primary")),
                Vec::new(),
            ),
        ],
        StartupCoordinatorRunMode::DryRun,
        &test_dir,
    ));

    assert_eq!(result.reachable_peers.len(), 1);
    assert_eq!(result.unreachable_peers.len(), 1);
    assert_fatal_reason(&result.status, StartupCoordinatorFatalReason::FewerThanTwoReachablePeers);

    remove_test_dir(&test_dir);
}

#[test]
fn startup_is_fatal_when_the_canon_peer_is_unreachable() {
    let test_dir = fresh_test_dir("startup_is_fatal_when_the_canon_peer_is_unreachable");
    let subject = subject();

    let result = subject.coordinate_startup(request(
        vec![
            peer(
                "canon",
                StartupCoordinatorPeerRole::Canon,
                file_url("canon-primary", test_dir.join("missing-canon-root")),
                Vec::new(),
            ),
            peer(
                "first-reachable",
                StartupCoordinatorPeerRole::Normal,
                file_url("first-primary", existing_dir(&test_dir, "first-root")),
                Vec::new(),
            ),
            peer(
                "second-reachable",
                StartupCoordinatorPeerRole::Subordinate,
                file_url("second-primary", existing_dir(&test_dir, "second-root")),
                Vec::new(),
            ),
        ],
        StartupCoordinatorRunMode::DryRun,
        &test_dir,
    ));

    assert_eq!(result.reachable_peers.len(), 2);
    assert_eq!(result.unreachable_peers.len(), 1);
    assert_fatal_reason(&result.status, StartupCoordinatorFatalReason::CanonPeerUnreachable);

    remove_test_dir(&test_dir);
}

fn subject() -> std::sync::Arc<dyn peerconnections_startupcoordinator::StartupCoordinator> {
    new(
        peerconnections_fileurlconnection::new(),
        peerconnections_sftpurlconnection::new(),
    )
}

fn request(
    peers: Vec<StartupCoordinatorPeer>,
    run_mode: StartupCoordinatorRunMode,
    test_dir: &Path,
) -> StartupCoordinatorRequest {
    StartupCoordinatorRequest {
        peers,
        global_connection: StartupCoordinatorGlobalSettings {
            timeout_conn_seconds: 5,
            timeout_idle_seconds: 7,
        },
        run_mode,
        local_environment: StartupCoordinatorLocalEnvironment {
            home_directory: test_dir.join("home"),
            known_hosts_path: test_dir.join("known_hosts"),
            ssh_agent_socket: None,
        },
    }
}

fn peer(
    identity: &str,
    role: StartupCoordinatorPeerRole,
    primary_url: StartupCoordinatorUrl,
    fallback_urls: Vec<StartupCoordinatorUrl>,
) -> StartupCoordinatorPeer {
    StartupCoordinatorPeer {
        identity: identity.to_string(),
        role,
        primary_url,
        fallback_urls,
    }
}

fn file_url(normalized_identity: &str, local_peer_root_path: PathBuf) -> StartupCoordinatorUrl {
    StartupCoordinatorUrl {
        normalized_identity: normalized_identity.to_string(),
        location: StartupCoordinatorUrlLocation::File(StartupCoordinatorFileUrl {
            local_peer_root_path,
        }),
        connection: StartupCoordinatorUrlSettings {
            timeout_conn_seconds: None,
            timeout_idle_seconds: None,
        },
    }
}

fn reachable_peer<'a>(
    result: &'a peerconnections_startupcoordinator::StartupCoordinatorResult,
    peer_identity: &str,
) -> &'a peerconnections_startupcoordinator::StartupCoordinatorReachablePeer {
    result
        .reachable_peers
        .iter()
        .find(|peer| peer.peer_identity == peer_identity)
        .expect("expected peer to be reachable")
}

fn assert_fatal_reason(status: &StartupCoordinatorStatus, reason: StartupCoordinatorFatalReason) {
    match status {
        StartupCoordinatorStatus::Fatal(reasons) => {
            assert!(reasons.contains(&reason), "fatal status should include {reason:?}");
        }
        StartupCoordinatorStatus::Ready => panic!("startup should be fatal"),
    }
}

fn existing_dir(test_dir: &Path, name: &str) -> PathBuf {
    let path = test_dir.join(name);
    fs::create_dir_all(&path).expect("test setup should create an existing peer root");
    path
}

fn fresh_test_dir(test_name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "kitchensync-startupcoordinator-{}-{}",
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
