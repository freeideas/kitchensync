use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::thread;

use crate::operations::OperationExecutor;
use crate::snapshot::fresh_timestamp;
use crate::{
    EffectivePeerRole, EntryMeta, PeerId, PeerSession, ProgressEvent, ProgressSink, RelPath,
    RunConfig, TransportError,
};

use super::{SkippedSubtree, SkippedSubtreeReason, SyncFailure, TraversalReport};

pub(super) fn traverse(
    input: TraversalInput<'_>,
    visitor: &mut dyn CandidateVisitor,
) -> TraversalOutput {
    let mut state = TraversalState::new(input);
    let active = state
        .peers
        .iter()
        .map(|peer| peer.peer_index)
        .collect::<Vec<_>>();
    state.walk_directory(root_path(), active, visitor);
    state.finish()
}

pub(super) struct TraversalInput<'a> {
    pub config: &'a RunConfig,
    pub peers: &'a [TraversalPeer<'a>],
    pub operations: &'a dyn OperationExecutor,
    pub excludes: &'a dyn TraversalExcludes,
    pub progress: &'a dyn ProgressSink,
}

#[derive(Clone, Copy)]
pub(super) struct TraversalPeer<'a> {
    pub peer_index: usize,
    pub session: &'a PeerSession,
}

pub(super) trait TraversalExcludes {
    fn excludes_candidate(&self, path: &RelPath, metadata: Option<&EntryMeta>) -> bool;
}

pub(super) trait CandidateVisitor {
    fn visit_candidate(&mut self, candidate: CandidateVisit<'_>) -> CandidateVisitReport;
}

pub(super) struct CandidateVisit<'a> {
    pub path: RelPath,
    pub basename: String,
    pub peers: Vec<CandidatePeer<'a>>,
}

pub(super) struct CandidatePeer<'a> {
    pub peer_index: usize,
    pub session: &'a PeerSession,
    pub live: Option<EntryMeta>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct CandidateVisitReport {
    pub child_recursion_peers: Vec<PeerId>,
    pub decided_entries: u64,
    pub enqueued_copies: u64,
    pub failures: Vec<SyncFailure>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct TraversalOutput {
    pub report: TraversalReport,
    pub skipped: Vec<SkippedSubtree>,
    pub failures: Vec<SyncFailure>,
}

struct TraversalState<'a> {
    config: &'a RunConfig,
    peers: &'a [TraversalPeer<'a>],
    operations: &'a dyn OperationExecutor,
    excludes: &'a dyn TraversalExcludes,
    progress: &'a dyn ProgressSink,
    report: TraversalReport,
    skipped: Vec<SkippedSubtree>,
    failures: Vec<SyncFailure>,
}

impl<'a> TraversalState<'a> {
    fn new(input: TraversalInput<'a>) -> Self {
        Self {
            config: input.config,
            peers: input.peers,
            operations: input.operations,
            excludes: input.excludes,
            progress: input.progress,
            report: TraversalReport::default(),
            skipped: Vec::new(),
            failures: Vec::new(),
        }
    }

    fn walk_directory(
        &mut self,
        directory: RelPath,
        active_peer_indices: Vec<usize>,
        visitor: &mut dyn CandidateVisitor,
    ) {
        self.report.scanned_directories += 1;
        self.progress.publish(ProgressEvent::Scanning {
            directory: directory.clone(),
        });

        let listing = self.prepare_and_list(&directory, &active_peer_indices);
        if listing.skip_subtree {
            return;
        }

        if !listing.has_contributing_peer(self.peers) {
            self.skipped.push(SkippedSubtree {
                directory,
                reason: SkippedSubtreeReason::NoContributingPeerListed,
            });
            return;
        }

        let names = listing.candidate_names(self.peers);
        let mut child_recursions = Vec::new();
        for name in names {
            let path = join_path(&directory, &name);
            if self.excluded(&path, &listing) {
                continue;
            }

            let visit = self.candidate_visit(path.clone(), name, &listing);
            let visit_report = visitor.visit_candidate(visit);
            self.report.decided_entries += visit_report.decided_entries;
            self.report.enqueued_copies += visit_report.enqueued_copies;
            self.failures.extend(visit_report.failures);

            let child_active = self.child_active_peers(
                &visit_report.child_recursion_peers,
                &listing,
            );
            if !child_active.is_empty() {
                child_recursions.push((path, child_active));
            }
        }

        for (child, child_active) in child_recursions {
            self.walk_directory(child, child_active, visitor);
        }

        if !self.config.dry_run {
            self.cleanup_directory(&directory, &listing);
        }
    }

