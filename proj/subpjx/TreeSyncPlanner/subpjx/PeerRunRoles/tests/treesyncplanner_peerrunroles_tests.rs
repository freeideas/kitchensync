use treesyncplanner_peerrunroles::{
    new, PeerRunRole, PeerRunRoleFact, PeerRunRolesFatalStartup, PeerRunRolesRequest,
    PeerRunRolesResult, StartupPeerFact, StartupPeerReachability, StartupPeerRoleMarker,
};

fn peer(
    peer_identity: &str,
    reachability: StartupPeerReachability,
    role_marker: StartupPeerRoleMarker,
    had_snapshot_database_at_startup: bool,
) -> StartupPeerFact {
    StartupPeerFact {
        peer_identity: peer_identity.to_string(),
        reachability,
        role_marker,
        had_snapshot_database_at_startup,
    }
}

fn classify(peers: Vec<StartupPeerFact>) -> PeerRunRolesResult {
    let subject = new();
    subject.classify_startup_roles(PeerRunRolesRequest { peers })
}

fn success_facts(result: PeerRunRolesResult) -> Vec<PeerRunRoleFact> {
    match result {
        PeerRunRolesResult::Success(facts) => facts.active_peers,
        other => panic!("expected success, got {other:?}"),
    }
}

fn fatal_startup(result: PeerRunRolesResult) -> PeerRunRolesFatalStartup {
    match result {
        PeerRunRolesResult::FatalStartup(fatal) => fatal,
        other => panic!("expected fatal startup, got {other:?}"),
    }
}

#[test]
fn classifies_reachable_peers_by_canon_marker_snapshot_history_and_subordinate_marker() {
    let active_peers = success_facts(classify(vec![
        peer(
            "canon-without-history",
            StartupPeerReachability::Reachable,
            StartupPeerRoleMarker::Canon,
            false,
        ),
        peer(
            "normal-with-history",
            StartupPeerReachability::Reachable,
            StartupPeerRoleMarker::Normal,
            true,
        ),
        peer(
            "normal-without-history",
            StartupPeerReachability::Reachable,
            StartupPeerRoleMarker::Normal,
            false,
        ),
        peer(
            "marked-subordinate-with-history",
            StartupPeerReachability::Reachable,
            StartupPeerRoleMarker::Subordinate,
            true,
        ),
    ]));

    assert_eq!(
        active_peers,
        vec![
            PeerRunRoleFact {
                peer_identity: "canon-without-history".to_string(),
                is_canon: true,
                role: PeerRunRole::Contributing,
                is_active_target: true,
            },
            PeerRunRoleFact {
                peer_identity: "normal-with-history".to_string(),
                is_canon: false,
                role: PeerRunRole::Contributing,
                is_active_target: true,
            },
            PeerRunRoleFact {
                peer_identity: "normal-without-history".to_string(),
                is_canon: false,
                role: PeerRunRole::Subordinate,
                is_active_target: true,
            },
            PeerRunRoleFact {
                peer_identity: "marked-subordinate-with-history".to_string(),
                is_canon: false,
                role: PeerRunRole::Subordinate,
                is_active_target: true,
            },
        ]
    );
}

#[test]
fn allows_non_canon_run_when_a_reachable_normal_peer_has_snapshot_history() {
    let active_peers = success_facts(classify(vec![
        peer(
            "contributing",
            StartupPeerReachability::Reachable,
            StartupPeerRoleMarker::Normal,
            true,
        ),
        peer(
            "target-only",
            StartupPeerReachability::Reachable,
            StartupPeerRoleMarker::Normal,
            false,
        ),
    ]));

    assert_eq!(active_peers.len(), 2);
    assert_eq!(active_peers[0].role, PeerRunRole::Contributing);
    assert!(!active_peers[0].is_canon);
    assert_eq!(active_peers[1].role, PeerRunRole::Subordinate);
    assert!(active_peers[1].is_active_target);
}

