use treesyncplanner::{
    AcceptedExclude, AcceptedExcludeKind, DirectoryListingFact, DirectoryListingOutcome,
    DisplacementKind, LiveDirectoryEntry, LiveEntryKind, PeerRunRole, PlanningPeer, SnapshotEntryKind,
    SnapshotFact, SnapshotRow, SnapshotUpdateIntent, StartupFatalOutcome, StartupPeer,
    StartupPeerCommandRole, StartupRoleRequest, SyncTimestamp, TreeSyncAction, TreeSyncDiagnosticKind,
    TreeSyncDiagnosticLevel, TreeSyncPlanRequest, TreeSyncPlanner,
};

fn subject() -> std::sync::Arc<dyn TreeSyncPlanner> {
    let directoryoutcomes = treesyncplanner_directoryoutcomes::new();
    let groupfiledecision = treesyncplanner_fileoutcomes_groupfiledecision::new();
    let peerfileclassification = treesyncplanner_fileoutcomes_peerfileclassification::new();
    let fileoutcomes = treesyncplanner_fileoutcomes::new(groupfiledecision, peerfileclassification);
    let peerrunroles = treesyncplanner_peerrunroles::new();
    let excludedpathfilter = treesyncplanner_treetraversal_excludedpathfilter::new();
    let livedirectorywalk = treesyncplanner_treetraversal_livedirectorywalk::new();
    let treetraversal = treesyncplanner_treetraversal::new(excludedpathfilter, livedirectorywalk);
    let typeconflictoutcomes = treesyncplanner_typeconflictoutcomes::new();

    treesyncplanner::new(
        directoryoutcomes,
        fileoutcomes,
        peerrunroles,
        treetraversal,
        typeconflictoutcomes,
    )
}

fn ts(unix_seconds: i64) -> SyncTimestamp {
    SyncTimestamp {
        unix_seconds,
        nanoseconds: 0,
    }
}

fn file(name: &str, byte_size: u64, modified_time: i64) -> LiveDirectoryEntry {
    LiveDirectoryEntry {
        name: name.to_string(),
        kind: LiveEntryKind::File {
            byte_size,
            modified_time: ts(modified_time),
        },
    }
}

fn directory(name: &str) -> LiveDirectoryEntry {
    LiveDirectoryEntry {
        name: name.to_string(),
        kind: LiveEntryKind::Directory,
    }
}

fn listing(
    peer_id: &str,
    relative_directory_path: &str,
    entries: Vec<LiveDirectoryEntry>,
) -> DirectoryListingFact {
    DirectoryListingFact {
        peer_id: peer_id.to_string(),
        relative_directory_path: relative_directory_path.to_string(),
        tries_used: 1,
        outcome: DirectoryListingOutcome::Entries(entries),
    }
}

fn peer(peer_id: &str, role: PeerRunRole, is_canon: bool) -> PlanningPeer {
    PlanningPeer {
        peer_id: peer_id.to_string(),
        role,
        is_canon,
    }
}

fn is_at_or_under(path: &str, excluded_directory: &str) -> bool {
    path == excluded_directory
        || matches!(
            path.strip_prefix(excluded_directory),
            Some(rest) if rest.starts_with('/')
        )
}

#[test]
fn startup_roles_apply_canon_subordinate_and_snapshot_rules() {
    let planner = subject();

    let decision = planner.decide_startup_roles(StartupRoleRequest {
        reachable_peers: vec![
            StartupPeer {
                peer_id: "canon".to_string(),
                command_line_role: StartupPeerCommandRole::Canon,
                had_snapshot_at_startup: false,
            },
            StartupPeer {
                peer_id: "fresh".to_string(),
                command_line_role: StartupPeerCommandRole::Normal,
                had_snapshot_at_startup: false,
            },
            StartupPeer {
                peer_id: "forced-subordinate".to_string(),
                command_line_role: StartupPeerCommandRole::Subordinate,
                had_snapshot_at_startup: true,
            },
            StartupPeer {
                peer_id: "history".to_string(),
                command_line_role: StartupPeerCommandRole::Normal,
                had_snapshot_at_startup: true,
            },
        ],
        designated_canon_peer_id: Some("canon".to_string()),
    });

    assert_eq!(None, decision.fatal_outcome);
    assert_eq!(
        vec![
            ("canon", PeerRunRole::Contributing),
            ("fresh", PeerRunRole::Subordinate),
            ("forced-subordinate", PeerRunRole::Subordinate),
            ("history", PeerRunRole::Contributing),
        ],
        decision
            .peer_roles
            .iter()
            .map(|role| (role.peer_id.as_str(), role.role))
            .collect::<Vec<_>>()
    );
}

