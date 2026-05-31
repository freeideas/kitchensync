use std::time::{Duration, SystemTime};

use super::classify::{
    ClassifiedCandidate, ContributingObservation, ContributingState, LiveFileObservation,
};
use crate::{EntryKind, EntryMeta, PeerId, RelPath, Timestamp};

const MODIFY_TOLERANCE: Duration = Duration::from_secs(5);

pub(super) fn decide_path(input: ClassifiedDecisionInput) -> DecisionOutcome {
    if input.active_canon_count > 1 {
        return invalid(
            input.candidate.path,
            InvalidDecisionInput::MultipleCanonPeers,
        );
    }

    if let Some(reason) = input.skip {
        return DecisionOutcome::Skipped {
            path: input.candidate.path,
            reason,
        };
    }

    if (input.canon_required || input.active_canon_count == 1) && input.candidate.canon.is_none() {
        return invalid(
            input.candidate.path,
            InvalidDecisionInput::CanonControlWithoutCanonState,
        );
    }

    if input.active_canon_count == 0 && input.candidate.canon.is_some() {
        return invalid(
            input.candidate.path,
            InvalidDecisionInput::CanonStateWithoutActiveCanon,
        );
    }

    if let Some(canon) = input.candidate.canon {
        return match canon.state {
            ContributingState::LiveFile(file) => match file_outcome(
                input.candidate.path.clone(),
                canon.peer_id,
                file,
                FileDecisionReason::Canon,
            ) {
                Ok(file) => DecisionOutcome::CanonFile(file),
                Err(reason) => invalid(input.candidate.path, reason),
            },
            ContributingState::LiveDirectory(_) => {
                DecisionOutcome::CanonDirectory(DirectoryDecision {
                    path: input.candidate.path,
                    reason: DirectoryDecisionReason::Canon,
                })
            }
            ContributingState::TombstoneDeletionVote(_)
            | ContributingState::AbsentUnconfirmedFile(_)
            | ContributingState::AbsentDirectoryHistory(_)
            | ContributingState::NoVote => DecisionOutcome::CanonAbsence(AbsenceDecision {
                path: input.candidate.path,
                reason: AbsenceDecisionReason::Canon,
            }),
        };
    }

    if input.candidate.contributors.is_empty() {
        return invalid(
            input.candidate.path,
            InvalidDecisionInput::NoActiveContributingPeer,
        );
    }

    let path = input.candidate.path;
    let contributors = input.candidate.contributors;
    let live_files = contributing_live_files(&contributors);
    let has_live_directory = contributors
        .iter()
        .any(|observation| matches!(observation.state, ContributingState::LiveDirectory(_)));

    if !live_files.is_empty() {
        let selection = match choose_file_winner(&live_files) {
            Ok(selection) => selection,
            Err(reason) => return invalid(path, reason),
        };

        if has_live_directory {
            let file = match file_outcome(
                path.clone(),
                selection.winner.peer_id,
                selection.winner.file.clone(),
                FileDecisionReason::TypeConflictFilePreferred,
            ) {
                Ok(file) => file,
                Err(reason) => return invalid(path, reason),
            };
            return DecisionOutcome::TypeConflictFile(file);
        }

        if deletion_wins(&contributors, selection.newest_mod_time) {
            return DecisionOutcome::Absence(AbsenceDecision {
                path,
                reason: AbsenceDecisionReason::DeletionEstimate,
            });
        }

        let file = match file_outcome(
            path.clone(),
            selection.winner.peer_id,
            selection.winner.file.clone(),
            FileDecisionReason::NewestLiveFile,
        ) {
            Ok(file) => file,
            Err(reason) => return invalid(path, reason),
        };
        DecisionOutcome::File(file)
    } else if has_live_directory {
        DecisionOutcome::Directory(DirectoryDecision {
            path,
            reason: DirectoryDecisionReason::ContributingLiveDirectory,
        })
    } else if has_deletion_or_absence_history(&contributors) {
        DecisionOutcome::Absence(AbsenceDecision {
            path,
            reason: AbsenceDecisionReason::DeletionOrSnapshotHistory,
        })
    } else {
        DecisionOutcome::NoVoteAbsence(AbsenceDecision {
            path,
            reason: AbsenceDecisionReason::NoVote,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ClassifiedDecisionInput {
    pub candidate: ClassifiedCandidate,
    pub active_canon_count: usize,
    pub canon_required: bool,
    pub skip: Option<DecisionSkipReason>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum DecisionOutcome {
    CanonFile(FileDecision),
    CanonDirectory(DirectoryDecision),
    CanonAbsence(AbsenceDecision),
    File(FileDecision),
    Directory(DirectoryDecision),
    Absence(AbsenceDecision),
    NoVoteAbsence(AbsenceDecision),
    TypeConflictFile(FileDecision),
    Skipped {
        path: RelPath,
        reason: DecisionSkipReason,
    },
    InvalidInput {
        path: RelPath,
        reason: InvalidDecisionInput,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FileDecision {
    pub path: RelPath,
    pub source_peer_id: PeerId,
    pub winning_meta: EntryMeta,
    pub reason: FileDecisionReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum FileDecisionReason {
    Canon,
    NewestLiveFile,
    TypeConflictFilePreferred,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DirectoryDecision {
    pub path: RelPath,
    pub reason: DirectoryDecisionReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DirectoryDecisionReason {
    Canon,
    ContributingLiveDirectory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AbsenceDecision {
    pub path: RelPath,
    pub reason: AbsenceDecisionReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AbsenceDecisionReason {
    Canon,
    DeletionEstimate,
    DeletionOrSnapshotHistory,
    NoVote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DecisionSkipReason {
    TraversalPolicy,
    ClassificationUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum InvalidDecisionInput {
    MultipleCanonPeers,
    CanonControlWithoutCanonState,
    CanonStateWithoutActiveCanon,
    NoActiveContributingPeer,
    FileCandidateMissingMetadata { peer_id: PeerId },
    FileOutcomeWithoutSource,
}

#[derive(Clone, Copy)]
struct FileCandidate<'a> {
    peer_id: PeerId,
    file: &'a LiveFileObservation,
}

#[derive(Clone, Copy)]
struct FileSelection<'a> {
    winner: FileCandidate<'a>,
    newest_mod_time: &'a Timestamp,
}

fn invalid(path: RelPath, reason: InvalidDecisionInput) -> DecisionOutcome {
    DecisionOutcome::InvalidInput { path, reason }
}

fn contributing_live_files(contributors: &[ContributingObservation]) -> Vec<FileCandidate<'_>> {
    contributors
        .iter()
        .filter_map(|observation| match &observation.state {
            ContributingState::LiveFile(file) => Some(FileCandidate {
                peer_id: observation.peer_id,
                file,
            }),
            ContributingState::LiveDirectory(_)
            | ContributingState::TombstoneDeletionVote(_)
            | ContributingState::AbsentUnconfirmedFile(_)
            | ContributingState::AbsentDirectoryHistory(_)
            | ContributingState::NoVote => None,
        })
        .collect()
}

fn choose_file_winner<'a>(
    candidates: &[FileCandidate<'a>],
) -> Result<FileSelection<'a>, InvalidDecisionInput> {
    let Some(first) = candidates.first() else {
        return Err(InvalidDecisionInput::FileOutcomeWithoutSource);
    };

    for candidate in candidates {
        validate_file_candidate(*candidate)?;
    }

    let newest_mod_time = candidates
        .iter()
        .map(|candidate| &candidate.file.meta.mod_time)
        .max_by(|left, right| compare_timestamps(left, right))
        .expect("non-empty candidates already checked");

    let winner = candidates
        .iter()
        .copied()
        .filter(|candidate| {
            !timestamp_more_than_tolerance_newer(newest_mod_time, &candidate.file.meta.mod_time)
        })
        .max_by(|left, right| compare_file_tie(left, right))
        .unwrap_or(*first);

    Ok(FileSelection {
        winner,
        newest_mod_time,
    })
}

fn compare_file_tie(left: &FileCandidate<'_>, right: &FileCandidate<'_>) -> std::cmp::Ordering {
    left.file
        .meta
        .byte_size
        .cmp(&right.file.meta.byte_size)
        .then_with(|| right.peer_id.cmp(&left.peer_id))
}

fn file_outcome(
    path: RelPath,
    source_peer_id: PeerId,
    file: LiveFileObservation,
    reason: FileDecisionReason,
) -> Result<FileDecision, InvalidDecisionInput> {
    if file.meta.kind != EntryKind::File || file.meta.byte_size < 0 {
        return Err(InvalidDecisionInput::FileCandidateMissingMetadata {
            peer_id: source_peer_id,
        });
    }

    Ok(FileDecision {
        path,
        source_peer_id,
        winning_meta: file.meta,
        reason,
    })
}

fn validate_file_candidate(candidate: FileCandidate<'_>) -> Result<(), InvalidDecisionInput> {
    if candidate.file.meta.kind != EntryKind::File || candidate.file.meta.byte_size < 0 {
        return Err(InvalidDecisionInput::FileCandidateMissingMetadata {
            peer_id: candidate.peer_id,
        });
    }

    Ok(())
}

fn deletion_wins(contributors: &[ContributingObservation], newest_file: &Timestamp) -> bool {
    contributors
        .iter()
        .filter_map(deletion_estimate)
        .max_by(compare_timestamps)
        .is_some_and(|deleted| timestamp_more_than_tolerance_newer(deleted, newest_file))
}

fn deletion_estimate(observation: &ContributingObservation) -> Option<&Timestamp> {
    match &observation.state {
        ContributingState::TombstoneDeletionVote(vote) => Some(&vote.deleted_time),
        ContributingState::AbsentUnconfirmedFile(absent) => Some(&absent.previous.last_seen),
        ContributingState::LiveFile(_)
        | ContributingState::LiveDirectory(_)
        | ContributingState::AbsentDirectoryHistory(_)
        | ContributingState::NoVote => None,
    }
}

fn has_deletion_or_absence_history(contributors: &[ContributingObservation]) -> bool {
    contributors.iter().any(|observation| {
        matches!(
            observation.state,
            ContributingState::TombstoneDeletionVote(_)
                | ContributingState::AbsentUnconfirmedFile(_)
                | ContributingState::AbsentDirectoryHistory(_)
        )
    })
}

fn compare_timestamps(left: &&Timestamp, right: &&Timestamp) -> std::cmp::Ordering {
    match (parse_timestamp(*left), parse_timestamp(*right)) {
        (Some(left), Some(right)) => left.cmp(&right),
        _ => left.0.cmp(&right.0),
    }
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
    if hour > 23 || minute > 59 || second > 59 || micros > 999_999 {
        return None;
    }

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
