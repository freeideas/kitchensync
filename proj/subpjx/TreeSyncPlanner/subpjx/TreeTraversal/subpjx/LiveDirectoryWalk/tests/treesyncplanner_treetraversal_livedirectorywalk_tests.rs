use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use treesyncplanner_treetraversal_livedirectorywalk::{
    self as live_directory_walk, LiveDirectoryEntryFact, LiveDirectoryFailedSubtreeFact,
    LiveDirectoryListedEntry, LiveDirectoryListedEntryKind, LiveDirectoryListingAttemptRequest,
    LiveDirectoryListingError, LiveDirectoryListingErrorCategory, LiveDirectoryParentEntryDecision,
    LiveDirectoryPeerChildDirectoryDecision, LiveDirectoryPeerChildDirectoryOutcome,
    LiveDirectoryPeerEligibilityReason, LiveDirectoryPeerEntryFact, LiveDirectoryProcessedEntry,
    LiveDirectoryRecursionRequest, LiveDirectorySubtreeSkipReason, LiveDirectoryTimestamp,
    LiveDirectoryWalkDiagnosticKind, LiveDirectoryWalkDiagnosticLevel,
    LiveDirectoryWalkDirectoryRequest, LiveDirectoryWalkPeer, LiveDirectoryWalkPeerRole,
};

fn file(name: &str) -> LiveDirectoryListedEntry {
    LiveDirectoryListedEntry {
        name: name.to_string(),
        kind: LiveDirectoryListedEntryKind::File {
            byte_size: 7,
            modified_time: LiveDirectoryTimestamp {
                unix_seconds: 100,
                nanoseconds: 2,
            },
        },
    }
}

fn directory(name: &str) -> LiveDirectoryListedEntry {
    LiveDirectoryListedEntry {
        name: name.to_string(),
        kind: LiveDirectoryListedEntryKind::Directory,
    }
}

fn listing_error(message: &str) -> LiveDirectoryListingError {
    LiveDirectoryListingError {
        category: LiveDirectoryListingErrorCategory::IoError,
        message: message.to_string(),
    }
}

fn static_peer(
    peer_id: &str,
    role: LiveDirectoryWalkPeerRole,
    is_canon: bool,
    entries: Vec<LiveDirectoryListedEntry>,
    starts: Arc<Mutex<Vec<LiveDirectoryListingAttemptRequest>>>,
    expected_started_before_completion: usize,
) -> LiveDirectoryWalkPeer {
    let peer_id_string = peer_id.to_string();

    LiveDirectoryWalkPeer {
        peer_id: peer_id_string.clone(),
        role,
        is_canon,
        listing_starter: Arc::new(move |request| {
            starts.lock().unwrap().push(request);

            let starts_for_completion = Arc::clone(&starts);
            let entries_for_completion = entries.clone();
            Box::new(move || {
                assert_eq!(
                    expected_started_before_completion,
                    starts_for_completion.lock().unwrap().len()
                );
                Ok(entries_for_completion)
            })
        }),
    }
}

fn scripted_peer(
    peer_id: &str,
    role: LiveDirectoryWalkPeerRole,
    is_canon: bool,
    results: Vec<Result<Vec<LiveDirectoryListedEntry>, LiveDirectoryListingError>>,
    starts: Arc<Mutex<Vec<LiveDirectoryListingAttemptRequest>>>,
) -> LiveDirectoryWalkPeer {
    let pending_results = Arc::new(Mutex::new(VecDeque::from(results)));

    LiveDirectoryWalkPeer {
        peer_id: peer_id.to_string(),
        role,
        is_canon,
        listing_starter: Arc::new(move |request| {
            starts.lock().unwrap().push(request);

            let pending_results_for_completion = Arc::clone(&pending_results);
            Box::new(move || {
                pending_results_for_completion
                    .lock()
                    .unwrap()
                    .pop_front()
                    .expect("test peer has a scripted result for every attempt")
            })
        }),
    }
}

fn entry_fact(name: &str) -> LiveDirectoryEntryFact {
    LiveDirectoryEntryFact {
        relative_directory_path: "root".to_string(),
        entry_name: name.to_string(),
        peer_entries: Vec::new(),
        peer_eligibility: Vec::new(),
    }
}

