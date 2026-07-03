use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::time::SystemTime;

use copystaging::{
    CopyStagingCopyRequest, CopyStagingCopyStatus, CopyStagingDirectoryRequest,
    CopyStagingDisplacementRequest, CopyStagingDisplacementStatus, CopyStagingPeer,
    CopyStagingRunMode, CopyStagingRunOptions, CopyStagingVerbosity,
};
use formatrules::FormatRulesTimestamp;
use peertransportsurface::{PeerDirectoryEntry, PeerTransportError};
use snapshotdatabase::{
    SnapshotDatabaseCompletedCopyRequest, SnapshotDatabaseConfirmedAbsenceRequest,
    SnapshotDatabaseConfirmedFileRequest, SnapshotDatabaseCreatedDirectoryRequest,
    SnapshotDatabaseDisplacementRequest, SnapshotDatabaseEntryIdentity,
    SnapshotDatabaseIntendedCopyRequest, SnapshotDatabaseListedDirectoryRequest,
    SnapshotDatabaseRow,
};

use crate::api::*;

struct SyncTraversalImpl {
    formatrules: Arc<dyn formatrules::FormatRules>,
    peertransportsurface: Arc<dyn peertransportsurface::PeerTransportSurface>,
    snapshotdatabase: Arc<dyn snapshotdatabase::SnapshotDatabase>,
    copystaging: Arc<dyn copystaging::CopyStaging>,
}

#[derive(Clone)]
struct ActivePeer {
    peer: SyncTraversalPeer,
}

struct ListedDirectory {
    peer_index: usize,
    entries: HashMap<String, PeerDirectoryEntry>,
}

#[derive(Clone)]
struct EntryEvidence {
    peer: ActivePeer,
    live: Option<PeerDirectoryEntry>,
    row: Option<SnapshotDatabaseRow>,
}

#[derive(Clone)]
struct FileVersion {
    peer: ActivePeer,
    entry: PeerDirectoryEntry,
    mod_timestamp: FormatRulesTimestamp,
}

enum Outcome {
    File { sources: Vec<FileVersion> },
    Directory,
    Absence,
    SkipSubtree,
}

impl SyncTraversalImpl {
    fn default_copy_options(&self) -> CopyStagingRunOptions {
        CopyStagingRunOptions {
            mode: CopyStagingRunMode::Normal,
            max_copies: 10,
            retries_copy: 3,
            keep_bak_days: 90,
            keep_tmp_days: 2,
            verbosity: CopyStagingVerbosity::Info,
        }
    }

    fn peer_for_copy(&self, peer: &SyncTraversalPeer) -> CopyStagingPeer {
        CopyStagingPeer {
            peer_index: peer.peer_index,
            peer_url: peer.peer_url.clone(),
            root: peer.root.clone(),
        }
    }

    fn transport_path(path: Option<&str>) -> &str {
        path.unwrap_or("")
    }

    fn join_path(parent: Option<&str>, child: &str) -> String {
        match parent {
            Some(parent) if !parent.is_empty() => format!("{}/{}", parent, child),
            _ => child.to_string(),
        }
    }

    fn basename(path: &str) -> String {
        path.rsplit('/').next().unwrap_or(path).to_string()
    }

    fn entry_identity(&self, path: &str) -> Option<SnapshotDatabaseEntryIdentity> {
        let ids = self.formatrules.snapshot_path_ids(path).ok()?;
        Some(SnapshotDatabaseEntryIdentity {
            id: ids.id,
            parent_id: ids.parent_id,
            basename: Self::basename(path),
        })
    }

    fn row_for(&self, peer: &SyncTraversalPeer, entry_id: &str) -> Option<SnapshotDatabaseRow> {
        self.snapshotdatabase
            .read_snapshot_row(peer.snapshot_database.clone(), entry_id.to_string())
            .ok()
            .flatten()
    }

    fn timestamp_from_system_time(&self, time: SystemTime) -> FormatRulesTimestamp {
        self.formatrules.format_timestamp(time)
    }

    fn row_last_seen_timestamp(&self, row: &SnapshotDatabaseRow) -> Option<FormatRulesTimestamp> {
        row.last_seen
            .as_deref()
            .and_then(|value| self.formatrules.parse_timestamp(value).ok())
    }

    fn row_deleted_timestamp(&self, row: &SnapshotDatabaseRow) -> Option<FormatRulesTimestamp> {
        row.deleted_time
            .as_deref()
            .and_then(|value| self.formatrules.parse_timestamp(value).ok())
    }