#[test]
fn omits_unreachable_non_fatal_peers_from_all_active_role_facts() {
    let active_peers = success_facts(classify(vec![
        peer(
            "reachable-contributor",
            StartupPeerReachability::Reachable,
            StartupPeerRoleMarker::Normal,
            true,
        ),
        peer(
            "unreachable-with-history",
            StartupPeerReachability::Unreachable,
            StartupPeerRoleMarker::Normal,
            true,
        ),
        peer(
            "unreachable-without-history",
            StartupPeerReachability::Unreachable,
            StartupPeerRoleMarker::Normal,
            false,
        ),
    ]));

    assert_eq!(
        active_peers,
        vec![PeerRunRoleFact {
            peer_identity: "reachable-contributor".to_string(),
            is_canon: false,
            role: PeerRunRole::Contributing,
            is_active_target: true,
        }]
    );
}

#[test]
fn unreachable_state_does_not_persist_to_later_classifications() {
    let first_run_active_peers = success_facts(classify(vec![
        peer(
            "reachable-contributor",
            StartupPeerReachability::Reachable,
            StartupPeerRoleMarker::Normal,
            true,
        ),
        peer(
            "temporarily-unreachable",
            StartupPeerReachability::Unreachable,
            StartupPeerRoleMarker::Normal,
            true,
        ),
    ]));

    assert_eq!(first_run_active_peers.len(), 1);
    assert_eq!(
        first_run_active_peers[0].peer_identity,
        "reachable-contributor"
    );

    let later_run_active_peers = success_facts(classify(vec![peer(
        "temporarily-unreachable",
        StartupPeerReachability::Reachable,
        StartupPeerRoleMarker::Normal,
        true,
    )]));

    assert_eq!(
        later_run_active_peers,
        vec![PeerRunRoleFact {
            peer_identity: "temporarily-unreachable".to_string(),
            is_canon: false,
            role: PeerRunRole::Contributing,
            is_active_target: true,
        }]
    );
}

#[test]
fn first_sync_without_canon_is_fatal_with_required_status_and_stdout() {
    assert_eq!(
        fatal_startup(classify(vec![
            peer(
                "reachable-new-peer",
                StartupPeerReachability::Reachable,
                StartupPeerRoleMarker::Normal,
                false,
            ),
            peer(
                "unreachable-history-does-not-count",
                StartupPeerReachability::Unreachable,
                StartupPeerRoleMarker::Normal,
                true,
            ),
        ])),
        PeerRunRolesFatalStartup::FirstSyncRequiresCanon {
            exit_status: 1,
            stdout_line: "First sync? Mark the authoritative peer with a leading +".to_string(),
        }
    );
}

#[test]
fn no_reachable_contributing_peer_after_subordination_is_fatal() {
    assert_eq!(
        fatal_startup(classify(vec![
            peer(
                "marked-subordinate",
                StartupPeerReachability::Reachable,
                StartupPeerRoleMarker::Subordinate,
                true,
            ),
            peer(
                "unreachable-contributor",
                StartupPeerReachability::Unreachable,
                StartupPeerRoleMarker::Normal,
                true,
            ),
        ])),
        PeerRunRolesFatalStartup::NoContributingPeer {
            exit_status: 1,
            stdout_line: "No contributing peer reachable - cannot make sync decisions".to_string(),
        }
    );
}

#[test]
fn unreachable_canon_is_fatal_with_required_status_and_identity() {
    assert_eq!(
        fatal_startup(classify(vec![
            peer(
                "reachable-history",
                StartupPeerReachability::Reachable,
                StartupPeerRoleMarker::Normal,
                true,
            ),
            peer(
                "unreachable-canon",
                StartupPeerReachability::Unreachable,
                StartupPeerRoleMarker::Canon,
                true,
            ),
        ])),
        PeerRunRolesFatalStartup::UnreachableCanon {
            exit_status: 1,
            canon_peer_identity: "unreachable-canon".to_string(),
        }
    );
}