#[test]
fn lists_every_active_peer_before_awaiting_and_returns_sorted_live_entries() {
    let subject = live_directory_walk::new();
    let starts = Arc::new(Mutex::new(Vec::new()));

    let result = subject.list_directory(LiveDirectoryWalkDirectoryRequest {
        relative_directory_path: "root".to_string(),
        active_peers: vec![
            static_peer(
                "canon",
                LiveDirectoryWalkPeerRole::Contributing,
                true,
                vec![file("Beta"), directory("child")],
                Arc::clone(&starts),
                3,
            ),
            static_peer(
                "contributor",
                LiveDirectoryWalkPeerRole::Contributing,
                false,
                vec![file("alpha"), directory("beta")],
                Arc::clone(&starts),
                3,
            ),
            static_peer(
                "subordinate",
                LiveDirectoryWalkPeerRole::Subordinate,
                false,
                vec![file("subOnly")],
                Arc::clone(&starts),
                3,
            ),
        ],
        list_total_tries: 1,
    });

    assert!(result.diagnostics.is_empty());
    assert!(result.failed_subtrees.is_empty());
    assert!(result.subtree_skips.is_empty());

    let mut started_requests: Vec<(String, String, u32)> = starts
        .lock()
        .unwrap()
        .iter()
        .map(|request| {
            (
                request.peer_id.clone(),
                request.relative_directory_path.clone(),
                request.attempt_number,
            )
        })
        .collect();
    started_requests.sort();
    assert_eq!(
        vec![
            ("canon".to_string(), "root".to_string(), 1),
            ("contributor".to_string(), "root".to_string(), 1),
            ("subordinate".to_string(), "root".to_string(), 1),
        ],
        started_requests
    );

    let entry_names: Vec<String> = result
        .entry_facts
        .iter()
        .map(|entry| entry.entry_name.clone())
        .collect();
    assert_eq!(vec!["alpha", "Beta", "beta", "child", "subOnly"], entry_names);

    let subordinate_entry = result
        .entry_facts
        .iter()
        .find(|entry| entry.entry_name == "subOnly")
        .expect("subordinate live entry contributes to the directory entry set");
    assert_eq!(
        vec![LiveDirectoryPeerEntryFact {
            peer_id: "subordinate".to_string(),
            kind: file("subOnly").kind,
        }],
        subordinate_entry.peer_entries
    );

    for entry in &result.entry_facts {
        assert_eq!("root", entry.relative_directory_path);
        assert_eq!(3, entry.peer_eligibility.len());
        assert!(entry
            .peer_eligibility
            .iter()
            .all(|eligibility| eligibility.eligible
                && eligibility.reason == LiveDirectoryPeerEligibilityReason::ListedDirectory));
    }
}

#[test]
fn non_canon_listing_failure_is_retried_excluded_and_does_not_persist_to_later_runs() {
    let subject = live_directory_walk::new();
    let starts = Arc::new(Mutex::new(Vec::new()));

    let result = subject.list_directory(LiveDirectoryWalkDirectoryRequest {
        relative_directory_path: "branch".to_string(),
        active_peers: vec![
            scripted_peer(
                "canon",
                LiveDirectoryWalkPeerRole::Contributing,
                true,
                vec![Ok(vec![file("canon-file")])],
                Arc::clone(&starts),
            ),
            scripted_peer(
                "offline",
                LiveDirectoryWalkPeerRole::Contributing,
                false,
                vec![
                    Err(listing_error("temporary listing failure")),
                    Err(listing_error("final listing failure")),
                ],
                Arc::clone(&starts),
            ),
            scripted_peer(
                "subordinate",
                LiveDirectoryWalkPeerRole::Subordinate,
                false,
                vec![Ok(vec![file("subordinate-file")])],
                Arc::clone(&starts),
            ),
        ],
        list_total_tries: 2,
    });

    let offline_attempts: Vec<u32> = starts
        .lock()
        .unwrap()
        .iter()
        .filter(|request| request.peer_id == "offline")
        .map(|request| request.attempt_number)
        .collect();
    assert_eq!(vec![1, 2], offline_attempts);

    assert_eq!(
        vec![LiveDirectoryFailedSubtreeFact {
            peer_id: "offline".to_string(),
            relative_directory_path: "branch".to_string(),
        }],
        result.failed_subtrees
    );
    assert_eq!(1, result.diagnostics.len());
    assert_eq!(LiveDirectoryWalkDiagnosticLevel::Error, result.diagnostics[0].level);
    assert_eq!(
        LiveDirectoryWalkDiagnosticKind::DirectoryListingFailed,
        result.diagnostics[0].kind
    );
    assert_eq!(Some("offline".to_string()), result.diagnostics[0].peer_id);
    assert_eq!("branch", result.diagnostics[0].relative_directory_path);
    assert!(result.subtree_skips.is_empty());

    let entry_names: Vec<String> = result
        .entry_facts
        .iter()
        .map(|entry| entry.entry_name.clone())
        .collect();
    assert_eq!(vec!["canon-file", "subordinate-file"], entry_names);
    assert!(!entry_names.iter().any(|name| name == "offline-file"));

    for entry in &result.entry_facts {
        let offline_eligibility = entry
            .peer_eligibility
            .iter()
            .find(|eligibility| eligibility.peer_id == "offline")
            .expect("failed peer remains visible as ineligible for this subtree");
        assert!(!offline_eligibility.eligible);
        assert_eq!(
            LiveDirectoryPeerEligibilityReason::ListingFailedForSubtree,
            offline_eligibility.reason
        );
    }

    let later_starts = Arc::new(Mutex::new(Vec::new()));
    let later_result = subject.list_directory(LiveDirectoryWalkDirectoryRequest {
        relative_directory_path: "branch".to_string(),
        active_peers: vec![
            scripted_peer(
                "canon",
                LiveDirectoryWalkPeerRole::Contributing,
                true,
                vec![Ok(vec![file("canon-file")])],
                Arc::clone(&later_starts),
            ),
            scripted_peer(
                "offline",
                LiveDirectoryWalkPeerRole::Contributing,
                false,
                vec![Ok(vec![file("offline-file")])],
                Arc::clone(&later_starts),
            ),
        ],
        list_total_tries: 2,
    });

    let later_entry_names: Vec<String> = later_result
        .entry_facts
        .iter()
        .map(|entry| entry.entry_name.clone())
        .collect();
    assert_eq!(vec!["canon-file", "offline-file"], later_entry_names);
    assert!(later_result.failed_subtrees.is_empty());
    assert!(later_result.subtree_skips.is_empty());
}