    fn is_contributing(peer: &SyncTraversalPeer) -> bool {
        peer.role == SyncTraversalPeerRole::Canon
            || (peer.role == SyncTraversalPeerRole::Normal && peer.had_snapshot_history)
    }

    fn is_canon(peer: &SyncTraversalPeer) -> bool {
        peer.role == SyncTraversalPeerRole::Canon
    }

    fn is_excluded(&self, path: &str, is_dir: bool, excludes: &[String]) -> bool {
        if (path == ".kitchensync" || path == ".git") && is_dir {
            return true;
        }
        excludes
            .iter()
            .any(|exclude| path == exclude || path.starts_with(&format!("{}/", exclude)))
    }

    fn list_dir_for_peer(
        &self,
        peer: &ActivePeer,
        path: Option<&str>,
        retries: u64,
    ) -> Result<Vec<PeerDirectoryEntry>, PeerTransportError> {
        let attempts = retries.max(1);
        let copy_options = self.default_copy_options();
        let recovery = self.copystaging.recover_user_swap(CopyStagingDirectoryRequest {
            options: copy_options,
            peer: self.peer_for_copy(&peer.peer),
            directory_relative_path: path.map(ToOwned::to_owned),
        });
        if matches!(
            recovery.status,
            copystaging::CopyStagingSwapRecoveryStatus::Failed
        ) {
            return Err(PeerTransportError::IoError);
        }

        let mut last_error = PeerTransportError::IoError;
        for _ in 0..attempts {
            match self
                .peertransportsurface
                .list_dir(&peer.peer.root, Self::transport_path(path))
            {
                Ok(entries) => return Ok(entries),
                Err(error) => last_error = error,
            }
        }
        Err(last_error)
    }

    fn list_level(
        &self,
        peers: &[ActivePeer],
        path: Option<&str>,
        retries: u64,
        diagnostics: &mut Vec<SyncTraversalDiagnostic>,
    ) -> Option<Vec<ListedDirectory>> {
        let mut listed = Vec::new();
        let mut canon_failed = false;

        std::thread::scope(|scope| {
            let mut handles = Vec::new();
            for peer in peers {
                handles.push(scope.spawn(move || {
                    let result = self.list_dir_for_peer(peer, path, retries);
                    (peer.clone(), result)
                }));
            }

            for handle in handles {
                if let Ok((peer, result)) = handle.join() {
                    match result {
                        Ok(entries) => listed.push(ListedDirectory {
                            peer_index: peer.peer.peer_index,
                            entries: entries
                                .into_iter()
                                .map(|entry| (entry.child_name.clone(), entry))
                                .collect(),
                        }),
                        Err(error) => {
                            diagnostics.push(SyncTraversalDiagnostic {
                                level: SyncTraversalDiagnosticLevel::Error,
                                peer_index: peer.peer.peer_index,
                                path: path.map(ToOwned::to_owned),
                                kind: SyncTraversalDiagnosticKind::DirectoryListingFailed(error),
                            });
                            if Self::is_canon(&peer.peer) {
                                canon_failed = true;
                            }
                        }
                    }
                }
            }
        });

        if canon_failed {
            return None;
        }
        Some(listed)
    }