#[test]
fn startup_roles_report_first_sync_without_canon() {
    let planner = subject();

    let decision = planner.decide_startup_roles(StartupRoleRequest {
        reachable_peers: vec![
            StartupPeer {
                peer_id: "fresh-left".to_string(),
                command_line_role: StartupPeerCommandRole::Normal,
                had_snapshot_at_startup: false,
            },
            StartupPeer {
                peer_id: "fresh-right".to_string(),
                command_line_role: StartupPeerCommandRole::Normal,
                had_snapshot_at_startup: false,
            },
        ],
        designated_canon_peer_id: None,
    });

    assert_eq!(
        Some(StartupFatalOutcome::FirstSyncRequiresCanon {
            exit_code: 1,
            stdout_line: "First sync? Mark the authoritative peer with a leading +".to_string(),
        }),
        decision.fatal_outcome
    );
    assert!(decision.peer_roles.is_empty());
}

#[test]
fn startup_roles_report_fewer_than_two_reachable_peers() {
    let planner = subject();

    let decision = planner.decide_startup_roles(StartupRoleRequest {
        reachable_peers: vec![StartupPeer {
            peer_id: "only-peer".to_string(),
            command_line_role: StartupPeerCommandRole::Normal,
            had_snapshot_at_startup: true,
        }],
        designated_canon_peer_id: None,
    });

    assert_eq!(
        Some(StartupFatalOutcome::FewerThanTwoReachablePeers { exit_code: 1 }),
        decision.fatal_outcome
    );
    assert!(decision.peer_roles.is_empty());
}

