use std::collections::{BTreeSet, HashMap};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use crate::operations::{OperationError, OperationExecutor};
use crate::runtime::{
    CopyAttemptFailure, CopyAttemptOutcome, CopyOperation, CopyScheduler, SchedulerSummary,
};
use crate::snapshot::{fresh_timestamp, SnapshotEntryKind, SnapshotRow, SnapshotStore};
use crate::{
    CopyResult, CopyTask, DiagnosticEvent, DiagnosticSink, EffectivePeerRole, EntryKind,
    EntryMeta, PeerId, PeerSession, ProgressEvent, ProgressSink, RelPath, RunConfig, Timestamp,
    TransferPhase, TransportError,
};

const SUMMARY: &str =
    "sync: combined-tree traversal, reconciliation decisions, copy planning, and snapshot updates.";
const MODIFY_TOLERANCE: Duration = Duration::from_secs(5);

pub fn summary() -> &'static str {
    SUMMARY
}

pub fn run(run: SyncRun<'_>) -> SyncReport {
    let mut context = RunContext::new(run);
    if !context.validate_inputs() {
        return context.finish(SchedulerSummary::default());
    }

    let active = (0..context.peers.len()).collect::<Vec<_>>();
    let root = root_path();
    context.walk_directory(root, active);
    context.run.copy_scheduler.close();

    let operation = SchedulerCopyOperation::new(
        context.run.operations,
        context
            .run
            .peers
            .iter()
            .map(|peer| peer.session)
            .collect::<Vec<_>>(),
    );
    let copies = context.run.copy_scheduler.run_until_complete(&operation);
    context.consume_copy_results(operation.into_results());
    context.finish(copies)
}

pub struct SyncRun<'a> {
    pub config: &'a RunConfig,
    pub peers: &'a mut [SyncPeer<'a>],
    pub operations: &'a dyn OperationExecutor,
    pub copy_scheduler: &'a CopyScheduler,
    pub diagnostics: &'a dyn DiagnosticSink,
    pub progress: &'a dyn ProgressSink,
}

