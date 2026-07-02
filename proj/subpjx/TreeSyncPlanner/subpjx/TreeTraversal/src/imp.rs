use std::sync::Arc;
use crate::api::*;

struct TreeTraversalImpl {
    excludedpathfilter: std::sync::Arc<dyn treesyncplanner_treetraversal_excludedpathfilter::ExcludedPathFilter>,
    livedirectorywalk: std::sync::Arc<dyn treesyncplanner_treetraversal_livedirectorywalk::LiveDirectoryWalk>,
}

impl TreeTraversal for TreeTraversalImpl {
    fn traverse_directory(&self, request: TraverseDirectoryRequest) -> TraverseDirectoryResult {
        let final_failures = final_listing_failures(&request);
        let listing_failures = final_failures
            .iter()
            .map(|fact| DirectoryListingFailureFact {
                peer_id: fact.peer_id.clone(),
                relative_directory_path: fact.relative_directory_path.clone(),
                tries_used: fact.tries_used,
                diagnostic: match &fact.outcome {
                    DirectoryListingOutcome::Failed { diagnostic } => diagnostic.clone(),
                    DirectoryListingOutcome::Entries(_) => String::new(),
                },
            })
            .collect::<Vec<_>>();
        let diagnostics = listing_failures
            .iter()
            .map(|failure| TreeTraversalDiagnostic {
                level: TreeTraversalDiagnosticLevel::Error,
                kind: TreeTraversalDiagnosticKind::DirectoryListingFailed,
                peer_id: Some(failure.peer_id.clone()),
                relative_path: failure.relative_directory_path.clone(),
                message: failure.diagnostic.clone(),
            })
            .collect::<Vec<_>>();
        let run_local_exclusions = listing_failures
            .iter()
            .map(|failure| RunLocalPeerExclusion {
                peer_id: failure.peer_id.clone(),
                relative_directory_path: failure.relative_directory_path.clone(),
            })
            .collect::<Vec<_>>();

        let failed_peer_ids = final_failures
            .iter()
            .map(|fact| fact.peer_id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        let active_peer_ids = request
            .active_peers
            .iter()
            .map(|peer| peer.peer_id.clone())
            .collect::<Vec<_>>();
        let canon_failed = request
            .active_peers
            .iter()
            .any(|peer| peer.is_canon && failed_peer_ids.contains(peer.peer_id.as_str()));
        let has_contributing_peer = request
            .active_peers
            .iter()
            .any(|peer| peer.role == TreeTraversalPeerRole::Contributing);
        let all_contributing_failed = has_contributing_peer
            && request
                .active_peers
                .iter()
                .filter(|peer| peer.role == TreeTraversalPeerRole::Contributing)
                .all(|peer| failed_peer_ids.contains(peer.peer_id.as_str()));

        let subtree_skips = if canon_failed {
            vec![SubtreeSkipIntent {
                relative_directory_path: request.relative_directory_path.clone(),
                peer_ids: active_peer_ids,
                reason: SubtreeSkipReason::CanonListingFailed,
            }]
        } else if all_contributing_failed {
            vec![SubtreeSkipIntent {
                relative_directory_path: request.relative_directory_path.clone(),
                peer_ids: active_peer_ids,
                reason: SubtreeSkipReason::AllContributingListingsFailed,
            }]
        } else {
            Vec::new()
        };

        let entries = if subtree_skips.is_empty() {
            entry_processing_facts(&request, &failed_peer_ids)
        } else {
            Vec::new()
        };

        TraverseDirectoryResult {
            relative_directory_path: request.relative_directory_path,
            diagnostics,
            listing_failures,
            run_local_exclusions,
            subtree_skips,
            entries,
        }
    }
    fn plan_child_recursions( &self, request: ChildRecursionRequest, ) -> Vec<ChildRecursionIntent> {
        request
            .processed_entries
            .into_iter()
            .filter_map(|entry| {
                let peer_ids = entry
                    .peer_decisions
                    .into_iter()
                    .filter(|decision| {
                        matches!(
                            decision.disposition,
                            ChildDirectoryDisposition::KeepsDirectory
                                | ChildDirectoryDisposition::CreatesDirectory
                        )
                    })
                    .map(|decision| decision.peer_id)
                    .collect::<Vec<_>>();

                if peer_ids.is_empty() {
                    None
                } else {
                    Some(ChildRecursionIntent {
                        relative_directory_path: entry.relative_path,
                        peer_ids,
                    })
                }
            })
            .collect()
    }
}

fn final_listing_failures(request: &TraverseDirectoryRequest) -> Vec<DirectoryListingFact> {
    request
        .directory_listing_facts
        .iter()
        .filter(|fact| {
            fact.relative_directory_path == request.relative_directory_path
                && fact.tries_used >= request.list_total_tries
                && matches!(fact.outcome, DirectoryListingOutcome::Failed { .. })
        })
        .cloned()
        .collect()
}

fn entry_processing_facts(
    request: &TraverseDirectoryRequest,
    failed_peer_ids: &std::collections::BTreeSet<&str>,
) -> Vec<EntryProcessingFact> {
    let visible_peers = request
        .active_peers
        .iter()
        .filter(|peer| !failed_peer_ids.contains(peer.peer_id.as_str()))
        .collect::<Vec<_>>();
    let successful_listings = request
        .directory_listing_facts
        .iter()
        .filter(|fact| {
            fact.relative_directory_path == request.relative_directory_path
                && !failed_peer_ids.contains(fact.peer_id.as_str())
                && matches!(fact.outcome, DirectoryListingOutcome::Entries(_))
        })
        .collect::<Vec<_>>();

    let mut entry_names = successful_listings
        .iter()
        .flat_map(|fact| match &fact.outcome {
            DirectoryListingOutcome::Entries(entries) => entries
                .iter()
                .filter_map(|entry| {
                    let relative_path = join_relative_path(
                        &request.relative_directory_path,
                        &entry.name,
                    );
                    if path_is_visible(&relative_path, &entry.kind, &request.accepted_excludes) {
                        Some(entry.name.clone())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>(),
            DirectoryListingOutcome::Failed { .. } => Vec::new(),
        })
        .collect::<Vec<_>>();
    entry_names.sort_by(|left, right| {
        left.to_lowercase()
            .cmp(&right.to_lowercase())
            .then_with(|| left.cmp(right))
    });
    entry_names.dedup();

    entry_names
        .into_iter()
        .map(|entry_name| {
            let peer_facts = visible_peers
                .iter()
                .map(|peer| EntryPeerFact {
                    peer_id: peer.peer_id.clone(),
                    role: peer.role,
                    is_canon: peer.is_canon,
                    live_entry: live_entry_for_peer(successful_listings.as_slice(), &peer.peer_id, &entry_name),
                    eligibility: all_eligible(),
                })
                .collect::<Vec<_>>();
            EntryProcessingFact {
                relative_path: join_relative_path(&request.relative_directory_path, &entry_name),
                entry_name,
                peer_facts,
            }
        })
        .collect()
}

fn live_entry_for_peer(
    listings: &[&DirectoryListingFact],
    peer_id: &str,
    entry_name: &str,
) -> Option<PeerLiveEntry> {
    listings
        .iter()
        .find(|fact| fact.peer_id == peer_id)
        .and_then(|fact| match &fact.outcome {
            DirectoryListingOutcome::Entries(entries) => entries
                .iter()
                .find(|entry| entry.name == entry_name)
                .map(|entry| PeerLiveEntry {
                    name: entry.name.clone(),
                    kind: entry.kind.clone(),
                }),
            DirectoryListingOutcome::Failed { .. } => None,
        })
}

fn path_is_visible(
    relative_path: &str,
    entry_kind: &LiveEntryKind,
    accepted_excludes: &[AcceptedExclude],
) -> bool {
    if is_builtin_excluded(relative_path, entry_kind) {
        return false;
    }

    !accepted_excludes.iter().any(|exclude| match exclude.kind {
        AcceptedExcludeKind::File => exclude.relative_path == relative_path,
        AcceptedExcludeKind::DirectorySubtree => {
            exclude.relative_path == relative_path
                || relative_path
                    .strip_prefix(exclude.relative_path.as_str())
                    .is_some_and(|suffix| suffix.starts_with('/'))
        }
    })
}

fn is_builtin_excluded(relative_path: &str, entry_kind: &LiveEntryKind) -> bool {
    matches!(
        entry_kind,
        LiveEntryKind::SymbolicLinkFile | LiveEntryKind::SymbolicLinkDirectory | LiveEntryKind::Special
    ) || matches!(
        entry_kind,
        LiveEntryKind::Directory
    ) && relative_path
        .rsplit('/')
        .next()
        .is_some_and(|name| name == ".kitchensync" || name == ".git")
}

fn join_relative_path(parent: &str, child: &str) -> String {
    if parent.is_empty() {
        child.to_string()
    } else {
        format!("{parent}/{child}")
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

pub fn new(excludedpathfilter: std::sync::Arc<dyn treesyncplanner_treetraversal_excludedpathfilter::ExcludedPathFilter>, livedirectorywalk: std::sync::Arc<dyn treesyncplanner_treetraversal_livedirectorywalk::LiveDirectoryWalk>) -> std::sync::Arc<dyn TreeTraversal> {
    Arc::new(TreeTraversalImpl { excludedpathfilter, livedirectorywalk })
}
