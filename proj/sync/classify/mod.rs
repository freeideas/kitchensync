use std::collections::HashSet;
use std::time::{Duration, SystemTime};

#[cfg(not(test))]
use crate::snapshot::{SnapshotEntryKind, SnapshotRow};
#[cfg(not(test))]
use crate::{EffectivePeerRole, EntryKind, EntryMeta, PeerId, PeerSession, RelPath, Timestamp};
#[cfg(test)]
use kitchensync::snapshot::{SnapshotEntryKind, SnapshotRow};
#[cfg(test)]
use kitchensync::{
    EffectivePeerRole, EntryKind, EntryMeta, PeerId, PeerSession, RelPath, Timestamp,
};

const MODIFY_TOLERANCE: Duration = Duration::from_secs(5);

pub(super) fn classify_candidate(
    input: ClassificationInput,
) -> Result<ClassifiedCandidate, ClassificationError> {
    let mut seen = HashSet::new();
    let mut canon = None;
    let mut contributors = Vec::new();
    let mut subordinates = Vec::new();
    let mut summary = ClassificationSummary::default();

    for peer in input.peers {
        let peer_id = peer.session.id;
        if !seen.insert(peer_id) {
            return Err(ClassificationError::DuplicatePeer { peer_id });
        }

        if let Some(live) = peer.live.as_ref() {
            validate_live(peer_id, live)?;
        }
        validate_snapshot_lookup_path(&input.path, peer_id, &peer.snapshot)?;

        match peer.session.effective_role {
            EffectivePeerRole::Canon | EffectivePeerRole::Contributing => {
                let state = classify_contributing(peer_id, peer.live, peer.snapshot)?;
                update_summary(&mut summary, &state);

                if peer.session.effective_role == EffectivePeerRole::Canon {
                    if canon.is_some() {
                        return Err(ClassificationError::MultipleCanonPeers);
                    }
                    canon = Some(CanonObservation {
                        peer_id,
                        state: state.clone(),
                    });
                }

                contributors.push(ContributingObservation { peer_id, state });
            }
            EffectivePeerRole::Subordinate => {
                let snapshot = classify_subordinate_snapshot(peer_id, peer.snapshot)?;
                subordinates.push(SubordinateTarget {
                    peer_id,
                    live: peer.live,
                    snapshot,
                });
            }
        }
    }

    Ok(ClassifiedCandidate {
        path: input.path,
        basename: input.basename,
        canon,
        contributors,
        subordinates,
        summary,
    })
}

#[derive(Debug, Clone)]
pub(super) struct ClassificationInput {
    pub path: RelPath,
    pub basename: String,
    pub peers: Vec<PeerCandidateInput>,
}

#[derive(Debug, Clone)]
pub(super) struct PeerCandidateInput {
    pub session: PeerSession,
    pub live: Option<EntryMeta>,
    pub snapshot: SnapshotLookup,
}