pub struct SyncPeer<'a> {
    pub session: &'a PeerSession,
    pub snapshot: &'a mut SnapshotStore,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncReport {
    pub completed: bool,
    pub traversal: TraversalReport,
    pub copies: SchedulerSummary,
    pub skipped: Vec<SkippedSubtree>,
    pub failures: Vec<SyncFailure>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TraversalReport {
    pub scanned_directories: u64,
    pub decided_entries: u64,
    pub enqueued_copies: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedSubtree {
    pub directory: RelPath,
    pub reason: SkippedSubtreeReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkippedSubtreeReason {
    CanonListingUnavailable { peer_id: PeerId },
    NoContributingPeerListed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncFailure {
    Listing {
        peer_id: PeerId,
        directory: RelPath,
        attempts: usize,
        canon: bool,
        error: TransportError,
    },
    SwapRecovery {
        peer_id: PeerId,
        directory: RelPath,
        attempts: usize,
        canon: bool,
        error: OperationError,
    },
    Operation {
        peer_id: PeerId,
        path: RelPath,
        error: OperationError,
    },
    Copy {
        result: CopyResult,
    },
    InvalidRunInput {
        reason: SyncInputError,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncInputError {
    EmptyPeerSet,
    MissingSnapshotStore { peer_id: PeerId },
    SnapshotPeerMismatch { peer_id: PeerId },
    NoContributingPeer,
    MoreThanOneCanonPeer,
}

struct RunContext<'a> {
    run: SyncRun<'a>,
    peers: Vec<PeerSlot>,
    traversal: TraversalReport,
    skipped: Vec<SkippedSubtree>,
    failures: Vec<SyncFailure>,
    snapshot_failed: bool,
}

#[derive(Debug, Clone)]
struct PeerSlot {
    id: PeerId,
    role: EffectivePeerRole,
}

impl<'a> RunContext<'a> {
    fn new(run: SyncRun<'a>) -> Self {
        let peers = run
            .peers
            .iter()
            .map(|peer| PeerSlot {
                id: peer.session.id,
                role: peer.session.effective_role,
            })
            .collect();
        Self {
            run,
            peers,
            traversal: TraversalReport::default(),
            skipped: Vec::new(),
            failures: Vec::new(),
            snapshot_failed: false,
        }
    }

    fn validate_inputs(&mut self) -> bool {
        if self.peers.is_empty() {
            self.fail_input(SyncInputError::EmptyPeerSet);
            return false;
        }

        let mut canon_count = 0;
        let mut contributing_count = 0;
        for peer in self.run.peers.iter() {
            if peer.snapshot.peer() != peer.session.id {
                self.failures.push(SyncFailure::InvalidRunInput {
                    reason: SyncInputError::SnapshotPeerMismatch {
                        peer_id: peer.session.id,
                    },
                });
            }
            match peer.session.effective_role {
                EffectivePeerRole::Canon => {
                    canon_count += 1;
                    contributing_count += 1;
                }
                EffectivePeerRole::Contributing => contributing_count += 1,
                EffectivePeerRole::Subordinate => {}
            }
        }

        if canon_count > 1 {
            self.fail_input(SyncInputError::MoreThanOneCanonPeer);
        }
        if contributing_count == 0 {
            self.fail_input(SyncInputError::NoContributingPeer);
        }
        !self
            .failures
            .iter()
            .any(|failure| matches!(failure, SyncFailure::InvalidRunInput { .. }))
    }

    fn fail_input(&mut self, reason: SyncInputError) {
        self.failures.push(SyncFailure::InvalidRunInput { reason });
    }

    fn walk_directory(&mut self, directory: RelPath, active_peers: Vec<usize>) {
        self.traversal.scanned_directories += 1;
        self.run.progress.publish(ProgressEvent::Scanning {
            directory: directory.clone(),
        });

        let listed = self.prepare_and_list(&directory, &active_peers);
        if listed.skip_subtree {
            return;
        }

        let listed = listed.listed;
        if !has_contributing_peer(&listed, &self.peers) {
            self.skipped.push(SkippedSubtree {
                directory,
                reason: SkippedSubtreeReason::NoContributingPeerListed,
            });
            return;
        }

        let names = candidate_names(&listed, &self.peers);
        let mut child_recursions = Vec::new();
        for name in names {
            let path = join_path(&directory, &name);
            if self.is_excluded(&path) {
                continue;
            }

            let candidate = self.classify_candidate(path.clone(), &listed);
            let decision = choose_decision(&candidate);
            self.traversal.decided_entries += 1;
            match decision {
                Decision::File { source } => self.apply_file_decision(&candidate, source),
                Decision::Directory => {
                    let child_active = self.apply_directory_decision(&candidate);
                    if !child_active.is_empty() {
                        child_recursions.push((path, child_active));
                    }
                }
                Decision::Absence => self.apply_absence_decision(&candidate),
            }
        }

        if !self.run.config.dry_run {
            self.cleanup_directory(&directory, &listed);
        }

        for (child, child_active) in child_recursions {
            self.walk_directory(child, child_active);
        }
    }

    fn prepare_and_list(&mut self, directory: &RelPath, active: &[usize]) -> DirectoryListing {
        let mut listing = DirectoryListing::default();
        for &peer_index in active {
            match self.prepare_one_peer(directory, peer_index) {
                PeerDirectoryState::Listed(entries) => {
                    listing.listed.push(ListedPeer {
                        peer_index,
                        entries: entries
                            .into_iter()
                            .filter(|entry| {
                                matches!(entry.kind, EntryKind::File | EntryKind::Directory)
                            })
                            .collect(),
                    });
                }
                PeerDirectoryState::FailedCanon(peer_id) => {
                    self.skipped.push(SkippedSubtree {
                        directory: directory.clone(),
                        reason: SkippedSubtreeReason::CanonListingUnavailable { peer_id },
                    });
                    listing.skip_subtree = true;
                    return listing;
                }
                PeerDirectoryState::FailedNonCanon => {}
            }
        }
        listing
    }

    fn prepare_one_peer(&mut self, directory: &RelPath, peer_index: usize) -> PeerDirectoryState {
        let peer = self.run.peers[peer_index].session;
        let canon = self.peers[peer_index].role == EffectivePeerRole::Canon;
        let attempts = self.run.config.retries_list.max(1);

        for attempt in 1..=attempts {
            if !self.run.config.dry_run {
                if let Err(error) = self.run.operations.recover_directory_swaps(peer, directory) {
                    if attempt == attempts {
                        self.publish_error(format!(
                            "failed SWAP recovery peer={} directory={} attempts={}",
                            peer.id,
                            render_path(directory),
                            attempts
                        ));
                        self.failures.push(SyncFailure::SwapRecovery {
                            peer_id: peer.id,
                            directory: directory.clone(),
                            attempts,
                            canon,
                            error: error.clone(),
                        });
                        return if canon {
                            PeerDirectoryState::FailedCanon(peer.id)
                        } else {
                            PeerDirectoryState::FailedNonCanon
                        };
                    }
                    continue;
                }
            }

            match peer.transport.list_dir(directory) {
                Ok(entries) => return PeerDirectoryState::Listed(entries),
                Err(error) if attempt == attempts => {
                    self.publish_error(format!(
                        "failed listing peer={} directory={} attempts={}",
                        peer.id,
                        render_path(directory),
                        attempts
                    ));
                    self.failures.push(SyncFailure::Listing {
                        peer_id: peer.id,
                        directory: directory.clone(),
                        attempts,
                        canon,
                        error: error.clone(),
                    });
                    return if canon {
                        PeerDirectoryState::FailedCanon(peer.id)
                    } else {
                        PeerDirectoryState::FailedNonCanon
                    };
                }
                Err(_) => {}
            }
        }

        PeerDirectoryState::FailedNonCanon
    }

    fn classify_candidate(&mut self, path: RelPath, listed: &[ListedPeer]) -> Candidate {
        let mut states = Vec::new();
        for listed_peer in listed {
            let peer_id = self.peers[listed_peer.peer_index].id;
            let role = self.peers[listed_peer.peer_index].role;
            let live = listed_peer
                .entries
                .iter()
                .find(|entry| join_path(&parent_path(&path), &entry.name) == path)
                .cloned();
            let snapshot = if contributes(role) {
                match self.run.peers[listed_peer.peer_index].snapshot.lookup(&path) {
                    Ok(row) => row,
                    Err(error) => {
                        self.snapshot_failed = true;
                        self.publish_error(format!(
                            "snapshot lookup failed peer={} path={}: {:?}",
                            peer_id, path, error
                        ));
                        None
                    }
                }
            } else {
                None
            };
            states.push(PeerState {
                peer_index: listed_peer.peer_index,
                role,
                live,
                snapshot,
            });
        }
        Candidate { path, states }
    }

    fn apply_file_decision(&mut self, candidate: &Candidate, source_index: usize) {
        let source_state = &candidate.states[source_index];
        let Some(winner_meta) = source_state.live.as_ref().cloned() else {
            return;
        };
        let source_peer_index = source_state.peer_index;
        self.upsert_confirmed_present(source_peer_index, &candidate.path, &winner_meta);

        for state in &candidate.states {
            match state.live.as_ref().map(|meta| meta.kind) {
                Some(EntryKind::Directory) => {
                    if !self.displace(
                        state.peer_index,
                        &candidate.path,
                        SnapshotEntryKind::Directory,
                    )
                    {
                        continue;
                    }
                }
                Some(EntryKind::File) => {
                    let live = state.live.as_ref().expect("live file metadata exists");
                    self.upsert_confirmed_present(state.peer_index, &candidate.path, live);
                    if file_matches(live, &winner_meta) {
                        continue;
                    }
                }
                _ => {}
            }

            if state.peer_index == source_peer_index {
                continue;
            }

            self.upsert_intended_copy(state.peer_index, &candidate.path, &winner_meta);
            self.run.copy_scheduler.submit(CopyTask {
                source_peer_id: self.peers[source_peer_index].id,
                source_path: candidate.path.clone(),
                destination_peer_id: self.peers[state.peer_index].id,
                destination_path: candidate.path.clone(),
                winning_meta: winner_meta.clone(),
            });
            self.traversal.enqueued_copies += 1;
        }
    }

    fn apply_directory_decision(&mut self, candidate: &Candidate) -> Vec<usize> {
        let mut recurse = Vec::new();
        let directory_meta = EntryMeta {
            name: basename(&candidate.path).to_string(),
            kind: EntryKind::Directory,
            mod_time: fresh_timestamp(),
            byte_size: -1,
        };

        for state in &candidate.states {
            match state.live.as_ref().map(|meta| meta.kind) {
                Some(EntryKind::Directory) => {
                    let live = state.live.as_ref().expect("live directory metadata exists");
                    self.upsert_confirmed_present(state.peer_index, &candidate.path, live);
                    recurse.push(state.peer_index);
                }
                Some(EntryKind::File) => {
                    if self.displace(state.peer_index, &candidate.path, SnapshotEntryKind::File)
                        && self.create_directory(state.peer_index, &candidate.path, &directory_meta)
                    {
                        recurse.push(state.peer_index);
                    }
                }
                None => {
                    if self.create_directory(state.peer_index, &candidate.path, &directory_meta) {
                        recurse.push(state.peer_index);
                    }
                }
            }
        }
        recurse
    }

    fn apply_absence_decision(&mut self, candidate: &Candidate) {
        for state in &candidate.states {
            match state.live.as_ref().map(|meta| meta.kind) {
                Some(EntryKind::File) => {
                    self.displace(state.peer_index, &candidate.path, SnapshotEntryKind::File);
                }
                Some(EntryKind::Directory) => {
                    self.displace(state.peer_index, &candidate.path, SnapshotEntryKind::Directory);
                }
                None => self.mark_absent(state.peer_index, &candidate.path),
            }
        }
    }

    fn displace(&mut self, peer_index: usize, path: &RelPath, kind: SnapshotEntryKind) -> bool {
        let peer = self.run.peers[peer_index].session;
        match self
            .run
            .operations
            .displace_to_bak(peer, path, fresh_timestamp())
        {
            Ok(_) => {
                self.mark_displaced(peer_index, path, kind);
                true
            }
            Err(error) => {
                self.publish_operation_failure(peer.id, path, &error);
                self.failures.push(SyncFailure::Operation {
                    peer_id: peer.id,
                    path: path.clone(),
                    error,
                });
                false
            }
        }
    }

    fn create_directory(
        &mut self,
        peer_index: usize,
        path: &RelPath,
        meta: &EntryMeta,
    ) -> bool {
        let peer = self.run.peers[peer_index].session;
        match self.run.operations.create_directory(peer, path) {
            Ok(_) => {
                self.upsert_confirmed_present(peer_index, path, meta);
                true
            }
            Err(error) => {
                self.publish_operation_failure(peer.id, path, &error);
                self.failures.push(SyncFailure::Operation {
                    peer_id: peer.id,
                    path: path.clone(),
                    error,
                });
                false
            }
        }
    }

    fn cleanup_directory(&mut self, directory: &RelPath, listed: &[ListedPeer]) {
        for listed_peer in listed {
            let peer = self.run.peers[listed_peer.peer_index].session;
            if let Err(error) = self.run.operations.cleanup_retention(
                peer,
                directory,
                fresh_timestamp(),
                self.run.config.keep_bak_days,
                self.run.config.keep_tmp_days,
            ) {
                self.publish_operation_failure(peer.id, directory, &error);
                self.failures.push(SyncFailure::Operation {
                    peer_id: peer.id,
                    path: directory.clone(),
                    error,
                });
            }
        }
    }

    fn consume_copy_results(&mut self, results: CopyResults) {
        for result in results.successes {
            if let Some(peer_index) = self
                .peers
                .iter()
                .position(|peer| peer.id == result.destination_peer_id)
            {
                self.mark_copy_complete(peer_index, &result.destination_path);
            }
        }

        for result in results.terminal_failures.into_values() {
            self.failures.push(SyncFailure::Copy { result });
        }
    }

    fn finish(self, copies: SchedulerSummary) -> SyncReport {
        let completed = !self.snapshot_failed
            && self.skipped.is_empty()
            && self.failures.is_empty()
            && copies.failed == 0;
        SyncReport {
            completed,
            traversal: self.traversal,
            copies,
            skipped: self.skipped,
            failures: self.failures,
        }
    }

    fn is_excluded(&self, path: &RelPath) -> bool {
        let value = path.as_str();
        if value == ".kitchensync" || value.starts_with(".kitchensync/") {
            return true;
        }
        if value == ".git" || value.starts_with(".git/") {
            return true;
        }

        self.run.config.excludes.iter().any(|excluded| {
            value == excluded.as_str()
                || value
                    .strip_prefix(excluded.as_str())
                    .is_some_and(|rest| rest.starts_with('/'))
        })
    }

    fn upsert_confirmed_present(&mut self, peer_index: usize, path: &RelPath, meta: &EntryMeta) {
        if let Err(error) = self.run.peers[peer_index]
            .snapshot
            .upsert_confirmed_present(path, meta)
        {
            self.snapshot_failed = true;
            self.publish_error(format!(
                "snapshot present update failed peer={} path={}: {:?}",
                self.peers[peer_index].id, path, error
            ));
        }
    }

    fn upsert_intended_copy(&mut self, peer_index: usize, path: &RelPath, meta: &EntryMeta) {
        if let Err(error) = self.run.peers[peer_index]
            .snapshot
            .upsert_intended_copy(path, meta)
        {
            self.snapshot_failed = true;
            self.publish_error(format!(
                "snapshot copy-intent update failed peer={} path={}: {:?}",
                self.peers[peer_index].id, path, error
            ));
        }
    }

    fn mark_copy_complete(&mut self, peer_index: usize, path: &RelPath) {
        if let Err(error) = self.run.peers[peer_index].snapshot.mark_copy_complete(path) {
            self.snapshot_failed = true;
            self.publish_error(format!(
                "snapshot copy-complete update failed peer={} path={}: {:?}",
                self.peers[peer_index].id, path, error
            ));
        }
    }

    fn mark_absent(&mut self, peer_index: usize, path: &RelPath) {
        if let Err(error) = self.run.peers[peer_index].snapshot.mark_absent(path) {
            self.snapshot_failed = true;
            self.publish_error(format!(
                "snapshot absent update failed peer={} path={}: {:?}",
                self.peers[peer_index].id, path, error
            ));
        }
    }

    fn mark_displaced(&mut self, peer_index: usize, path: &RelPath, kind: SnapshotEntryKind) {
        if let Err(error) = self.run.peers[peer_index].snapshot.mark_displaced(path, kind) {
            self.snapshot_failed = true;
            self.publish_error(format!(
                "snapshot displaced update failed peer={} path={}: {:?}",
                self.peers[peer_index].id, path, error
            ));
        }
    }

    fn publish_operation_failure(&self, peer_id: PeerId, path: &RelPath, error: &OperationError) {
        self.publish_error(format!(
            "operation failed peer={} path={}: {:?}",
            peer_id, path, error
        ));
    }

    fn publish_error(&self, message: String) {
        self.run
            .diagnostics
            .publish(DiagnosticEvent::Error { message });
    }
}

#[derive(Default)]
struct DirectoryListing {
    listed: Vec<ListedPeer>,
    skip_subtree: bool,
}

struct ListedPeer {
    peer_index: usize,
    entries: Vec<EntryMeta>,
}

enum PeerDirectoryState {
    Listed(Vec<EntryMeta>),
    FailedCanon(PeerId),
    FailedNonCanon,
}

#[derive(Clone)]
struct Candidate {
    path: RelPath,
    states: Vec<PeerState>,
}

#[derive(Clone)]
struct PeerState {
    peer_index: usize,
    role: EffectivePeerRole,
    live: Option<EntryMeta>,
    snapshot: Option<SnapshotRow>,
}

enum Decision {
    File { source: usize },
    Directory,
    Absence,
}

fn choose_decision(candidate: &Candidate) -> Decision {
    if let Some((index, state)) = candidate
        .states
        .iter()
        .enumerate()
        .find(|(_, state)| state.role == EffectivePeerRole::Canon)
    {
        return match state.live.as_ref().map(|meta| meta.kind) {
            Some(EntryKind::File) => Decision::File { source: index },
            Some(EntryKind::Directory) => Decision::Directory,
            None => Decision::Absence,
        };
    }

    let contributing = candidate
        .states
        .iter()
        .enumerate()
        .filter(|(_, state)| contributes(state.role))
        .collect::<Vec<_>>();

    let file_candidates = contributing
        .iter()
        .copied()
        .filter(|(_, state)| state.live.as_ref().is_some_and(|meta| meta.kind == EntryKind::File))
        .collect::<Vec<_>>();

    let directory_exists = contributing
        .iter()
        .any(|(_, state)| state.live.as_ref().is_some_and(|meta| meta.kind == EntryKind::Directory));

    if !file_candidates.is_empty() {
        let winner = choose_file_winner(&file_candidates);
        let newest_file = winner
            .1
            .live
            .as_ref()
            .map(|meta| &meta.mod_time)
            .expect("file winner has live metadata");
        if deletion_wins(&contributing, newest_file) {
            Decision::Absence
        } else {
            Decision::File { source: winner.0 }
        }
    } else if directory_exists {
        Decision::Directory
    } else {
        Decision::Absence
    }
}

fn choose_file_winner<'a>(
    candidates: &'a [(usize, &'a PeerState)],
) -> (usize, &'a PeerState) {
    let mut winner = candidates[0];
    for &(index, state) in candidates.iter().skip(1) {
        let current = state.live.as_ref().expect("file candidate has live metadata");
        let best = winner
            .1
            .live
            .as_ref()
            .expect("file candidate has live metadata");

        if timestamp_more_than_tolerance_newer(&current.mod_time, &best.mod_time)
            || (!timestamp_more_than_tolerance_newer(&best.mod_time, &current.mod_time)
                && current.byte_size > best.byte_size)
        {
            winner = (index, state);
        }
    }
    winner
}

fn deletion_wins(contributing: &[(usize, &PeerState)], newest_file: &Timestamp) -> bool {
    contributing.iter().any(|(_, state)| {
        deletion_estimate(state)
            .is_some_and(|deleted| timestamp_more_than_tolerance_newer(deleted, newest_file))
    })
}

fn deletion_estimate(state: &PeerState) -> Option<&Timestamp> {
    let row = state.snapshot.as_ref()?;
    if row.deleted_time.is_some() {
        return row.deleted_time.as_ref();
    }
    if state.live.is_none() {
        return row.last_seen.as_ref();
    }
    None
}

fn candidate_names(listed: &[ListedPeer], peers: &[PeerSlot]) -> Vec<String> {
    let has_contributing = has_contributing_peer(listed, peers);
    let mut names = BTreeSet::new();
    for listed_peer in listed {
        let role = peers[listed_peer.peer_index].role;
        if contributes(role) || (has_contributing && role == EffectivePeerRole::Subordinate) {
            for entry in &listed_peer.entries {
                names.insert(SortName(entry.name.clone()));
            }
        }
    }
    names.into_iter().map(|name| name.0).collect()
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

fn has_contributing_peer(listed: &[ListedPeer], peers: &[PeerSlot]) -> bool {
    listed
        .iter()
        .any(|listed_peer| contributes(peers[listed_peer.peer_index].role))
}

fn contributes(role: EffectivePeerRole) -> bool {
    matches!(role, EffectivePeerRole::Canon | EffectivePeerRole::Contributing)
}

fn file_matches(live: &EntryMeta, winner: &EntryMeta) -> bool {
    live.kind == EntryKind::File
        && winner.kind == EntryKind::File
        && live.byte_size == winner.byte_size
        && !timestamp_more_than_tolerance_newer(&live.mod_time, &winner.mod_time)
        && !timestamp_more_than_tolerance_newer(&winner.mod_time, &live.mod_time)
}

fn timestamp_more_than_tolerance_newer(left: &Timestamp, right: &Timestamp) -> bool {
    match (parse_timestamp(left), parse_timestamp(right)) {
        (Some(left), Some(right)) => left
            .duration_since(right)
            .is_ok_and(|difference| difference > MODIFY_TOLERANCE),
        _ => left.0 > right.0,
    }
}

fn parse_timestamp(timestamp: &Timestamp) -> Option<SystemTime> {
    let (date, rest) = timestamp.0.split_once('_')?;
    let (time, micros_z) = rest.rsplit_once('_')?;
    let micros = micros_z.strip_suffix('Z')?.parse::<u32>().ok()?;

    let mut date_parts = date.split('-');
    let year = date_parts.next()?.parse::<i64>().ok()?;
    let month = date_parts.next()?.parse::<i64>().ok()?;
    let day = date_parts.next()?.parse::<i64>().ok()?;

    let mut time_parts = time.split('-');
    let hour = time_parts.next()?.parse::<i64>().ok()?;
    let minute = time_parts.next()?.parse::<i64>().ok()?;
    let second = time_parts.next()?.parse::<i64>().ok()?;

    let days = days_from_civil(year, month, day)?;
    let seconds = days
        .checked_mul(86_400)?
        .checked_add(hour.checked_mul(3_600)?)?
        .checked_add(minute.checked_mul(60)?)?
        .checked_add(second)?;
    if seconds < 0 {
        return None;
    }
    Some(SystemTime::UNIX_EPOCH + Duration::from_secs(seconds as u64) + Duration::from_micros(micros as u64))
}

fn days_from_civil(year: i64, month: i64, day: i64) -> Option<i64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let year = year - if month <= 2 { 1 } else { 0 };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month_prime = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * month_prime + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

fn root_path() -> RelPath {
    RelPath::new("").expect("RelPath root value must be supplied by the root contract")
}

fn join_path(parent: &RelPath, child: &str) -> RelPath {
    let value = if parent.as_str().is_empty() {
        child.to_string()
    } else {
        format!("{}/{}", parent.as_str(), child)
    };
    RelPath::new(value).expect("transport entry name must produce a valid relative path")
}

fn parent_path(path: &RelPath) -> RelPath {
    match path.as_str().rsplit_once('/') {
        Some((parent, _)) => RelPath::new(parent.to_string()).expect("parent path is valid"),
        None => root_path(),
    }
}

fn basename(path: &RelPath) -> &str {
    path.as_str().rsplit('/').next().unwrap_or(path.as_str())
}

fn render_path(path: &RelPath) -> &str {
    if path.as_str().is_empty() {
        "."
    } else {
        path.as_str()
    }
}

struct SchedulerCopyOperation<'a> {
    operations: &'a dyn OperationExecutor,
    peers: HashMap<PeerId, &'a PeerSession>,
    results: Mutex<CopyResults>,
}

#[derive(Default)]
struct CopyResults {
    successes: Vec<CopyResult>,
    terminal_failures: HashMap<CopyTaskKey, CopyResult>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CopyTaskKey {
    source_peer_id: PeerId,
    source_path: RelPath,
    destination_peer_id: PeerId,
    destination_path: RelPath,
}

impl CopyTaskKey {
    fn from_task(task: &CopyTask) -> Self {
        Self {
            source_peer_id: task.source_peer_id,
            source_path: task.source_path.clone(),
            destination_peer_id: task.destination_peer_id,
            destination_path: task.destination_path.clone(),
        }
    }

    fn from_result(result: &CopyResult) -> Self {
        Self {
            source_peer_id: result.source_peer_id,
            source_path: result.source_path.clone(),
            destination_peer_id: result.destination_peer_id,
            destination_path: result.destination_path.clone(),
        }
    }
}

impl<'a> SchedulerCopyOperation<'a> {
    fn new(operations: &'a dyn OperationExecutor, peers: Vec<&'a PeerSession>) -> Self {
        Self {
            operations,
            peers: peers.into_iter().map(|peer| (peer.id, peer)).collect(),
            results: Mutex::new(CopyResults::default()),
        }
    }

    fn into_results(self) -> CopyResults {
        self.results.into_inner().expect("copy result mutex poisoned")
    }
}

unsafe impl Send for SchedulerCopyOperation<'_> {}
unsafe impl Sync for SchedulerCopyOperation<'_> {}

impl CopyOperation for SchedulerCopyOperation<'_> {
    fn execute_copy_attempt(
        &self,
        task: &CopyTask,
        _progress: &dyn crate::runtime::ProgressSink,
    ) -> CopyAttemptOutcome {
        let Some(source_peer) = self.peers.get(&task.source_peer_id).copied() else {
            return CopyAttemptOutcome::Failure(CopyAttemptFailure {
                phase: TransferPhase::ReadSource,
                error: TransportError::IoError,
                message: Some("source peer missing from sync run".to_string()),
            });
        };
        let Some(destination_peer) = self.peers.get(&task.destination_peer_id).copied() else {
            return CopyAttemptOutcome::Failure(CopyAttemptFailure {
                phase: TransferPhase::WriteSwapNew,
                error: TransportError::IoError,
                message: Some("destination peer missing from sync run".to_string()),
            });
        };

        let result = self.operations.execute_copy_attempt(
            source_peer,
            &task.source_path,
            destination_peer,
            &task.destination_path,
            &task.winning_meta,
        );

        if result.completed && result.error.is_none() {
            let mut results = self.results.lock().expect("copy result mutex poisoned");
            results.terminal_failures.remove(&CopyTaskKey::from_result(&result));
            results.successes.push(result.clone());
            CopyAttemptOutcome::Success(result)
        } else {
            self.results
                .lock()
                .expect("copy result mutex poisoned")
                .terminal_failures
                .insert(CopyTaskKey::from_task(task), result.clone());
            CopyAttemptOutcome::Failure(CopyAttemptFailure {
                phase: result.failed_phase.unwrap_or(TransferPhase::Cleanup),
                error: result.error.clone().unwrap_or(TransportError::IoError),
                message: Some(format!(
                    "{} -> {}",
                    result.source_path, result.destination_path
                )),
            })
        }
    }
}
