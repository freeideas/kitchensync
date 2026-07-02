use treesyncplanner_treetraversal::{
    AcceptedExclude, AcceptedExcludeKind, ChildDirectoryDisposition,
    ChildDirectoryPeerDecision, ChildRecursionRequest, DirectoryListingFact,
    DirectoryListingOutcome, EntryPeerEligibility, LiveEntryKind, LiveDirectoryEntry,
    ProcessedEntryRecursionFact, SubtreeSkipReason, SyncTimestamp, TraverseDirectoryRequest,
    TreeTraversal, TreeTraversalDiagnosticKind, TreeTraversalDiagnosticLevel, TreeTraversalPeer,
    TreeTraversalPeerRole,
};

fn subject() -> std::sync::Arc<dyn TreeTraversal> {
    treesyncplanner_treetraversal::new(
        treesyncplanner_treetraversal_excludedpathfilter::new(),
        treesyncplanner_treetraversal_livedirectorywalk::new(),
    )
}

fn peer(peer_id: &str, role: TreeTraversalPeerRole, is_canon: bool) -> TreeTraversalPeer {
    TreeTraversalPeer {
        peer_id: peer_id.to_string(),
        role,
        is_canon,
    }
}

fn successful_listing(peer_id: &str, entries: Vec<LiveDirectoryEntry>) -> DirectoryListingFact {
    DirectoryListingFact {
        peer_id: peer_id.to_string(),
        relative_directory_path: "root".to_string(),
        tries_used: 1,
        outcome: DirectoryListingOutcome::Entries(entries),
    }
}

fn regular_file(name: &str) -> LiveDirectoryEntry {
    LiveDirectoryEntry {
        name: name.to_string(),
        kind: LiveEntryKind::File {
            byte_size: 7,
            modified_time: SyncTimestamp {
                unix_seconds: 1_700_000_000,
                nanoseconds: 0,
            },
        },
    }
}

fn directory(name: &str) -> LiveDirectoryEntry {
    LiveDirectoryEntry {
        name: name.to_string(),
        kind: LiveEntryKind::Directory,
    }
}

fn symbolic_link_file(name: &str) -> LiveDirectoryEntry {
    LiveDirectoryEntry {
        name: name.to_string(),
        kind: LiveEntryKind::SymbolicLinkFile,
    }
}

fn special_file(name: &str) -> LiveDirectoryEntry {
    LiveDirectoryEntry {
        name: name.to_string(),
        kind: LiveEntryKind::Special,
    }
}

fn all_eligible() -> EntryPeerEligibility {
    EntryPeerEligibility {
        snapshot_lookup: true,
        snapshot_update: true,
        file_mutation: true,
        directory_mutation: true,
        copy: true,
        deletion: true,
        displacement: true,
    }
}