    fn listed_entry<'a>(
        &self,
        listings: &'a [ListedDirectory],
        peer_index: usize,
        child_name: &str,
    ) -> Option<&'a PeerDirectoryEntry> {
        listings
            .iter()
            .find(|listing| listing.peer_index == peer_index)
            .and_then(|listing| listing.entries.get(child_name))
    }

    fn traverse_directory(
        &self,
        peers: Vec<ActivePeer>,
        path: Option<String>,
        excludes: &[String],
        retries: u64,
        diagnostics: &mut Vec<SyncTraversalDiagnostic>,
    ) {
        let Some(listings) = self.list_level(
            &peers,
            path.as_deref(),
            retries,
            diagnostics,
        ) else {
            return;
        };

        let live_peer_indices: BTreeSet<usize> = listings.iter().map(|l| l.peer_index).collect();
        let active_peers: Vec<ActivePeer> = peers
            .into_iter()
            .filter(|peer| live_peer_indices.contains(&peer.peer.peer_index))
            .collect();

        if !active_peers.iter().any(|peer| Self::is_contributing(&peer.peer)) {
            return;
        }

        let mut names = BTreeSet::new();
        for listing in &listings {
            for name in listing.entries.keys() {
                names.insert((name.to_lowercase(), name.clone()));
            }
        }

        let mut recursion = Vec::new();
        for (_, name) in names {
            let rel_path = Self::join_path(path.as_deref(), &name);
            let visible_entry = listings
                .iter()
                .find_map(|listing| listing.entries.get(&name));
            if visible_entry
                .map(|entry| self.is_excluded(&rel_path, entry.is_dir, excludes))
                .unwrap_or(false)
            {
                continue;
            }

            let Some(identity) = self.entry_identity(&rel_path) else {
                continue;
            };
            let evidence = self.gather_evidence(&active_peers, &listings, &name, &identity.id);
            let outcome = self.decide_outcome(&rel_path, &evidence, retries, diagnostics);
            match outcome {
                Outcome::File { sources } => {
                    self.apply_file_outcome(&rel_path, &identity, &evidence, &sources);
                }
                Outcome::Directory => {
                    let next_peers = self.apply_directory_outcome(&rel_path, &identity, &evidence);
                    if !next_peers.is_empty() {
                        recursion.push((rel_path, next_peers));
                    }
                }
                Outcome::Absence => {
                    self.apply_absence_outcome(&rel_path, &identity, &evidence);
                }
                Outcome::SkipSubtree => {}
            }
        }

        for peer in &active_peers {
            let _ = self.copystaging.cleanup_metadata(CopyStagingDirectoryRequest {
                options: self.default_copy_options(),
                peer: self.peer_for_copy(&peer.peer),
                directory_relative_path: path.clone(),
            });
        }

        for (child_path, child_peers) in recursion {
            self.traverse_directory(
                child_peers,
                Some(child_path),
                excludes,
                retries,
                diagnostics,
            );
        }
    }

    fn gather_evidence(
        &self,
        peers: &[ActivePeer],
        listings: &[ListedDirectory],
        child_name: &str,
        entry_id: &str,
    ) -> Vec<EntryEvidence> {
        peers
            .iter()
            .map(|peer| EntryEvidence {
                peer: peer.clone(),
                live: self
                    .listed_entry(listings, peer.peer.peer_index, child_name)
                    .cloned(),
                row: self.row_for(&peer.peer, entry_id),
            })
            .collect()
    }

    fn decide_outcome(
        &self,
        path: &str,
        evidence: &[EntryEvidence],
        retries: u64,
        diagnostics: &mut Vec<SyncTraversalDiagnostic>,
    ) -> Outcome {
        if let Some(canon) = evidence.iter().find(|item| Self::is_canon(&item.peer.peer)) {
            return match &canon.live {
                Some(entry) if entry.is_dir => Outcome::Directory,
                Some(_) => self.decide_file_outcome(&[canon.clone()]),
                None => Outcome::Absence,
            };
        }

        let contributing: Vec<EntryEvidence> = evidence
            .iter()
            .filter(|item| Self::is_contributing(&item.peer.peer))
            .cloned()
            .collect();
        let has_file = contributing
            .iter()
            .any(|item| item.live.as_ref().map(|entry| !entry.is_dir).unwrap_or(false));
        let has_dir = contributing
            .iter()
            .any(|item| item.live.as_ref().map(|entry| entry.is_dir).unwrap_or(false));

        if has_file && has_dir {
            let file_only = contributing
                .into_iter()
                .filter(|item| item.live.as_ref().map(|entry| !entry.is_dir).unwrap_or(false))
                .collect::<Vec<_>>();
            return self.decide_file_outcome(&file_only);
        }

        if has_file {
            return self.decide_file_outcome(&contributing);
        }

        if has_dir {
            return self.decide_directory_outcome(path, evidence, &contributing, retries, diagnostics);
        }

        if contributing.iter().any(|item| item.row.is_some()) {
            Outcome::Absence
        } else {
            Outcome::Absence
        }
    }

    fn decide_file_outcome(&self, evidence: &[EntryEvidence]) -> Outcome {
        let live_versions = evidence
            .iter()
            .filter_map(|item| {
                let entry = item.live.as_ref()?;
                if entry.is_dir {
                    return None;
                }
                Some(FileVersion {
                    peer: item.peer.clone(),
                    entry: entry.clone(),
                    mod_timestamp: self.timestamp_from_system_time(entry.mod_time),
                })
            })
            .collect::<Vec<_>>();

        if live_versions.is_empty() {
            return Outcome::Absence;
        }

        let max_live = live_versions
            .iter()
            .map(|version| version.mod_timestamp.clone())
            .reduce(|max, current| {
                if self.formatrules.peer_mod_time_older_than_max(&max, &current) {
                    current
                } else {
                    max
                }
            })
            .unwrap();

        let newest_deletion = evidence
            .iter()
            .filter_map(|item| self.file_deletion_vote(item, &max_live))
            .max_by_key(|timestamp| self.formatrules.timestamp_system_time(timestamp));

        if let Some(deletion) = newest_deletion {
            let deletion_wins = live_versions.iter().all(|version| {
                self.formatrules
                    .deletion_estimate_wins_over_file_mod_time(&deletion, &version.mod_timestamp)
            });
            if deletion_wins {
                return Outcome::Absence;
            }
        }

        let tied_to_max = live_versions
            .iter()
            .filter(|version| {
                self.formatrules
                    .peer_mod_time_tied_with_max(&version.mod_timestamp, &max_live)
            })
            .cloned()
            .collect::<Vec<_>>();
        let max_size = tied_to_max
            .iter()
            .map(|version| version.entry.byte_size)
            .max()
            .unwrap_or(0);
        let sources = tied_to_max
            .into_iter()
            .filter(|version| version.entry.byte_size == max_size)
            .collect::<Vec<_>>();

        Outcome::File { sources }
    }

    fn file_deletion_vote(
        &self,
        item: &EntryEvidence,
        max_live_file_mod_time: &FormatRulesTimestamp,
    ) -> Option<FormatRulesTimestamp> {
        if item.live.as_ref().map(|entry| !entry.is_dir).unwrap_or(false) {
            return None;
        }
        let row = item.row.as_ref()?;
        if let Some(deleted) = self.row_deleted_timestamp(row) {
            return Some(deleted);
        }
        let last_seen = self.row_last_seen_timestamp(row)?;
        if self
            .formatrules
            .absent_unconfirmed_file_counts_as_deletion(&last_seen, max_live_file_mod_time)
        {
            Some(last_seen)
        } else {
            None
        }
    }

    fn decide_directory_outcome(
        &self,
        path: &str,
        evidence: &[EntryEvidence],
        contributing: &[EntryEvidence],
        retries: u64,
        diagnostics: &mut Vec<SyncTraversalDiagnostic>,
    ) -> Outcome {
        let live_dirs = contributing
            .iter()
            .filter(|item| item.live.as_ref().map(|entry| entry.is_dir).unwrap_or(false))
            .count();
        let row_absences = contributing
            .iter()
            .filter(|item| {
                item.live.is_none()
                    && item
                        .row
                        .as_ref()
                        .map(|row| row.byte_size == -1)
                        .unwrap_or(false)
            })
            .collect::<Vec<_>>();

        if live_dirs > 0 && row_absences.is_empty() {
            return Outcome::Directory;
        }
        if live_dirs == 0 {
            return Outcome::Absence;
        }

        let deletion_estimate = row_absences
            .iter()
            .filter_map(|item| {
                item.row
                    .as_ref()
                    .and_then(|row| self.row_deleted_timestamp(row).or_else(|| self.row_last_seen_timestamp(row)))
            })
            .max_by_key(|timestamp| self.formatrules.timestamp_system_time(timestamp));

        let Some(deletion_estimate) = deletion_estimate else {
            return Outcome::Directory;
        };

        let live_dir_peers = evidence
            .iter()
            .filter(|item| item.live.as_ref().map(|entry| entry.is_dir).unwrap_or(false))
            .map(|item| item.peer.clone())
            .collect::<Vec<_>>();
        let Some(file_times) = self.collect_live_file_times(
            path.to_string(),
            live_dir_peers,
            retries,
            diagnostics,
        ) else {
            return Outcome::SkipSubtree;
        };

        let Some(newest_live_file) = self
            .formatrules
            .directory_live_file_timestamp_evidence(&file_times)
        else {
            return Outcome::Absence;
        };

        if self
            .formatrules
            .directory_deletion_estimate_newer_than_live_file_evidence(
                &deletion_estimate,
                &newest_live_file,
            )
        {
            Outcome::Absence
        } else {
            Outcome::Directory
        }
    }

    fn collect_live_file_times(
        &self,
        path: String,
        peers: Vec<ActivePeer>,
        retries: u64,
        diagnostics: &mut Vec<SyncTraversalDiagnostic>,
    ) -> Option<Vec<FormatRulesTimestamp>> {
        let listings = self.list_level(&peers, Some(&path), retries, diagnostics)?;
        let live_peer_indices: BTreeSet<usize> = listings.iter().map(|l| l.peer_index).collect();
        let active_peers = peers
            .into_iter()
            .filter(|peer| live_peer_indices.contains(&peer.peer.peer_index))
            .collect::<Vec<_>>();
        let mut timestamps = Vec::new();
        let mut subdirs = Vec::new();
        for listing in &listings {
            for entry in listing.entries.values() {
                if entry.child_name == ".kitchensync" || entry.child_name == ".git" {
                    continue;
                }
                let child_path = Self::join_path(Some(&path), &entry.child_name);
                if entry.is_dir {
                    subdirs.push(child_path);
                } else {
                    timestamps.push(self.timestamp_from_system_time(entry.mod_time));
                }
            }
        }
        for subdir in subdirs {
            let mut child_peers = Vec::new();
            let child_name = Self::basename(&subdir);
            for peer in &active_peers {
                if self
                    .listed_entry(&listings, peer.peer.peer_index, &child_name)
                    .map(|entry| entry.is_dir)
                    .unwrap_or(false)
                {
                    child_peers.push(peer.clone());
                }
            }
            timestamps.extend(self.collect_live_file_times(subdir, child_peers, retries, diagnostics)?);
        }
        Some(timestamps)
    }

    fn apply_file_outcome(
        &self,
        path: &str,
        identity: &SnapshotDatabaseEntryIdentity,
        evidence: &[EntryEvidence],
        sources: &[FileVersion],
    ) {
        let Some(primary_source) = sources.first() else {
            return;
        };
        let winning_mod_time = primary_source.entry.mod_time;
        let winning_mod_text = self
            .formatrules
            .timestamp_text(&primary_source.mod_timestamp);
        let winning_size = primary_source.entry.byte_size;

        for item in evidence {
            if item
                .live
                .as_ref()
                .map(|entry| entry.is_dir)
                .unwrap_or(false)
            {
                if !self.displace(path, identity, item, true) {
                    continue;
                }
            }

            let already_source = sources.iter().any(|source| {
                source.peer.peer.peer_index == item.peer.peer.peer_index
                    && item
                        .live
                        .as_ref()
                        .map(|entry| {
                            !entry.is_dir
                                && entry.byte_size == source.entry.byte_size
                                && entry.mod_time == source.entry.mod_time
                        })
                        .unwrap_or(false)
            });

            if already_source {
                let _ = self.snapshotdatabase.record_confirmed_file(
                    SnapshotDatabaseConfirmedFileRequest {
                        database: item.peer.peer.snapshot_database.clone(),
                        entry: identity.clone(),
                        mod_time: winning_mod_text.clone(),
                        byte_size: winning_size,
                        last_seen: self
                            .formatrules
                            .timestamp_text(&self.formatrules.current_timestamp()),
                    },
                );
                continue;
            }

            let _ = self.snapshotdatabase.record_intended_file_copy(
                SnapshotDatabaseIntendedCopyRequest {
                    database: item.peer.peer.snapshot_database.clone(),
                    entry: identity.clone(),
                    mod_time: winning_mod_text.clone(),
                    byte_size: winning_size,
                },
            );
            let result = self.copystaging.copy_file(CopyStagingCopyRequest {
                options: self.default_copy_options(),
                source_peer: self.peer_for_copy(&primary_source.peer.peer),
                destination_peer: self.peer_for_copy(&item.peer.peer),
                source_path: path.to_string(),
                destination_path: path.to_string(),
                relative_path: path.to_string(),
                winning_mod_time,
                winning_byte_size: winning_size,
            });
            if matches!(
                result.status,
                CopyStagingCopyStatus::Completed | CopyStagingCopyStatus::PlannedDryRun
            ) {
                let _ = self.snapshotdatabase.record_completed_file_copy(
                    SnapshotDatabaseCompletedCopyRequest {
                        database: item.peer.peer.snapshot_database.clone(),
                        entry_id: identity.id.clone(),
                        last_seen: self
                            .formatrules
                            .timestamp_text(&self.formatrules.current_timestamp()),
                    },
                );
            }
        }
    }

    fn apply_directory_outcome(
        &self,
        path: &str,
        identity: &SnapshotDatabaseEntryIdentity,
        evidence: &[EntryEvidence],
    ) -> Vec<ActivePeer> {
        let mut recursion_peers = Vec::new();
        for item in evidence {
            if item
                .live
                .as_ref()
                .map(|entry| !entry.is_dir)
                .unwrap_or(false)
            {
                if !self.displace(path, identity, item, false) {
                    continue;
                }
            }

            if let Some(entry) = item.live.as_ref().filter(|entry| entry.is_dir) {
                let mod_time = self.formatrules.timestamp_text(
                    &self.timestamp_from_system_time(entry.mod_time),
                );
                let _ = self.snapshotdatabase.record_listed_directory(
                    SnapshotDatabaseListedDirectoryRequest {
                        database: item.peer.peer.snapshot_database.clone(),
                        entry: identity.clone(),
                        mod_time,
                        last_seen: self
                            .formatrules
                            .timestamp_text(&self.formatrules.current_timestamp()),
                    },
                );
                recursion_peers.push(item.peer.clone());
                continue;
            }

            if self
                .peertransportsurface
                .create_dir(&item.peer.peer.root, path)
                .is_ok()
            {
                let now = self.formatrules.current_timestamp();
                let now_text = self.formatrules.timestamp_text(&now);
                let _ = self.snapshotdatabase.record_created_directory(
                    SnapshotDatabaseCreatedDirectoryRequest {
                        database: item.peer.peer.snapshot_database.clone(),
                        entry: identity.clone(),
                        mod_time: now_text.clone(),
                        last_seen: now_text,
                    },
                );
                recursion_peers.push(item.peer.clone());
            }
        }
        recursion_peers
    }

    fn apply_absence_outcome(
        &self,
        path: &str,
        identity: &SnapshotDatabaseEntryIdentity,
        evidence: &[EntryEvidence],
    ) {
        for item in evidence {
            match &item.live {
                Some(entry) => {
                    let is_directory = entry.is_dir;
                    let displaced = self.displace(path, identity, item, is_directory);
                    if !displaced {
                        continue;
                    }
                }
                None => {}
            }
            let _ = self.snapshotdatabase.record_confirmed_absence(
                SnapshotDatabaseConfirmedAbsenceRequest {
                    database: item.peer.peer.snapshot_database.clone(),
                    entry_id: identity.id.clone(),
                },
            );
        }
    }

    fn displace(
        &self,
        path: &str,
        identity: &SnapshotDatabaseEntryIdentity,
        item: &EntryEvidence,
        is_directory: bool,
    ) -> bool {
        let result = self.copystaging.displace_to_bak(CopyStagingDisplacementRequest {
            options: self.default_copy_options(),
            peer: self.peer_for_copy(&item.peer.peer),
            relative_path: path.to_string(),
            is_directory,
        });
        if matches!(
            result.status,
            CopyStagingDisplacementStatus::Displaced
                | CopyStagingDisplacementStatus::PlannedDryRun
        ) {
            let _ = self.snapshotdatabase.record_successful_displacement(
                SnapshotDatabaseDisplacementRequest {
                    database: item.peer.peer.snapshot_database.clone(),
                    entry_id: identity.id.clone(),
                    is_directory,
                },
            );
            true
        } else {
            false
        }
    }
}

impl SyncTraversal for SyncTraversalImpl {
    fn traverse(&self, request: SyncTraversalRequest) -> SyncTraversalResult {
        let peers = request
            .peers
            .into_iter()
            .map(|peer| ActivePeer { peer })
            .collect::<Vec<_>>();
        let mut diagnostics = Vec::new();
        self.traverse_directory(
            peers,
            None,
            &request.excludes,
            request.retries_list,
            &mut diagnostics,
        );
        SyncTraversalResult { diagnostics }
    }
}

pub fn new(
    formatrules: Arc<dyn formatrules::FormatRules>,
    peertransportsurface: Arc<dyn peertransportsurface::PeerTransportSurface>,
    snapshotdatabase: Arc<dyn snapshotdatabase::SnapshotDatabase>,
    copystaging: Arc<dyn copystaging::CopyStaging>,
) -> Arc<dyn SyncTraversal> {
    Arc::new(SyncTraversalImpl {
        formatrules,
        peertransportsurface,
        snapshotdatabase,
        copystaging,
    })
}