#[derive(Debug, Clone)]
pub(super) enum SnapshotLookup {
    NotLookedUp,
    Missing,
    Present(SnapshotRow),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ClassifiedCandidate {
    pub path: RelPath,
    pub basename: String,
    pub canon: Option<CanonObservation>,
    pub contributors: Vec<ContributingObservation>,
    pub subordinates: Vec<SubordinateTarget>,
    pub summary: ClassificationSummary,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct ClassificationSummary {
    pub has_live_file: bool,
    pub has_live_directory: bool,
    pub has_deletion_vote: bool,
    pub has_unconfirmed_absence: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CanonObservation {
    pub peer_id: PeerId,
    pub state: ContributingState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ContributingObservation {
    pub peer_id: PeerId,
    pub state: ContributingState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ContributingState {
    LiveFile(LiveFileObservation),
    LiveDirectory(LiveDirectoryObservation),
    TombstoneDeletionVote(TombstoneDeletionVote),
    AbsentUnconfirmedFile(AbsentUnconfirmedFile),
    AbsentDirectoryHistory(AbsentDirectoryHistory),
    NoVote,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LiveFileObservation {
    pub meta: EntryMeta,
    pub snapshot: LiveFileSnapshotState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum LiveFileSnapshotState {
    Unchanged { previous: SnapshotFileFacts },
    Modified { previous: SnapshotKnownFacts },
    Resurrected { tombstone: SnapshotTombstoneFacts },
    New,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LiveDirectoryObservation {
    pub meta: EntryMeta,
    pub previous: Option<SnapshotDirectoryFacts>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TombstoneDeletionVote {
    pub deleted_time: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AbsentUnconfirmedFile {
    pub previous: SnapshotFileFacts,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AbsentDirectoryHistory {
    pub previous: SnapshotDirectoryFacts,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SnapshotFileFacts {
    pub size: i64,
    pub modified_time: Timestamp,
    pub last_seen: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SnapshotDirectoryFacts {
    pub modified_time: Option<Timestamp>,
    pub last_seen: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SnapshotTombstoneFacts {
    pub previous_kind: Option<SnapshotEntryKind>,
    pub deleted_time: Timestamp,
    pub last_seen: Option<Timestamp>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SnapshotKnownFacts {
    File(SnapshotFileFacts),
    Directory(SnapshotDirectoryFacts),
    Tombstone(SnapshotTombstoneFacts),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SubordinateTarget {
    pub peer_id: PeerId,
    pub live: Option<EntryMeta>,
    pub snapshot: Option<SubordinateSnapshotFacts>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SubordinateSnapshotFacts {
    File(SnapshotFileFacts),
    Directory(SnapshotDirectoryFacts),
    Tombstone(SnapshotTombstoneFacts),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ClassificationError {
    DuplicatePeer {
        peer_id: PeerId,
    },
    UnknownOrInactivePeer {
        peer_id: PeerId,
    },
    MultipleCanonPeers,
    InvalidLiveMetadata {
        peer_id: PeerId,
        reason: InvalidLiveMetadata,
    },
    InvalidSnapshotState {
        peer_id: PeerId,
        reason: InvalidSnapshotState,
    },
    MissingRequiredSnapshot {
        peer_id: PeerId,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InvalidLiveMetadata {
    FileWithoutSize,
    DirectoryWithFileSize,
    UnsupportedEntryKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InvalidSnapshotState {
    KindFactsMismatch,
    TombstoneWithoutDeletedTime,
    UnsupportedEntryKind,
}

fn classify_contributing(
    peer_id: PeerId,
    live: Option<EntryMeta>,
    snapshot: SnapshotLookup,
) -> Result<ContributingState, ClassificationError> {
    match live {
        Some(meta) => match meta.kind {
            EntryKind::File => classify_live_file(peer_id, meta, snapshot),
            EntryKind::Directory => classify_live_directory(peer_id, meta, snapshot),
            EntryKind::SymbolicLink => Err(ClassificationError::InvalidLiveMetadata {
                peer_id,
                reason: InvalidLiveMetadata::UnsupportedEntryKind,
            }),
        },
        None => classify_absent_contributing(peer_id, snapshot),
    }
}

fn classify_live_file(
    peer_id: PeerId,
    meta: EntryMeta,
    snapshot: SnapshotLookup,
) -> Result<ContributingState, ClassificationError> {
    let snapshot = match snapshot {
        SnapshotLookup::NotLookedUp => {
            return Err(ClassificationError::MissingRequiredSnapshot { peer_id });
        }
        SnapshotLookup::Missing => LiveFileSnapshotState::New,
        SnapshotLookup::Present(row) => match known_facts(peer_id, row)? {
            SnapshotKnownFacts::File(previous) => {
                if previous.size == meta.byte_size
                    && timestamps_within_tolerance(&previous.modified_time, &meta.mod_time)
                {
                    LiveFileSnapshotState::Unchanged { previous }
                } else {
                    LiveFileSnapshotState::Modified {
                        previous: SnapshotKnownFacts::File(previous),
                    }
                }
            }
            SnapshotKnownFacts::Directory(previous) => LiveFileSnapshotState::Modified {
                previous: SnapshotKnownFacts::Directory(previous),
            },
            SnapshotKnownFacts::Tombstone(tombstone) => {
                LiveFileSnapshotState::Resurrected { tombstone }
            }
        },
    };

    Ok(ContributingState::LiveFile(LiveFileObservation {
        meta,
        snapshot,
    }))
}

fn classify_live_directory(
    peer_id: PeerId,
    meta: EntryMeta,
    snapshot: SnapshotLookup,
) -> Result<ContributingState, ClassificationError> {
    let previous = match snapshot {
        SnapshotLookup::NotLookedUp | SnapshotLookup::Missing => None,
        SnapshotLookup::Present(row) => match known_facts(peer_id, row)? {
            SnapshotKnownFacts::Directory(previous) => Some(previous),
            SnapshotKnownFacts::File(_) | SnapshotKnownFacts::Tombstone(_) => None,
        },
    };

    Ok(ContributingState::LiveDirectory(LiveDirectoryObservation {
        meta,
        previous,
    }))
}

fn classify_absent_contributing(
    peer_id: PeerId,
    snapshot: SnapshotLookup,
) -> Result<ContributingState, ClassificationError> {
    match snapshot {
        SnapshotLookup::NotLookedUp => {
            Err(ClassificationError::MissingRequiredSnapshot { peer_id })
        }
        SnapshotLookup::Missing => Ok(ContributingState::NoVote),
        SnapshotLookup::Present(row) => match known_facts(peer_id, row)? {
            SnapshotKnownFacts::Tombstone(tombstone) => Ok(
                ContributingState::TombstoneDeletionVote(TombstoneDeletionVote {
                    deleted_time: tombstone.deleted_time,
                }),
            ),
            SnapshotKnownFacts::File(previous) => Ok(ContributingState::AbsentUnconfirmedFile(
                AbsentUnconfirmedFile { previous },
            )),
            SnapshotKnownFacts::Directory(previous) => Ok(
                ContributingState::AbsentDirectoryHistory(AbsentDirectoryHistory { previous }),
            ),
        },
    }
}

fn classify_subordinate_snapshot(
    peer_id: PeerId,
    snapshot: SnapshotLookup,
) -> Result<Option<SubordinateSnapshotFacts>, ClassificationError> {
    match snapshot {
        SnapshotLookup::NotLookedUp | SnapshotLookup::Missing => Ok(None),
        SnapshotLookup::Present(row) => Ok(Some(match known_facts(peer_id, row)? {
            SnapshotKnownFacts::File(facts) => SubordinateSnapshotFacts::File(facts),
            SnapshotKnownFacts::Directory(facts) => SubordinateSnapshotFacts::Directory(facts),
            SnapshotKnownFacts::Tombstone(facts) => SubordinateSnapshotFacts::Tombstone(facts),
        })),
    }
}

fn known_facts(
    peer_id: PeerId,
    row: SnapshotRow,
) -> Result<SnapshotKnownFacts, ClassificationError> {
    if let Some(ref deleted_time) = row.deleted_time {
        let previous_kind = tombstone_previous_kind(peer_id, &row)?;
        return Ok(SnapshotKnownFacts::Tombstone(SnapshotTombstoneFacts {
            previous_kind,
            deleted_time: deleted_time.clone(),
            last_seen: row.last_seen,
        }));
    }

    if row.kind == SnapshotEntryKind::Tombstone {
        return Err(ClassificationError::InvalidSnapshotState {
            peer_id,
            reason: InvalidSnapshotState::TombstoneWithoutDeletedTime,
        });
    }

    validate_snapshot_row(peer_id, &row)?;

    match row.kind {
        SnapshotEntryKind::File => {
            let Some(last_seen) = row.last_seen else {
                return Err(ClassificationError::InvalidSnapshotState {
                    peer_id,
                    reason: InvalidSnapshotState::KindFactsMismatch,
                });
            };
            Ok(SnapshotKnownFacts::File(SnapshotFileFacts {
                size: row.byte_size,
                modified_time: row.mod_time,
                last_seen,
            }))
        }
        SnapshotEntryKind::Directory => {
            let Some(last_seen) = row.last_seen else {
                return Err(ClassificationError::InvalidSnapshotState {
                    peer_id,
                    reason: InvalidSnapshotState::KindFactsMismatch,
                });
            };
            Ok(SnapshotKnownFacts::Directory(SnapshotDirectoryFacts {
                modified_time: Some(row.mod_time),
                last_seen,
            }))
        }
        SnapshotEntryKind::Tombstone => Err(ClassificationError::InvalidSnapshotState {
            peer_id,
            reason: InvalidSnapshotState::TombstoneWithoutDeletedTime,
        }),
    }
}

fn tombstone_previous_kind(
    peer_id: PeerId,
    row: &SnapshotRow,
) -> Result<Option<SnapshotEntryKind>, ClassificationError> {
    match row.kind {
        SnapshotEntryKind::File if row.byte_size >= 0 => Ok(Some(SnapshotEntryKind::File)),
        SnapshotEntryKind::Directory if row.byte_size == -1 => {
            Ok(Some(SnapshotEntryKind::Directory))
        }
        SnapshotEntryKind::Tombstone if row.byte_size >= 0 => Ok(Some(SnapshotEntryKind::File)),
        SnapshotEntryKind::Tombstone if row.byte_size == -1 => {
            Ok(Some(SnapshotEntryKind::Directory))
        }
        SnapshotEntryKind::File | SnapshotEntryKind::Directory | SnapshotEntryKind::Tombstone => {
            Err(ClassificationError::InvalidSnapshotState {
                peer_id,
                reason: InvalidSnapshotState::KindFactsMismatch,
            })
        }
    }
}

fn validate_live(peer_id: PeerId, live: &EntryMeta) -> Result<(), ClassificationError> {
    match live.kind {
        EntryKind::File if live.byte_size < 0 => Err(ClassificationError::InvalidLiveMetadata {
            peer_id,
            reason: InvalidLiveMetadata::FileWithoutSize,
        }),
        EntryKind::Directory if live.byte_size != -1 => {
            Err(ClassificationError::InvalidLiveMetadata {
                peer_id,
                reason: InvalidLiveMetadata::DirectoryWithFileSize,
            })
        }
        EntryKind::SymbolicLink => Err(ClassificationError::InvalidLiveMetadata {
            peer_id,
            reason: InvalidLiveMetadata::UnsupportedEntryKind,
        }),
        EntryKind::File | EntryKind::Directory => Ok(()),
    }
}

fn validate_snapshot_row(peer_id: PeerId, row: &SnapshotRow) -> Result<(), ClassificationError> {
    let valid = match row.kind {
        SnapshotEntryKind::File => row.byte_size >= 0,
        SnapshotEntryKind::Directory => row.byte_size == -1,
        SnapshotEntryKind::Tombstone => row.byte_size == -1,
    };

    if !valid {
        return Err(ClassificationError::InvalidSnapshotState {
            peer_id,
            reason: InvalidSnapshotState::KindFactsMismatch,
        });
    }

    Ok(())
}

fn validate_snapshot_lookup_path(
    path: &RelPath,
    peer_id: PeerId,
    snapshot: &SnapshotLookup,
) -> Result<(), ClassificationError> {
    if let SnapshotLookup::Present(row) = snapshot {
        if &row.path != path {
            return Err(ClassificationError::InvalidSnapshotState {
                peer_id,
                reason: InvalidSnapshotState::KindFactsMismatch,
            });
        }
    }

    Ok(())
}

fn update_summary(summary: &mut ClassificationSummary, state: &ContributingState) {
    match state {
        ContributingState::LiveFile(_) => summary.has_live_file = true,
        ContributingState::LiveDirectory(_) => summary.has_live_directory = true,
        ContributingState::TombstoneDeletionVote(_) => summary.has_deletion_vote = true,
        ContributingState::AbsentUnconfirmedFile(_) => summary.has_unconfirmed_absence = true,
        ContributingState::AbsentDirectoryHistory(_) | ContributingState::NoVote => {}
    }
}

fn timestamps_within_tolerance(left: &Timestamp, right: &Timestamp) -> bool {
    match (parse_timestamp(left), parse_timestamp(right)) {
        (Some(left), Some(right)) => {
            let difference = if left >= right {
                left.duration_since(right)
            } else {
                right.duration_since(left)
            };
            difference.is_ok_and(|difference| difference <= MODIFY_TOLERANCE)
        }
        _ => left.0 == right.0,
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