#[test]
fn plan_uses_live_names_excludes_hidden_paths_and_targets_subordinates() {
    let planner = subject();

    let plan = planner.plan_sync_root(TreeSyncPlanRequest {
        peers: vec![
            peer("canon", PeerRunRole::Contributing, true),
            peer("target", PeerRunRole::Contributing, false),
            peer("subordinate", PeerRunRole::Subordinate, false),
        ],
        accepted_excludes: vec![AcceptedExclude {
            relative_path: "skip.txt".to_string(),
            kind: AcceptedExcludeKind::File,
        }],
        directory_listing_facts: vec![
            listing(
                "canon",
                "",
                vec![
                    file("beta.txt", 20, 200),
                    file("Alpha.txt", 10, 100),
                    file("skip.txt", 30, 300),
                    LiveDirectoryEntry {
                        name: ".git".to_string(),
                        kind: LiveEntryKind::Directory,
                    },
                    LiveDirectoryEntry {
                        name: "link.txt".to_string(),
                        kind: LiveEntryKind::SymbolicLinkFile,
                    },
                ],
            ),
            listing("target", "", Vec::new()),
            listing("subordinate", "", Vec::new()),
        ],
        snapshot_facts: vec![SnapshotFact {
            peer_id: "target".to_string(),
            relative_path: "snapshot-only.txt".to_string(),
            row: SnapshotRow {
                kind: SnapshotEntryKind::File,
                byte_size: Some(99),
                modified_time: Some(ts(99)),
                deleted_time: None,
                last_seen: Some(ts(99)),
            },
        }],
        list_total_tries: 2,
    });

    let copy_paths = plan
        .actions
        .iter()
        .filter_map(|action| match action {
            TreeSyncAction::CopyFile(copy) => Some((
                copy.source_peer_id.as_str(),
                copy.source_relative_path.as_str(),
                copy.destination_peer_id.as_str(),
                copy.destination_relative_path.as_str(),
                copy.winning_byte_size,
                copy.winning_modified_time,
            )),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(
        vec![
            ("canon", "Alpha.txt", "target", "Alpha.txt", 10, ts(100)),
            (
                "canon",
                "Alpha.txt",
                "subordinate",
                "Alpha.txt",
                10,
                ts(100),
            ),
            ("canon", "beta.txt", "target", "beta.txt", 20, ts(200)),
            (
                "canon",
                "beta.txt",
                "subordinate",
                "beta.txt",
                20,
                ts(200),
            ),
        ],
        copy_paths
    );
    assert!(!plan.actions.iter().any(|action| match action {
        TreeSyncAction::CopyFile(copy) => copy.source_relative_path == "skip.txt"
            || copy.source_relative_path == ".git"
            || copy.source_relative_path == "link.txt"
            || copy.source_relative_path == "snapshot-only.txt",
        TreeSyncAction::CreateDirectory(create) => create.relative_path == ".git",
        TreeSyncAction::DisplacePath(displace) => {
            displace.relative_path == ".git"
                || displace.relative_path == "link.txt"
                || displace.relative_path == "snapshot-only.txt"
        }
    }));
    assert!(!plan
        .snapshot_update_intents
        .iter()
        .any(|intent| match intent {
            SnapshotUpdateIntent::UpsertFile { relative_path, .. }
            | SnapshotUpdateIntent::UpsertDirectory { relative_path, .. }
            | SnapshotUpdateIntent::Tombstone { relative_path, .. } => {
                relative_path == "snapshot-only.txt"
            }
        }));
}

#[test]
fn non_canon_listing_failure_is_diagnostic_and_run_local_exclusion() {
    let planner = subject();

    let plan = planner.plan_sync_root(TreeSyncPlanRequest {
        peers: vec![
            peer("canon", PeerRunRole::Contributing, true),
            peer("failed-target", PeerRunRole::Contributing, false),
            peer("subordinate", PeerRunRole::Subordinate, false),
        ],
        accepted_excludes: Vec::new(),
        directory_listing_facts: vec![
            listing("canon", "", vec![file("canon.txt", 10, 100)]),
            DirectoryListingFact {
                peer_id: "failed-target".to_string(),
                relative_directory_path: "".to_string(),
                tries_used: 2,
                outcome: DirectoryListingOutcome::Failed {
                    diagnostic: "timeout".to_string(),
                },
            },
            listing("subordinate", "", Vec::new()),
        ],
        snapshot_facts: Vec::new(),
        list_total_tries: 2,
    });

    assert_eq!(
        vec![(
            TreeSyncDiagnosticLevel::Error,
            TreeSyncDiagnosticKind::DirectoryListingFailed,
            Some("failed-target"),
            "",
        )],
        plan.diagnostics
            .iter()
            .map(|diagnostic| (
                diagnostic.level,
                diagnostic.kind,
                diagnostic.peer_id.as_deref(),
                diagnostic.relative_path.as_str(),
            ))
            .collect::<Vec<_>>()
    );

    assert!(plan.actions.iter().all(|action| match action {
        TreeSyncAction::CopyFile(copy) => copy.destination_peer_id != "failed-target",
        TreeSyncAction::CreateDirectory(create) => create.peer_id != "failed-target",
        TreeSyncAction::DisplacePath(displace) => displace.peer_id != "failed-target",
    }));
    assert!(plan
        .snapshot_update_intents
        .iter()
        .all(|intent| match intent {
            SnapshotUpdateIntent::UpsertFile { peer_id, .. }
            | SnapshotUpdateIntent::UpsertDirectory { peer_id, .. }
            | SnapshotUpdateIntent::Tombstone { peer_id, .. } => peer_id != "failed-target",
        }));
    assert!(plan.actions.iter().any(|action| matches!(
        action,
        TreeSyncAction::CopyFile(copy)
            if copy.source_peer_id == "canon"
                && copy.destination_peer_id == "subordinate"
                && copy.destination_relative_path == "canon.txt"
    )));
}

#[test]
fn canon_listing_failure_blocks_the_subtree_for_every_peer() {
    let planner = subject();

    let plan = planner.plan_sync_root(TreeSyncPlanRequest {
        peers: vec![
            peer("canon", PeerRunRole::Contributing, true),
            peer("target", PeerRunRole::Contributing, false),
            peer("subordinate", PeerRunRole::Subordinate, false),
        ],
        accepted_excludes: Vec::new(),
        directory_listing_facts: vec![
            DirectoryListingFact {
                peer_id: "canon".to_string(),
                relative_directory_path: "".to_string(),
                tries_used: 3,
                outcome: DirectoryListingOutcome::Failed {
                    diagnostic: "permission denied".to_string(),
                },
            },
            listing("target", "", vec![file("target-only.txt", 10, 100)]),
            listing("subordinate", "", vec![file("subordinate-only.txt", 20, 200)]),
        ],
        snapshot_facts: Vec::new(),
        list_total_tries: 3,
    });

    assert_eq!(
        vec![(
            TreeSyncDiagnosticLevel::Error,
            TreeSyncDiagnosticKind::DirectoryListingFailed,
            Some("canon"),
            "",
        )],
        plan.diagnostics
            .iter()
            .map(|diagnostic| (
                diagnostic.level,
                diagnostic.kind,
                diagnostic.peer_id.as_deref(),
                diagnostic.relative_path.as_str(),
            ))
            .collect::<Vec<_>>()
    );
    assert!(plan.actions.is_empty());
    assert!(plan.snapshot_update_intents.is_empty());
    assert!(plan.directory_visit_intents.is_empty());
}

#[test]
fn without_canon_newer_file_vote_wins_and_preserves_source_case() {
    let planner = subject();

    let plan = planner.plan_sync_root(TreeSyncPlanRequest {
        peers: vec![
            peer("left", PeerRunRole::Contributing, false),
            peer("right", PeerRunRole::Contributing, false),
            peer("subordinate", PeerRunRole::Subordinate, false),
        ],
        accepted_excludes: Vec::new(),
        directory_listing_facts: vec![
            listing("left", "", vec![file("Report.TXT", 10, 100)]),
            listing("right", "", vec![file("Report.TXT", 12, 107)]),
            listing("subordinate", "", Vec::new()),
        ],
        snapshot_facts: Vec::new(),
        list_total_tries: 1,
    });

    let copies = plan
        .actions
        .iter()
        .filter_map(|action| match action {
            TreeSyncAction::CopyFile(copy) => Some((
                copy.source_peer_id.as_str(),
                copy.source_relative_path.as_str(),
                copy.destination_peer_id.as_str(),
                copy.destination_relative_path.as_str(),
                copy.winning_byte_size,
                copy.winning_modified_time,
            )),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(
        vec![
            ("right", "Report.TXT", "left", "Report.TXT", 12, ts(107)),
            (
                "right",
                "Report.TXT",
                "subordinate",
                "Report.TXT",
                12,
                ts(107),
            ),
        ],
        copies
    );
}

#[test]
fn subordinate_only_file_is_displaced_when_contributors_have_no_vote() {
    let planner = subject();

    let plan = planner.plan_sync_root(TreeSyncPlanRequest {
        peers: vec![
            peer("left", PeerRunRole::Contributing, false),
            peer("right", PeerRunRole::Contributing, false),
            peer("subordinate", PeerRunRole::Subordinate, false),
        ],
        accepted_excludes: Vec::new(),
        directory_listing_facts: vec![
            listing("left", "", Vec::new()),
            listing("right", "", Vec::new()),
            listing("subordinate", "", vec![file("extra.txt", 10, 100)]),
        ],
        snapshot_facts: Vec::new(),
        list_total_tries: 1,
    });

    assert_eq!(
        vec![("subordinate", "extra.txt", DisplacementKind::File)],
        plan.actions
            .iter()
            .filter_map(|action| match action {
                TreeSyncAction::DisplacePath(displace) => Some((
                    displace.peer_id.as_str(),
                    displace.relative_path.as_str(),
                    displace.kind,
                )),
                _ => None,
            })
            .collect::<Vec<_>>()
    );
    assert!(!plan.actions.iter().any(|action| matches!(
        action,
        TreeSyncAction::CopyFile(copy) if copy.source_relative_path == "extra.txt"
    )));
}

#[test]
fn directory_exclude_blocks_recursion_mutations_and_snapshot_updates() {
    let planner = subject();

    let plan = planner.plan_sync_root(TreeSyncPlanRequest {
        peers: vec![
            peer("canon", PeerRunRole::Contributing, true),
            peer("target", PeerRunRole::Contributing, false),
        ],
        accepted_excludes: vec![AcceptedExclude {
            relative_path: "Ignored".to_string(),
            kind: AcceptedExcludeKind::DirectorySubtree,
        }],
        directory_listing_facts: vec![
            listing("canon", "", vec![directory("Ignored")]),
            listing("target", "", Vec::new()),
            listing("canon", "Ignored", vec![file("child.txt", 10, 100)]),
            listing("target", "Ignored", Vec::new()),
        ],
        snapshot_facts: vec![
            SnapshotFact {
                peer_id: "target".to_string(),
                relative_path: "Ignored".to_string(),
                row: SnapshotRow {
                    kind: SnapshotEntryKind::Directory,
                    byte_size: None,
                    modified_time: None,
                    deleted_time: None,
                    last_seen: Some(ts(100)),
                },
            },
            SnapshotFact {
                peer_id: "target".to_string(),
                relative_path: "Ignored/child.txt".to_string(),
                row: SnapshotRow {
                    kind: SnapshotEntryKind::File,
                    byte_size: Some(1),
                    modified_time: Some(ts(1)),
                    deleted_time: None,
                    last_seen: Some(ts(1)),
                },
            },
        ],
        list_total_tries: 1,
    });

    assert!(plan.actions.iter().all(|action| match action {
        TreeSyncAction::CopyFile(copy) => {
            !is_at_or_under(&copy.source_relative_path, "Ignored")
                && !is_at_or_under(&copy.destination_relative_path, "Ignored")
        }
        TreeSyncAction::CreateDirectory(create) => !is_at_or_under(&create.relative_path, "Ignored"),
        TreeSyncAction::DisplacePath(displace) => {
            !is_at_or_under(&displace.relative_path, "Ignored")
        }
    }));
    assert!(plan
        .snapshot_update_intents
        .iter()
        .all(|intent| match intent {
            SnapshotUpdateIntent::UpsertFile { relative_path, .. }
            | SnapshotUpdateIntent::UpsertDirectory { relative_path, .. }
            | SnapshotUpdateIntent::Tombstone { relative_path, .. } => {
                !is_at_or_under(relative_path, "Ignored")
            }
        }));
    assert!(plan
        .directory_visit_intents
        .iter()
        .all(|intent| !is_at_or_under(&intent.relative_path, "Ignored")));
}

#[test]
fn parent_directory_entries_are_finished_before_child_file_work() {
    let planner = subject();

    let plan = planner.plan_sync_root(TreeSyncPlanRequest {
        peers: vec![
            peer("canon", PeerRunRole::Contributing, true),
            peer("target", PeerRunRole::Contributing, false),
        ],
        accepted_excludes: Vec::new(),
        directory_listing_facts: vec![
            listing("canon", "", vec![directory("Child"), file("z.txt", 9, 90)]),
            listing("target", "", Vec::new()),
            listing("canon", "Child", vec![file("nested.txt", 10, 100)]),
            listing("target", "Child", Vec::new()),
        ],
        snapshot_facts: Vec::new(),
        list_total_tries: 1,
    });

    assert!(plan.actions.iter().any(|action| matches!(
        action,
        TreeSyncAction::CreateDirectory(create)
            if create.peer_id == "target" && create.relative_path == "Child"
    )));
    assert!(plan.directory_visit_intents.iter().any(|intent| {
        intent.relative_path == "Child"
            && intent.peer_ids == vec!["canon".to_string(), "target".to_string()]
    }));

    let z_copy_index = plan
        .actions
        .iter()
        .position(|action| matches!(
            action,
            TreeSyncAction::CopyFile(copy) if copy.destination_relative_path == "z.txt"
        ))
        .expect("parent file copy should be planned");
    let nested_copy_index = plan
        .actions
        .iter()
        .position(|action| matches!(
            action,
            TreeSyncAction::CopyFile(copy) if copy.destination_relative_path == "Child/nested.txt"
        ))
        .expect("child file copy should be planned after parent entries");

    assert!(z_copy_index < nested_copy_index);
}

#[test]
fn non_canon_file_beats_directory_and_replaces_losing_paths() {
    let planner = subject();

    let plan = planner.plan_sync_root(TreeSyncPlanRequest {
        peers: vec![
            peer("file-peer", PeerRunRole::Contributing, false),
            peer("dir-peer", PeerRunRole::Contributing, false),
            peer("subordinate", PeerRunRole::Subordinate, false),
        ],
        accepted_excludes: Vec::new(),
        directory_listing_facts: vec![
            listing("file-peer", "", vec![file("Mixed", 11, 110)]),
            listing("dir-peer", "", vec![directory("Mixed")]),
            listing("subordinate", "", vec![directory("Mixed")]),
        ],
        snapshot_facts: Vec::new(),
        list_total_tries: 1,
    });

    let displacements = plan
        .actions
        .iter()
        .filter_map(|action| match action {
            TreeSyncAction::DisplacePath(displace) => Some((
                displace.peer_id.as_str(),
                displace.relative_path.as_str(),
                displace.kind,
            )),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        vec![
            ("dir-peer", "Mixed", DisplacementKind::DirectoryWholeSubtree),
            (
                "subordinate",
                "Mixed",
                DisplacementKind::DirectoryWholeSubtree,
            ),
        ],
        displacements
    );

    let copies = plan
        .actions
        .iter()
        .filter_map(|action| match action {
            TreeSyncAction::CopyFile(copy) => Some((
                copy.source_peer_id.as_str(),
                copy.source_relative_path.as_str(),
                copy.destination_peer_id.as_str(),
                copy.destination_relative_path.as_str(),
            )),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        vec![
            ("file-peer", "Mixed", "dir-peer", "Mixed"),
            ("file-peer", "Mixed", "subordinate", "Mixed"),
        ],
        copies
    );
    assert!(plan.directory_visit_intents.is_empty());
}
