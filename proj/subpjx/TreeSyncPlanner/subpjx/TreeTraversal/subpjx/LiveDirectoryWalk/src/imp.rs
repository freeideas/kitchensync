use crate::api::*;
use std::collections::BTreeSet;
use std::sync::Arc;

struct LiveDirectoryWalkImpl;

struct PeerDirectoryListing {
    peer: LiveDirectoryWalkPeer,
    result: Result<Vec<LiveDirectoryListedEntry>, LiveDirectoryListingError>,
}

impl LiveDirectoryWalk for LiveDirectoryWalkImpl {
    fn list_directory(
        &self,
        request: LiveDirectoryWalkDirectoryRequest,
    ) -> LiveDirectoryWalkDirectoryResult {
        let total_tries = request.list_total_tries.max(1);
        let first_attempts: Vec<(LiveDirectoryWalkPeer, LiveDirectoryListingCompletion)> = request
            .active_peers
            .iter()
            .cloned()
            .map(|peer| {
                let completion = (peer.listing_starter)(LiveDirectoryListingAttemptRequest {
                    peer_id: peer.peer_id.clone(),
                    relative_directory_path: request.relative_directory_path.clone(),
                    attempt_number: 1,
                });
                (peer, completion)
            })
            .collect();

        let peer_listings: Vec<PeerDirectoryListing> = first_attempts
            .into_iter()
            .map(|(peer, first_completion)| PeerDirectoryListing {
                result: complete_listing_attempts(
                    &peer,
                    &request.relative_directory_path,
                    total_tries,
                    first_completion,
                ),
                peer,
            })
            .collect();

        let diagnostics = failed_listing_diagnostics(&request.relative_directory_path, &peer_listings);
        let failed_subtrees = failed_subtree_facts(&request.relative_directory_path, &peer_listings);

        if let Some(reason) = subtree_skip_reason(&peer_listings) {
            return LiveDirectoryWalkDirectoryResult {
                diagnostics,
                failed_subtrees,
                subtree_skips: vec![LiveDirectorySubtreeSkipFact {
                    relative_directory_path: request.relative_directory_path,
                    peer_ids: request
                        .active_peers
                        .into_iter()
                        .map(|peer| peer.peer_id)
                        .collect(),
                    reason,
                }],
                entry_facts: Vec::new(),
            };
        }

        LiveDirectoryWalkDirectoryResult {
            diagnostics,
            failed_subtrees,
            subtree_skips: Vec::new(),
            entry_facts: entry_facts(&request.relative_directory_path, &peer_listings),
        }
    }

    fn form_child_recursion_intents(
        &self,
        request: LiveDirectoryRecursionRequest,
    ) -> Vec<LiveDirectoryRecursionIntent> {
        request
            .processed_entries
            .into_iter()
            .filter_map(|processed_entry| match processed_entry.parent_decision {
                LiveDirectoryParentEntryDecision::NotChildDirectory => None,
                LiveDirectoryParentEntryDecision::ChildDirectory { peer_decisions } => {
                    let peer_ids: Vec<String> = peer_decisions
                        .into_iter()
                        .filter_map(|peer_decision| match peer_decision.outcome {
                            LiveDirectoryPeerChildDirectoryOutcome::KeepDirectory
                            | LiveDirectoryPeerChildDirectoryOutcome::CreateDirectory => {
                                Some(peer_decision.peer_id)
                            }
                            LiveDirectoryPeerChildDirectoryOutcome::DisplaceDirectory
                            | LiveDirectoryPeerChildDirectoryOutcome::NoDirectory => None,
                        })
                        .collect();

                    if peer_ids.is_empty() {
                        None
                    } else {
                        Some(LiveDirectoryRecursionIntent {
                            relative_directory_path: child_path(
                                &request.relative_directory_path,
                                &processed_entry.entry_fact.entry_name,
                            ),
                            peer_ids,
                        })
                    }
                }
            })
            .collect()
    }
}

pub fn new() -> std::sync::Arc<dyn LiveDirectoryWalk> {
    Arc::new(LiveDirectoryWalkImpl)
}

fn complete_listing_attempts(
    peer: &LiveDirectoryWalkPeer,
    relative_directory_path: &str,
    total_tries: u32,
    first_completion: LiveDirectoryListingCompletion,
) -> Result<Vec<LiveDirectoryListedEntry>, LiveDirectoryListingError> {
    let mut attempt_number = 1;
    let mut result = first_completion();

    while result.is_err() && attempt_number < total_tries {
        attempt_number += 1;
        let completion = (peer.listing_starter)(LiveDirectoryListingAttemptRequest {
            peer_id: peer.peer_id.clone(),
            relative_directory_path: relative_directory_path.to_string(),
            attempt_number,
        });
        result = completion();
    }

    result
}

