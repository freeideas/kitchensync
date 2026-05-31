use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use super::decision::{AbsenceDecision, DecisionOutcome, DirectoryDecision, FileDecision};
use super::SyncFailure;
use crate::operations::{OperationError, OperationExecutor};
use crate::runtime::{
    CopyAttemptFailure, CopyAttemptOutcome, CopyOperation, CopyScheduler, SchedulerSummary,
};
use crate::snapshot::{fresh_timestamp, SnapshotEntryKind};
use crate::{
    CopyResult, CopyTask, EntryKind, EntryMeta, PeerId, PeerSession, RelPath, Timestamp,
    TransferPhase, TransportError,
};

const MODIFY_TOLERANCE: Duration = Duration::from_secs(5);

pub(super) fn dispatch_path(input: PathDispatch<'_>) -> PathDispatchReport {
    match input.outcome {
        DecisionOutcome::CanonFile(decision)
        | DecisionOutcome::File(decision)
        | DecisionOutcome::TypeConflictFile(decision) => dispatch_file(input.effects, decision, input.active_peers),
        DecisionOutcome::CanonDirectory(decision) | DecisionOutcome::Directory(decision) => {
            dispatch_directory(input.effects, decision, input.active_peers)
        }
        DecisionOutcome::CanonAbsence(decision)
        | DecisionOutcome::Absence(decision)
        | DecisionOutcome::NoVoteAbsence(decision) => {
            dispatch_absence(input.effects, decision, input.active_peers)
        }
        DecisionOutcome::Skipped { .. } | DecisionOutcome::InvalidInput { .. } => {
            PathDispatchReport::default()
        }
    }
}

pub(super) fn finish_dispatch(input: FinishDispatch<'_>) -> FinishDispatchReport {
    input.scheduler.close();

    let operation = SchedulerCopyOperation::new(input.operations, input.sessions);
    let summary = input.scheduler.run_until_complete(&operation);
    let results = operation.into_results();

    for result in &results.successes {
        input.snapshot_flow.copy_completed(result);
    }

    FinishDispatchReport {
        summary,
        successful_copies: results.successes,
        failed_copies: results.terminal_failures.into_values().collect(),
    }
}

pub(super) struct PathDispatch<'a> {
    pub effects: DispatchEffects<'a>,
    pub outcome: &'a DecisionOutcome,
    pub active_peers: &'a [ActivePeer<'a>],
}

#[derive(Clone, Copy)]
pub(super) struct DispatchEffects<'a> {
    pub operations: &'a dyn OperationExecutor,
    pub scheduler: &'a CopyScheduler,
    pub snapshot_flow: &'a dyn SnapshotFlowNotifier,
}

pub(super) struct FinishDispatch<'a> {
    pub operations: &'a dyn OperationExecutor,
    pub scheduler: &'a CopyScheduler,
    pub snapshot_flow: &'a dyn SnapshotFlowNotifier,
    pub sessions: Vec<&'a PeerSession>,
}