    fn prepare_and_list(
        &mut self,
        directory: &RelPath,
        active_peer_indices: &[usize],
    ) -> DirectoryListing {
        let mut ready = Vec::new();
        for &peer_index in active_peer_indices {
            match self.recover_before_listing(directory, peer_index) {
                PeerPreparation::Ready => ready.push(peer_index),
                PeerPreparation::FailedNonCanon => {}
                PeerPreparation::FailedCanon(peer_id) => {
                    self.skipped.push(SkippedSubtree {
                        directory: directory.clone(),
                        reason: SkippedSubtreeReason::CanonListingUnavailable { peer_id },
                    });
                    return DirectoryListing {
                        skip_subtree: true,
                        ..DirectoryListing::default()
                    };
                }
            }
        }

        let mut listing = DirectoryListing::default();
        for result in self.list_ready_peers(directory, &ready) {
            match result.entries {
                Ok(entries) => listing.peers.push(ListedPeer {
                    peer_index: result.peer_index,
                    entries: entries
                        .into_iter()
                        .map(|entry| (entry.name.clone(), entry))
                        .collect(),
                }),
                Err(error) => {
                    let peer = self.peer(result.peer_index);
                    let canon = peer.session.effective_role == EffectivePeerRole::Canon;
                    self.failures.push(SyncFailure::Listing {
                        peer_id: peer.session.id,
                        directory: directory.clone(),
                        attempts: result.attempts,
                        canon,
                        error,
                    });
                    if canon {
                        self.skipped.push(SkippedSubtree {
                            directory: directory.clone(),
                            reason: SkippedSubtreeReason::CanonListingUnavailable {
                                peer_id: peer.session.id,
                            },
                        });
                        listing.peers.clear();
                        listing.skip_subtree = true;
                        return listing;
                    }
                }
            }
        }

        listing
    }

    fn recover_before_listing(
        &mut self,
        directory: &RelPath,
        peer_index: usize,
    ) -> PeerPreparation {
        if self.config.dry_run {
            return PeerPreparation::Ready;
        }

        let attempts = self.config.retries_list.max(1);
        let peer = self.peer(peer_index);
        let canon = peer.session.effective_role == EffectivePeerRole::Canon;

        for attempt in 1..=attempts {
            match self
                .operations
                .recover_directory_swaps(peer.session, directory)
            {
                Ok(_) => return PeerPreparation::Ready,
                Err(error) if attempt == attempts => {
                    let peer_id = peer.session.id;
                    self.failures.push(SyncFailure::SwapRecovery {
                        peer_id,
                        directory: directory.clone(),
                        attempts,
                        canon,
                        error,
                    });
                    return if canon {
                        PeerPreparation::FailedCanon(peer_id)
                    } else {
                        PeerPreparation::FailedNonCanon
                    };
                }
                Err(_) => {}
            }
        }

        PeerPreparation::FailedNonCanon
    }

    fn list_ready_peers(
        &self,
        directory: &RelPath,
        peer_indices: &[usize],
    ) -> Vec<ListingResult> {
        let attempts = self.config.retries_list.max(1);
        thread::scope(|scope| {
            let handles = peer_indices
                .iter()
                .copied()
                .map(|peer_index| {
                    let session = self.peer(peer_index).session;
                    scope.spawn(move || {
                        let mut last_error = TransportError::IoError;
                        for attempt in 1..=attempts {
                            match session.transport.list_dir(directory) {
                                Ok(entries) => {
                                    return ListingResult {
                                        peer_index,
                                        attempts: attempt,
                                        entries: Ok(entries),
                                    };
                                }
                                Err(error) => last_error = error,
                            }
                        }

                        ListingResult {
                            peer_index,
                            attempts,
                            entries: Err(last_error),
                        }
                    })
                })
                .collect::<Vec<_>>();

            let mut results = handles
                .into_iter()
                .map(|handle| handle.join().expect("directory listing thread panicked"))
                .collect::<Vec<_>>();
            results.sort_by_key(|result| result.peer_index);
            results
        })
    }

    fn candidate_visit(
        &self,
        path: RelPath,
        basename: String,
        listing: &DirectoryListing,
    ) -> CandidateVisit<'a> {
        let peers = listing
            .peers
            .iter()
            .map(|listed| {
                let peer = self.peer(listed.peer_index);
                CandidatePeer {
                    peer_index: listed.peer_index,
                    session: peer.session,
                    live: listed.entries.get(&basename).cloned(),
                }
            })
            .collect();