fn failed_listing_diagnostics(
    relative_directory_path: &str,
    peer_listings: &[PeerDirectoryListing],
) -> Vec<LiveDirectoryWalkDiagnostic> {
    peer_listings
        .iter()
        .filter_map(|peer_listing| {
            peer_listing
                .result
                .as_ref()
                .err()
                .map(|error| LiveDirectoryWalkDiagnostic {
                    level: LiveDirectoryWalkDiagnosticLevel::Error,
                    kind: LiveDirectoryWalkDiagnosticKind::DirectoryListingFailed,
                    peer_id: Some(peer_listing.peer.peer_id.clone()),
                    relative_directory_path: relative_directory_path.to_string(),
                    message: error.message.clone(),
                })
        })
        .collect()
}

fn failed_subtree_facts(
    relative_directory_path: &str,
    peer_listings: &[PeerDirectoryListing],
) -> Vec<LiveDirectoryFailedSubtreeFact> {
    peer_listings
        .iter()
        .filter(|peer_listing| peer_listing.result.is_err())
        .map(|peer_listing| LiveDirectoryFailedSubtreeFact {
            peer_id: peer_listing.peer.peer_id.clone(),
            relative_directory_path: relative_directory_path.to_string(),
        })
        .collect()
}

fn subtree_skip_reason(
    peer_listings: &[PeerDirectoryListing],
) -> Option<LiveDirectorySubtreeSkipReason> {
    if peer_listings
        .iter()
        .any(|peer_listing| peer_listing.peer.is_canon && peer_listing.result.is_err())
    {
        return Some(LiveDirectorySubtreeSkipReason::CanonListingFailed);
    }

    let contributing_peer_count = peer_listings
        .iter()
        .filter(|peer_listing| peer_listing.peer.role == LiveDirectoryWalkPeerRole::Contributing)
        .count();
    let successful_contributing_peer_count = peer_listings
        .iter()
        .filter(|peer_listing| {
            peer_listing.peer.role == LiveDirectoryWalkPeerRole::Contributing
                && peer_listing.result.is_ok()
        })
        .count();

    if contributing_peer_count > 0 && successful_contributing_peer_count == 0 {
        Some(LiveDirectorySubtreeSkipReason::AllContributingPeersFailed)
    } else {
        None
    }
}

fn entry_facts(
    relative_directory_path: &str,
    peer_listings: &[PeerDirectoryListing],
) -> Vec<LiveDirectoryEntryFact> {
    sorted_live_entry_names(peer_listings)
        .into_iter()
        .map(|entry_name| LiveDirectoryEntryFact {
            relative_directory_path: relative_directory_path.to_string(),
            peer_entries: peer_entries_for_name(&entry_name, peer_listings),
            peer_eligibility: peer_eligibility(peer_listings),
            entry_name,
        })
        .collect()
}

fn sorted_live_entry_names(peer_listings: &[PeerDirectoryListing]) -> Vec<String> {
    let mut names = BTreeSet::new();

    for peer_listing in peer_listings {
        if let Ok(entries) = &peer_listing.result {
            for entry in entries {
                names.insert(entry.name.clone());
            }
        }
    }

    let mut names: Vec<String> = names.into_iter().collect();
    names.sort_by(|left, right| {
        left.to_lowercase()
            .cmp(&right.to_lowercase())
            .then_with(|| left.cmp(right))
    });
    names
}

fn peer_entries_for_name(
    entry_name: &str,
    peer_listings: &[PeerDirectoryListing],
) -> Vec<LiveDirectoryPeerEntryFact> {
    let mut peer_entries = Vec::new();

    for peer_listing in peer_listings {
        if let Ok(entries) = &peer_listing.result {
            for entry in entries {
                if entry.name == entry_name {
                    peer_entries.push(LiveDirectoryPeerEntryFact {
                        peer_id: peer_listing.peer.peer_id.clone(),
                        kind: entry.kind.clone(),
                    });
                }
            }
        }
    }

    peer_entries
}

fn peer_eligibility(peer_listings: &[PeerDirectoryListing]) -> Vec<LiveDirectoryPeerEligibility> {
    peer_listings
        .iter()
        .map(|peer_listing| {
            let eligible = peer_listing.result.is_ok();

            LiveDirectoryPeerEligibility {
                peer_id: peer_listing.peer.peer_id.clone(),
                role: peer_listing.peer.role,
                is_canon: peer_listing.peer.is_canon,
                eligible,
                reason: if eligible {
                    LiveDirectoryPeerEligibilityReason::ListedDirectory
                } else {
                    LiveDirectoryPeerEligibilityReason::ListingFailedForSubtree
                },
            }
        })
        .collect()
}

fn child_path(relative_directory_path: &str, entry_name: &str) -> String {
    if relative_directory_path.is_empty() {
        entry_name.to_string()
    } else {
        format!("{relative_directory_path}/{entry_name}")
    }
}