#[derive(Debug, Clone)]
pub(super) struct ActivePeer<'a> {
    pub session: &'a PeerSession,
    pub live: Option<EntryMeta>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct PathDispatchReport {
    pub child_recursion_peers: Vec<PeerId>,
    pub enqueued_copies: u64,
    pub failures: Vec<SyncFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FinishDispatchReport {
    pub summary: SchedulerSummary,
    pub successful_copies: Vec<CopyResult>,
    pub failed_copies: Vec<CopyResult>,
}

pub(super) trait SnapshotFlowNotifier {
    fn intended_copy(&self, peer_id: PeerId, path: &RelPath, winning_meta: &EntryMeta);
    fn directory_created(&self, peer_id: PeerId, path: &RelPath, meta: &EntryMeta);
    fn displaced(&self, peer_id: PeerId, path: &RelPath, kind: SnapshotEntryKind);
    fn copy_completed(&self, result: &CopyResult);
}

fn dispatch_file(
    effects: DispatchEffects<'_>,
    decision: &FileDecision,
    active_peers: &[ActivePeer<'_>],
) -> PathDispatchReport {
    let mut report = PathDispatchReport::default();

    for target in active_peers {
        if target.live.as_ref().is_some_and(|meta| meta.kind == EntryKind::Directory) {
            if !displace(
                effects,
                target.session,
                &decision.path,
                SnapshotEntryKind::Directory,
                &mut report,
            ) {
                continue;
            }
        }

        if let Some(live) = target.live.as_ref().filter(|meta| meta.kind == EntryKind::File) {
            if file_matches(live, &decision.winning_meta) {
                continue;
            }
        }

        effects.scheduler.submit(CopyTask {
            source_peer_id: decision.source_peer_id,
            source_path: decision.path.clone(),
            destination_peer_id: target.session.id,
            destination_path: decision.path.clone(),
            winning_meta: decision.winning_meta.clone(),
        });
        effects
            .snapshot_flow
            .intended_copy(target.session.id, &decision.path, &decision.winning_meta);
        report.enqueued_copies += 1;
    }

    report
}

fn dispatch_directory(
    effects: DispatchEffects<'_>,
    decision: &DirectoryDecision,
    active_peers: &[ActivePeer<'_>],
) -> PathDispatchReport {
    let mut report = PathDispatchReport::default();
    let directory_meta = directory_meta(&decision.path);

    for target in active_peers {
        match target.live.as_ref().map(|meta| meta.kind) {
            Some(EntryKind::Directory) => {
                report.child_recursion_peers.push(target.session.id);
            }
            Some(EntryKind::File) => {
                if displace(
                    effects,
                    target.session,
                    &decision.path,
                    SnapshotEntryKind::File,
                    &mut report,
                ) && create_directory(
                    effects,
                    target.session,
                    &decision.path,
                    &directory_meta,
                    &mut report,
                ) {
                    report.child_recursion_peers.push(target.session.id);
                }
            }
            Some(EntryKind::SymbolicLink) => {}
            None => {
                if create_directory(
                    effects,
                    target.session,
                    &decision.path,
                    &directory_meta,
                    &mut report,
                ) {
                    report.child_recursion_peers.push(target.session.id);
                }
            }
        }
    }

    report
}

fn dispatch_absence(
    effects: DispatchEffects<'_>,
    decision: &AbsenceDecision,
    active_peers: &[ActivePeer<'_>],
) -> PathDispatchReport {
    let mut report = PathDispatchReport::default();

    for target in active_peers {
        match target.live.as_ref().map(|meta| meta.kind) {
            Some(EntryKind::File) => {
                displace(
                    effects,
                    target.session,
                    &decision.path,
                    SnapshotEntryKind::File,
                    &mut report,
                );
            }
            Some(EntryKind::Directory) => {
                displace(
                    effects,
                    target.session,
                    &decision.path,
                    SnapshotEntryKind::Directory,
                    &mut report,
                );
            }
            Some(EntryKind::SymbolicLink) => {}
            None => {}
        }
    }

    report
}

fn displace(
    effects: DispatchEffects<'_>,
    peer: &PeerSession,
    path: &RelPath,
    kind: SnapshotEntryKind,
    report: &mut PathDispatchReport,
) -> bool {
    match effects
        .operations
        .displace_to_bak(peer, path, fresh_timestamp())
    {
        Ok(_) => {
            effects.snapshot_flow.displaced(peer.id, path, kind);
            true
        }
        Err(error) => {
            push_operation_failure(report, peer.id, path, error);
            false
        }
    }
}

fn create_directory(
    effects: DispatchEffects<'_>,
    peer: &PeerSession,
    path: &RelPath,
    meta: &EntryMeta,
    report: &mut PathDispatchReport,
) -> bool {
    match effects.operations.create_directory(peer, path) {
        Ok(_) => {
            effects.snapshot_flow.directory_created(peer.id, path, meta);
            true
        }
        Err(error) => {
            push_operation_failure(report, peer.id, path, error);
            false
        }
    }
}

fn push_operation_failure(
    report: &mut PathDispatchReport,
    peer_id: PeerId,
    path: &RelPath,
    error: OperationError,
) {
    report.failures.push(SyncFailure::Operation {
        peer_id,
        path: path.clone(),
        error,
    });
}

fn directory_meta(path: &RelPath) -> EntryMeta {
    EntryMeta {
        name: basename(path).to_string(),
        kind: EntryKind::Directory,
        mod_time: fresh_timestamp(),
        byte_size: -1,
    }
}

fn basename(path: &RelPath) -> &str {
    path.as_str()
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(path.as_str())
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

    Some(
        SystemTime::UNIX_EPOCH
            + Duration::from_secs(seconds as u64)
            + Duration::from_micros(micros as u64),
    )
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
            self.record_failed_task(task, TransferPhase::ReadSource, TransportError::IoError);
            return CopyAttemptOutcome::Failure(CopyAttemptFailure {
                phase: TransferPhase::ReadSource,
                error: TransportError::IoError,
                message: Some("source peer missing from sync run".to_string()),
            });
        };
        let Some(destination_peer) = self.peers.get(&task.destination_peer_id).copied() else {
            self.record_failed_task(task, TransferPhase::WriteSwapNew, TransportError::IoError);
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
            results
                .terminal_failures
                .remove(&CopyTaskKey::from_result(&result));
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

impl SchedulerCopyOperation<'_> {
    fn record_failed_task(&self, task: &CopyTask, phase: TransferPhase, error: TransportError) {
        self.results
            .lock()
            .expect("copy result mutex poisoned")
            .terminal_failures
            .insert(
                CopyTaskKey::from_task(task),
                CopyResult {
                    source_peer_id: task.source_peer_id,
                    source_path: task.source_path.clone(),
                    destination_peer_id: task.destination_peer_id,
                    destination_path: task.destination_path.clone(),
                    bytes_copied: 0,
                    completed: false,
                    failed_phase: Some(phase),
                    error: Some(error),
                },
            );
    }
}