        CandidateVisit {
            path,
            basename,
            peers,
        }
    }

    fn child_active_peers(
        &self,
        requested_peer_ids: &[PeerId],
        listing: &DirectoryListing,
    ) -> Vec<usize> {
        let requested = requested_peer_ids.iter().copied().collect::<HashSet<_>>();
        listing
            .peers
            .iter()
            .filter_map(|listed| {
                let peer = self.peer(listed.peer_index);
                requested.contains(&peer.session.id).then_some(listed.peer_index)
            })
            .collect()
    }

    fn cleanup_directory(&mut self, directory: &RelPath, listing: &DirectoryListing) {
        for listed in &listing.peers {
            let peer = self.peer(listed.peer_index);
            if let Err(error) = self.operations.cleanup_retention(
                peer.session,
                directory,
                fresh_timestamp(),
                self.config.keep_bak_days,
                self.config.keep_tmp_days,
            ) {
                self.failures.push(SyncFailure::Operation {
                    peer_id: peer.session.id,
                    path: directory.clone(),
                    error,
                });
            }
        }
    }

    fn excluded(&self, path: &RelPath, listing: &DirectoryListing) -> bool {
        if self.excludes.excludes_candidate(path, None) {
            return true;
        }

        listing
            .metadata_for_path(path)
            .any(|metadata| self.excludes.excludes_candidate(path, Some(metadata)))
    }

    fn peer(&self, peer_index: usize) -> TraversalPeer<'a> {
        self.peers
            .iter()
            .copied()
            .find(|peer| peer.peer_index == peer_index)
            .expect("active peer index must belong to traversal input")
    }

    fn finish(self) -> TraversalOutput {
        TraversalOutput {
            report: self.report,
            skipped: self.skipped,
            failures: self.failures,
        }
    }
}

#[derive(Default)]
struct DirectoryListing {
    peers: Vec<ListedPeer>,
    skip_subtree: bool,
}

impl DirectoryListing {
    fn has_contributing_peer(&self, peers: &[TraversalPeer<'_>]) -> bool {
        self.peers.iter().any(|listed| {
            peers
                .iter()
                .find(|peer| peer.peer_index == listed.peer_index)
                .is_some_and(|peer| contributes(peer.session.effective_role))
        })
    }

    fn candidate_names(&self, peers: &[TraversalPeer<'_>]) -> Vec<String> {
        let has_contributing = self.has_contributing_peer(peers);
        let mut names = BTreeSet::new();

        for listed in &self.peers {
            let role = peers
                .iter()
                .find(|peer| peer.peer_index == listed.peer_index)
                .map(|peer| peer.session.effective_role)
                .unwrap_or(EffectivePeerRole::Subordinate);
            if contributes(role) || (has_contributing && role == EffectivePeerRole::Subordinate) {
                names.extend(listed.entries.keys().cloned().map(SortName));
            }
        }

        names.into_iter().map(|name| name.0).collect()
    }

    fn metadata_for_path<'a>(&'a self, path: &RelPath) -> impl Iterator<Item = &'a EntryMeta> {
        let name = basename(path);
        self.peers
            .iter()
            .filter_map(move |listed| listed.entries.get(name))
    }
}

struct ListedPeer {
    peer_index: usize,
    entries: BTreeMap<String, EntryMeta>,
}

enum PeerPreparation {
    Ready,
    FailedCanon(PeerId),
    FailedNonCanon,
}

struct ListingResult {
    peer_index: usize,
    attempts: usize,
    entries: Result<Vec<EntryMeta>, TransportError>,
}

#[derive(Debug, Clone, Eq)]
struct SortName(String);

impl PartialEq for SortName {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl PartialOrd for SortName {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SortName {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0
            .to_ascii_lowercase()
            .cmp(&other.0.to_ascii_lowercase())
            .then_with(|| self.0.cmp(&other.0))
    }
}

fn contributes(role: EffectivePeerRole) -> bool {
    matches!(
        role,
        EffectivePeerRole::Canon | EffectivePeerRole::Contributing
    )
}

fn root_path() -> RelPath {
    RelPath::new("").expect("root relative path must be valid")
}

fn join_path(parent: &RelPath, child: &str) -> RelPath {
    let value = if parent.as_str().is_empty() {
        child.to_string()
    } else {
        format!("{}/{}", parent.as_str(), child)
    };
    RelPath::new(value).expect("transport child name must form a valid relative path")
}

fn basename(path: &RelPath) -> &str {
    path.as_str()
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(path.as_str())
}