#[test]
fn traverse_directory_uses_live_visible_entries_in_required_order() {
    let traversal = subject();

    let result = traversal.traverse_directory(TraverseDirectoryRequest {
        relative_directory_path: "root".to_string(),
        active_peers: vec![
            peer("canon", TreeTraversalPeerRole::Contributing, true),
            peer("remote", TreeTraversalPeerRole::Contributing, false),
            peer("archive", TreeTraversalPeerRole::Subordinate, false),
        ],
        accepted_excludes: vec![
            AcceptedExclude {
                relative_path: "root/skip-file".to_string(),
                kind: AcceptedExcludeKind::File,
            },
            AcceptedExclude {
                relative_path: "root/skip-dir".to_string(),
                kind: AcceptedExcludeKind::DirectorySubtree,
            },
        ],
        list_total_tries: 2,
        directory_listing_facts: vec![
            successful_listing(
                "canon",
                vec![
                    regular_file("Beta"),
                    regular_file("alpha"),
                    directory("child"),
                    regular_file("skip-file"),
                    directory(".git"),
                    symbolic_link_file("link"),
                ],
            ),
            DirectoryListingFact {
                peer_id: "remote".to_string(),
                relative_directory_path: "root".to_string(),
                tries_used: 2,
                outcome: DirectoryListingOutcome::Failed {
                    diagnostic: "permission denied".to_string(),
                },
            },
            successful_listing(
                "archive",
                vec![
                    regular_file("ALPHA"),
                    regular_file("gamma"),
                    directory("skip-dir"),
                    directory(".kitchensync"),
                    special_file("socket"),
                ],
            ),
        ],
    });

    assert_eq!(result.relative_directory_path, "root");
    assert!(result.subtree_skips.is_empty());
    assert_eq!(
        result
            .entries
            .iter()
            .map(|entry| entry.entry_name.as_str())
            .collect::<Vec<_>>(),
        vec!["ALPHA", "alpha", "Beta", "child", "gamma"]
    );
    assert_eq!(
        result
            .entries
            .iter()
            .map(|entry| entry.relative_path.as_str())
            .collect::<Vec<_>>(),
        vec![
            "root/ALPHA",
            "root/alpha",
            "root/Beta",
            "root/child",
            "root/gamma"
        ]
    );
    assert!(result.entries.iter().all(|entry| {
        !entry
            .peer_facts
            .iter()
            .any(|peer_fact| peer_fact.peer_id == "remote")
    }));

    let gamma = result
        .entries
        .iter()
        .find(|entry| entry.entry_name == "gamma")
        .expect("subordinate-only live entry should be processed");
    assert_eq!(gamma.peer_facts.len(), 2);
    assert_eq!(gamma.peer_facts[0].peer_id, "canon");
    assert_eq!(gamma.peer_facts[0].live_entry, None);
    assert_eq!(gamma.peer_facts[0].eligibility, all_eligible());
    assert_eq!(gamma.peer_facts[1].peer_id, "archive");
    assert_eq!(
        gamma.peer_facts[1]
            .live_entry
            .as_ref()
            .map(|entry| entry.name.as_str()),
        Some("gamma")
    );
    assert_eq!(gamma.peer_facts[1].eligibility, all_eligible());

    assert_eq!(result.listing_failures.len(), 1);
    assert_eq!(result.listing_failures[0].peer_id, "remote");
    assert_eq!(result.listing_failures[0].relative_directory_path, "root");
    assert_eq!(result.listing_failures[0].tries_used, 2);
    assert_eq!(result.listing_failures[0].diagnostic, "permission denied");
    assert_eq!(result.run_local_exclusions.len(), 1);
    assert_eq!(result.run_local_exclusions[0].peer_id, "remote");
    assert_eq!(result.run_local_exclusions[0].relative_directory_path, "root");
    assert_eq!(result.diagnostics.len(), 1);
    assert_eq!(result.diagnostics[0].level, TreeTraversalDiagnosticLevel::Error);
    assert_eq!(
        result.diagnostics[0].kind,
        TreeTraversalDiagnosticKind::DirectoryListingFailed
    );
    assert_eq!(result.diagnostics[0].peer_id.as_deref(), Some("remote"));
    assert_eq!(result.diagnostics[0].relative_path, "root");
}

#[test]
fn canon_listing_failure_skips_the_subtree_for_every_peer() {
    let traversal = subject();

    let result = traversal.traverse_directory(TraverseDirectoryRequest {
        relative_directory_path: "root".to_string(),
        active_peers: vec![
            peer("canon", TreeTraversalPeerRole::Contributing, true),
            peer("remote", TreeTraversalPeerRole::Contributing, false),
            peer("archive", TreeTraversalPeerRole::Subordinate, false),
        ],
        accepted_excludes: Vec::new(),
        list_total_tries: 3,
        directory_listing_facts: vec![
            DirectoryListingFact {
                peer_id: "canon".to_string(),
                relative_directory_path: "root".to_string(),
                tries_used: 3,
                outcome: DirectoryListingOutcome::Failed {
                    diagnostic: "canon unavailable".to_string(),
                },
            },
            successful_listing("remote", vec![regular_file("kept-out")]),
            successful_listing("archive", vec![regular_file("also-kept-out")]),
        ],
    });

    assert!(result.entries.is_empty());
    assert_eq!(result.subtree_skips.len(), 1);
    assert_eq!(result.subtree_skips[0].relative_directory_path, "root");
    assert_eq!(
        result.subtree_skips[0].peer_ids,
        vec![
            "canon".to_string(),
            "remote".to_string(),
            "archive".to_string()
        ]
    );
    assert_eq!(
        result.subtree_skips[0].reason,
        SubtreeSkipReason::CanonListingFailed
    );
    assert_eq!(result.listing_failures.len(), 1);
    assert_eq!(result.listing_failures[0].peer_id, "canon");
    assert_eq!(result.run_local_exclusions[0].peer_id, "canon");
}