#[test]
fn canon_listing_failure_skips_the_subtree_for_every_peer() {
    let subject = live_directory_walk::new();
    let starts = Arc::new(Mutex::new(Vec::new()));

    let result = subject.list_directory(LiveDirectoryWalkDirectoryRequest {
        relative_directory_path: "canon-missing".to_string(),
        active_peers: vec![
            scripted_peer(
                "canon",
                LiveDirectoryWalkPeerRole::Contributing,
                true,
                vec![Err(listing_error("canon failed"))],
                Arc::clone(&starts),
            ),
            scripted_peer(
                "contributor",
                LiveDirectoryWalkPeerRole::Contributing,
                false,
                vec![Ok(vec![file("contributor-file")])],
                Arc::clone(&starts),
            ),
            scripted_peer(
                "subordinate",
                LiveDirectoryWalkPeerRole::Subordinate,
                false,
                vec![Ok(vec![file("subordinate-file")])],
                Arc::clone(&starts),
            ),
        ],
        list_total_tries: 1,
    });

    assert!(result.entry_facts.is_empty());
    assert_eq!(1, result.subtree_skips.len());
    assert_eq!("canon-missing", result.subtree_skips[0].relative_directory_path);
    assert_eq!(
        LiveDirectorySubtreeSkipReason::CanonListingFailed,
        result.subtree_skips[0].reason
    );
    let mut skipped_peer_ids = result.subtree_skips[0].peer_ids.clone();
    skipped_peer_ids.sort();
    assert_eq!(
        vec![
            "canon".to_string(),
            "contributor".to_string(),
            "subordinate".to_string(),
        ],
        skipped_peer_ids
    );
    assert_eq!(1, result.failed_subtrees.len());
    assert_eq!("canon", result.failed_subtrees[0].peer_id);
}

