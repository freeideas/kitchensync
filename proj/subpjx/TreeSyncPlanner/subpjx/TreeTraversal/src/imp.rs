use crate::api::*;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use treesyncplanner_treetraversal_excludedpathfilter as excluded_filter;
use treesyncplanner_treetraversal_livedirectorywalk as live_walk;

struct TreeTraversalImpl {
    excluded_path_filter: Arc<dyn excluded_filter::ExcludedPathFilter>,
    live_directory_walk: Arc<dyn live_walk::LiveDirectoryWalk>,
}

impl TreeTraversal for TreeTraversalImpl {
    fn traverse_directory(&self, request: TraverseDirectoryRequest) -> TraverseDirectoryResult {
        let _ = self.excluded_path_filter.build_run_policy(Vec::new());

        let final_failures = final_listing_failures(&request);
        let failed_peer_ids = final_failures
            .iter()
            .map(|fact| fact.peer_id.clone())
            .collect::<BTreeSet<_>>();

        let listing_failures = final_failures
            .iter()
            .map(|fact| DirectoryListingFailureFact {
                peer_id: fact.peer_id.clone(),
                relative_directory_path: fact.relative_directory_path.clone(),
                tries_used: fact.tries_used,
                diagnostic: failure_diagnostic(fact),
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

        let subtree_skips = subtree_skips(&request, &failed_peer_ids);
        let entries = if subtree_skips.is_empty() {
            entry_processing_facts(&request, &failed_peer_ids, self.excluded_path_filter.as_ref())
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

    fn plan_child_recursions(
        &self,
        request: ChildRecursionRequest,
    ) -> Vec<ChildRecursionIntent> {
        self.live_directory_walk
            .form_child_recursion_intents(live_walk::LiveDirectoryRecursionRequest {
                relative_directory_path: request.parent_relative_directory_path.clone(),
                processed_entries: request
                    .processed_entries
                    .into_iter()
                    .map(|entry| live_walk::LiveDirectoryProcessedEntry {
                        entry_fact: live_walk_entry_fact(
                            &request.parent_relative_directory_path,
                            &entry.relative_path,
                        ),
                        parent_decision: live_walk_parent_decision(entry.peer_decisions),
                    })
                    .collect(),
            })
            .into_iter()
            .map(|intent| ChildRecursionIntent {
                relative_directory_path: intent.relative_directory_path,
                peer_ids: intent.peer_ids,
            })
            .collect()
    }
}

fn final_listing_failures(request: &TraverseDirectoryRequest) -> Vec<DirectoryListingFact> {
    let active_peer_ids = request
        .active_peers
        .iter()
        .map(|peer| peer.peer_id.as_str())
        .collect::<BTreeSet<_>>();

    request
        .directory_listing_facts
        .iter()
        .filter(|fact| {
            active_peer_ids.contains(fact.peer_id.as_str())
                && fact.relative_directory_path == request.relative_directory_path
                && fact.tries_used >= request.list_total_tries
                && matches!(fact.outcome, DirectoryListingOutcome::Failed { .. })
        })
        .cloned()
        .collect()
}

fn failure_diagnostic(fact: &DirectoryListingFact) -> String {
    match &fact.outcome {
        DirectoryListingOutcome::Failed { diagnostic } => diagnostic.clone(),
        DirectoryListingOutcome::Entries(_) => String::new(),
    }
}

fn subtree_skips(
    request: &TraverseDirectoryRequest,
    failed_peer_ids: &BTreeSet<String>,
) -> Vec<SubtreeSkipIntent> {
    let active_peer_ids = request
        .active_peers
        .iter()
        .map(|peer| peer.peer_id.clone())
        .collect::<Vec<_>>();

    if request
        .active_peers
        .iter()
        .any(|peer| peer.is_canon && failed_peer_ids.contains(&peer.peer_id))
    {
        return vec![SubtreeSkipIntent {
            relative_directory_path: request.relative_directory_path.clone(),
            peer_ids: active_peer_ids,
            reason: SubtreeSkipReason::CanonListingFailed,
        }];
    }

    let contributing_peers = request
        .active_peers
        .iter()
        .filter(|peer| peer.role == TreeTraversalPeerRole::Contributing)
        .collect::<Vec<_>>();

    if !contributing_peers.is_empty()
        && contributing_peers
            .iter()
            .all(|peer| failed_peer_ids.contains(&peer.peer_id))
    {
        return vec![SubtreeSkipIntent {
            relative_directory_path: request.relative_directory_path.clone(),
            peer_ids: active_peer_ids,
            reason: SubtreeSkipReason::AllContributingListingsFailed,
        }];
    }

    Vec::new()
}

fn entry_processing_facts(
    request: &TraverseDirectoryRequest,
    failed_peer_ids: &BTreeSet<String>,
    excluded_path_filter: &dyn excluded_filter::ExcludedPathFilter,
) -> Vec<EntryProcessingFact> {
    let visible_peers = request
        .active_peers
        .iter()
        .filter(|peer| !failed_peer_ids.contains(&peer.peer_id))
        .collect::<Vec<_>>();
    let listings_by_peer = successful_listings_by_peer(request, failed_peer_ids);
    let mut entries_by_name = BTreeMap::new();

    for entries in listings_by_peer.values() {
        for entry in entries {
            let relative_path = join_relative_path(&request.relative_directory_path, &entry.name);
            if let Some(eligibility) = path_eligibility(
                &relative_path,
                &entry.kind,
                &request.accepted_excludes,
                excluded_path_filter,
            ) {
                entries_by_name.insert(entry.name.clone(), (relative_path, eligibility));
            }
        }
    }

    let mut entry_names = entries_by_name.keys().cloned().collect::<Vec<_>>();
    entry_names.sort_by(|left, right| {
        left.to_lowercase()
            .cmp(&right.to_lowercase())
            .then_with(|| left.cmp(right))
    });

    entry_names
        .into_iter()
        .map(|entry_name| {
            let (relative_path, eligibility) = entries_by_name
                .remove(&entry_name)
                .expect("entry name came from entry map");
            let peer_facts = visible_peers
                .iter()
                .map(|peer| EntryPeerFact {
                    peer_id: peer.peer_id.clone(),
                    role: peer.role,
                    is_canon: peer.is_canon,
                    live_entry: live_entry_for_peer(&listings_by_peer, &peer.peer_id, &entry_name),
                    eligibility: eligibility.clone(),
                })
                .collect();

            EntryProcessingFact {
                relative_path,
                entry_name,
                peer_facts,
            }
        })
        .collect()
}

fn successful_listings_by_peer(
    request: &TraverseDirectoryRequest,
    failed_peer_ids: &BTreeSet<String>,
) -> BTreeMap<String, Vec<LiveDirectoryEntry>> {
    let active_peer_ids = request
        .active_peers
        .iter()
        .map(|peer| peer.peer_id.as_str())
        .collect::<BTreeSet<_>>();
    let mut listings = BTreeMap::new();

    for fact in &request.directory_listing_facts {
        if !active_peer_ids.contains(fact.peer_id.as_str())
            || failed_peer_ids.contains(&fact.peer_id)
            || fact.relative_directory_path != request.relative_directory_path
        {
            continue;
        }

        if let DirectoryListingOutcome::Entries(entries) = &fact.outcome {
            listings
                .entry(fact.peer_id.clone())
                .or_insert_with(|| entries.clone());
        }
    }

    listings
}

fn live_entry_for_peer(
    listings_by_peer: &BTreeMap<String, Vec<LiveDirectoryEntry>>,
    peer_id: &str,
    entry_name: &str,
) -> Option<PeerLiveEntry> {
    listings_by_peer.get(peer_id).and_then(|entries| {
        entries
            .iter()
            .find(|entry| entry.name == entry_name)
            .map(|entry| PeerLiveEntry {
                name: entry.name.clone(),
                kind: entry.kind.clone(),
            })
    })
}

fn path_eligibility(
    relative_path: &str,
    entry_kind: &LiveEntryKind,
    accepted_excludes: &[AcceptedExclude],
    excluded_path_filter: &dyn excluded_filter::ExcludedPathFilter,
) -> Option<EntryPeerEligibility> {
    if accepted_excludes
        .iter()
        .any(|exclude| accepted_exclude_matches(exclude, relative_path))
    {
        return None;
    }

    excluded_path_filter
        .decide_path_visibility(excluded_filter::PathVisibilityRequest {
            relative_path: relative_path.to_string(),
            entry_kind: excluded_filter_entry_kind(entry_kind),
        })
        .ok()
        .and_then(|decision| {
            if decision.exclusion.is_some() {
                None
            } else {
                Some(entry_peer_eligibility(decision.eligibility))
            }
        })
}

fn accepted_exclude_matches(exclude: &AcceptedExclude, relative_path: &str) -> bool {
    match exclude.kind {
        AcceptedExcludeKind::File => exclude.relative_path == relative_path,
        AcceptedExcludeKind::DirectorySubtree => {
            exclude.relative_path == relative_path
                || relative_path
                    .strip_prefix(exclude.relative_path.as_str())
                    .is_some_and(|suffix| suffix.starts_with('/'))
        }
    }
}

fn excluded_filter_entry_kind(entry_kind: &LiveEntryKind) -> excluded_filter::LiveEntryKind {
    match entry_kind {
        LiveEntryKind::File { .. } => excluded_filter::LiveEntryKind::RegularFile,
        LiveEntryKind::Directory => excluded_filter::LiveEntryKind::Directory,
        LiveEntryKind::SymbolicLinkFile => excluded_filter::LiveEntryKind::SymbolicLinkFile,
        LiveEntryKind::SymbolicLinkDirectory => {
            excluded_filter::LiveEntryKind::SymbolicLinkDirectory
        }
        LiveEntryKind::Special => excluded_filter::LiveEntryKind::SpecialFile,
    }
}

fn entry_peer_eligibility(eligibility: excluded_filter::PathEligibility) -> EntryPeerEligibility {
    EntryPeerEligibility {
        snapshot_lookup: eligibility.snapshot_lookup,
        snapshot_update: eligibility.snapshot_update,
        file_mutation: eligibility.copy || eligibility.delete || eligibility.displace,
        directory_mutation: eligibility.scan || eligibility.recursion,
        copy: eligibility.copy,
        deletion: eligibility.delete,
        displacement: eligibility.displace,
    }
}

fn live_walk_entry_fact(parent_path: &str, relative_path: &str) -> live_walk::LiveDirectoryEntryFact {
    live_walk::LiveDirectoryEntryFact {
        relative_directory_path: parent_path.to_string(),
        entry_name: child_name(parent_path, relative_path),
        peer_entries: Vec::new(),
        peer_eligibility: Vec::new(),
    }
}

fn live_walk_parent_decision(
    peer_decisions: Vec<ChildDirectoryPeerDecision>,
) -> live_walk::LiveDirectoryParentEntryDecision {
    if peer_decisions.iter().all(|decision| {
        decision.disposition == ChildDirectoryDisposition::NotAChildDirectory
    }) {
        return live_walk::LiveDirectoryParentEntryDecision::NotChildDirectory;
    }

    live_walk::LiveDirectoryParentEntryDecision::ChildDirectory {
        peer_decisions: peer_decisions
            .into_iter()
            .map(|decision| live_walk::LiveDirectoryPeerChildDirectoryDecision {
                peer_id: decision.peer_id,
                outcome: live_walk_peer_outcome(decision.disposition),
            })
            .collect(),
    }
}

fn live_walk_peer_outcome(
    disposition: ChildDirectoryDisposition,
) -> live_walk::LiveDirectoryPeerChildDirectoryOutcome {
    match disposition {
        ChildDirectoryDisposition::KeepsDirectory => {
            live_walk::LiveDirectoryPeerChildDirectoryOutcome::KeepDirectory
        }
        ChildDirectoryDisposition::CreatesDirectory => {
            live_walk::LiveDirectoryPeerChildDirectoryOutcome::CreateDirectory
        }
        ChildDirectoryDisposition::DirectoryAbsent => {
            live_walk::LiveDirectoryPeerChildDirectoryOutcome::NoDirectory
        }
        ChildDirectoryDisposition::DisplacesDirectory => {
            live_walk::LiveDirectoryPeerChildDirectoryOutcome::DisplaceDirectory
        }
        ChildDirectoryDisposition::NotAChildDirectory => {
            live_walk::LiveDirectoryPeerChildDirectoryOutcome::NoDirectory
        }
    }
}

fn child_name(parent_path: &str, relative_path: &str) -> String {
    if parent_path.is_empty() {
        return relative_path.to_string();
    }

    relative_path
        .strip_prefix(parent_path)
        .and_then(|suffix| suffix.strip_prefix('/'))
        .unwrap_or(relative_path)
        .to_string()
}

fn join_relative_path(parent: &str, child: &str) -> String {
    if parent.is_empty() {
        child.to_string()
    } else {
        format!("{parent}/{child}")
    }
}

pub fn new(
    excludedpathfilter: Arc<dyn excluded_filter::ExcludedPathFilter>,
    livedirectorywalk: Arc<dyn live_walk::LiveDirectoryWalk>,
) -> Arc<dyn TreeTraversal> {
    Arc::new(TreeTraversalImpl {
        excluded_path_filter: excludedpathfilter,
        live_directory_walk: livedirectorywalk,
    })
}