#[test]
fn all_contributing_listing_failures_skip_subordinate_cleanup() {
    let traversal = subject();

    let result = traversal.traverse_directory(TraverseDirectoryRequest {
        relative_directory_path: "root".to_string(),
        active_peers: vec![
            peer("left", TreeTraversalPeerRole::Contributing, false),
            peer("right", TreeTraversalPeerRole::Contributing, false),
            peer("archive", TreeTraversalPeerRole::Subordinate, false),
        ],
        accepted_excludes: Vec::new(),
        list_total_tries: 2,
        directory_listing_facts: vec![
            DirectoryListingFact {
                peer_id: "left".to_string(),
                relative_directory_path: "root".to_string(),
                tries_used: 2,
                outcome: DirectoryListingOutcome::Failed {
                    diagnostic: "left failed".to_string(),
                },
            },
            DirectoryListingFact {
                peer_id: "right".to_string(),
                relative_directory_path: "root".to_string(),
                tries_used: 2,
                outcome: DirectoryListingOutcome::Failed {
                    diagnostic: "right failed".to_string(),
                },
            },
            successful_listing("archive", vec![regular_file("must-not-displace")]),
        ],
    });

    assert!(result.entries.is_empty());
    assert_eq!(result.subtree_skips.len(), 1);
    assert_eq!(
        result.subtree_skips[0].reason,
        SubtreeSkipReason::AllContributingListingsFailed
    );
    assert_eq!(
        result.subtree_skips[0].peer_ids,
        vec![
            "left".to_string(),
            "right".to_string(),
            "archive".to_string()
        ]
    );
    assert_eq!(
        result
            .listing_failures
            .iter()
            .map(|failure| failure.peer_id.as_str())
            .collect::<Vec<_>>(),
        vec!["left", "right"]
    );
}

#[test]
fn child_recursion_includes_only_peers_that_keep_or_create_the_child_directory() {
    let traversal = subject();

    let intents = traversal.plan_child_recursions(ChildRecursionRequest {
        parent_relative_directory_path: "root".to_string(),
        processed_entries: vec![
            ProcessedEntryRecursionFact {
                relative_path: "root/alpha".to_string(),
                peer_decisions: vec![
                    ChildDirectoryPeerDecision {
                        peer_id: "canon".to_string(),
                        disposition: ChildDirectoryDisposition::KeepsDirectory,
                    },
                    ChildDirectoryPeerDecision {
                        peer_id: "remote".to_string(),
                        disposition: ChildDirectoryDisposition::CreatesDirectory,
                    },
                    ChildDirectoryPeerDecision {
                        peer_id: "archive".to_string(),
                        disposition: ChildDirectoryDisposition::DisplacesDirectory,
                    },
                ],
            },
            ProcessedEntryRecursionFact {
                relative_path: "root/beta".to_string(),
                peer_decisions: vec![
                    ChildDirectoryPeerDecision {
                        peer_id: "canon".to_string(),
                        disposition: ChildDirectoryDisposition::DirectoryAbsent,
                    },
                    ChildDirectoryPeerDecision {
                        peer_id: "remote".to_string(),
                        disposition: ChildDirectoryDisposition::DisplacesDirectory,
                    },
                ],
            },
            ProcessedEntryRecursionFact {
                relative_path: "root/gamma".to_string(),
                peer_decisions: vec![ChildDirectoryPeerDecision {
                    peer_id: "canon".to_string(),
                    disposition: ChildDirectoryDisposition::NotAChildDirectory,
                }],
            },
        ],
    });

    assert_eq!(intents.len(), 1);
    assert_eq!(intents[0].relative_directory_path, "root/alpha");
    assert_eq!(
        intents[0].peer_ids,
        vec!["canon".to_string(), "remote".to_string()]
    );
}