#[test]
fn all_contributing_peer_failures_skip_the_subtree_even_when_subordinate_lists() {
    let subject = live_directory_walk::new();
    let starts = Arc::new(Mutex::new(Vec::new()));

    let result = subject.list_directory(LiveDirectoryWalkDirectoryRequest {
        relative_directory_path: "no-contributors".to_string(),
        active_peers: vec![
            scripted_peer(
                "contributor-a",
                LiveDirectoryWalkPeerRole::Contributing,
                false,
                vec![Err(listing_error("a failed"))],
                Arc::clone(&starts),
            ),
            scripted_peer(
                "contributor-b",
                LiveDirectoryWalkPeerRole::Contributing,
                false,
                vec![Err(listing_error("b failed"))],
                Arc::clone(&starts),
            ),
            scripted_peer(
                "subordinate",
                LiveDirectoryWalkPeerRole::Subordinate,
                false,
                vec![Ok(vec![file("subordinate-file")])],
                Arc::clone(&starts),
            ),
        ],
        list_total_tries: 1,
    });

    assert!(result.entry_facts.is_empty());
    assert_eq!(1, result.subtree_skips.len());
    assert_eq!(
        "no-contributors",
        result.subtree_skips[0].relative_directory_path
    );
    assert_eq!(
        LiveDirectorySubtreeSkipReason::AllContributingPeersFailed,
        result.subtree_skips[0].reason
    );
    let mut skipped_peer_ids = result.subtree_skips[0].peer_ids.clone();
    skipped_peer_ids.sort();
    assert_eq!(
        vec![
            "contributor-a".to_string(),
            "contributor-b".to_string(),
            "subordinate".to_string(),
        ],
        skipped_peer_ids
    );
    assert_eq!(2, result.failed_subtrees.len());
    let mut failed_peer_ids: Vec<String> = result
        .failed_subtrees
        .iter()
        .map(|fact| fact.peer_id.clone())
        .collect();
    failed_peer_ids.sort();
    assert_eq!(
        vec!["contributor-a".to_string(), "contributor-b".to_string()],
        failed_peer_ids
    );
    assert!(result
        .failed_subtrees
        .iter()
        .all(|fact| fact.relative_directory_path == "no-contributors"));
}

#[test]
fn recursion_intents_include_only_peers_that_keep_or_create_child_directories() {
    let subject = live_directory_walk::new();

    let intents = subject.form_child_recursion_intents(LiveDirectoryRecursionRequest {
        relative_directory_path: "root".to_string(),
        processed_entries: vec![
            LiveDirectoryProcessedEntry {
                entry_fact: entry_fact("plain-file"),
                parent_decision: LiveDirectoryParentEntryDecision::NotChildDirectory,
            },
            LiveDirectoryProcessedEntry {
                entry_fact: entry_fact("child-a"),
                parent_decision: LiveDirectoryParentEntryDecision::ChildDirectory {
                    peer_decisions: vec![
                        LiveDirectoryPeerChildDirectoryDecision {
                            peer_id: "canon".to_string(),
                            outcome: LiveDirectoryPeerChildDirectoryOutcome::KeepDirectory,
                        },
                        LiveDirectoryPeerChildDirectoryDecision {
                            peer_id: "contributor".to_string(),
                            outcome: LiveDirectoryPeerChildDirectoryOutcome::CreateDirectory,
                        },
                        LiveDirectoryPeerChildDirectoryDecision {
                            peer_id: "displaced".to_string(),
                            outcome: LiveDirectoryPeerChildDirectoryOutcome::DisplaceDirectory,
                        },
                        LiveDirectoryPeerChildDirectoryDecision {
                            peer_id: "absent".to_string(),
                            outcome: LiveDirectoryPeerChildDirectoryOutcome::NoDirectory,
                        },
                    ],
                },
            },
            LiveDirectoryProcessedEntry {
                entry_fact: entry_fact("child-b"),
                parent_decision: LiveDirectoryParentEntryDecision::ChildDirectory {
                    peer_decisions: vec![LiveDirectoryPeerChildDirectoryDecision {
                        peer_id: "displaced".to_string(),
                        outcome: LiveDirectoryPeerChildDirectoryOutcome::DisplaceDirectory,
                    }],
                },
            },
            LiveDirectoryProcessedEntry {
                entry_fact: entry_fact("child-c"),
                parent_decision: LiveDirectoryParentEntryDecision::ChildDirectory {
                    peer_decisions: vec![LiveDirectoryPeerChildDirectoryDecision {
                        peer_id: "canon".to_string(),
                        outcome: LiveDirectoryPeerChildDirectoryOutcome::KeepDirectory,
                    }],
                },
            },
        ],
    });

    assert_eq!(2, intents.len());
    assert_eq!("root/child-a", intents[0].relative_directory_path);
    let mut child_a_peer_ids = intents[0].peer_ids.clone();
    child_a_peer_ids.sort();
    assert_eq!(
        vec!["canon".to_string(), "contributor".to_string()],
        child_a_peer_ids
    );
    assert_eq!("root/child-c", intents[1].relative_directory_path);
    assert_eq!(vec!["canon".to_string()], intents[1].peer_ids);
}
