use std::io::{Read, Write};
use std::time::Duration;

use crate::{
    DiagnosticSink, EntryKind, EntryMeta, PeerId, PeerSession, ProgressEvent, ProgressSink,
    RelPath, RunConfig, Timestamp, TransferPhase, TransportError, TransportHandle,
};

const PURPOSE: &str = "Own peer-side mutation sequences other than abstract sync decision-making. The module composes connected transport operations into recoverable user-file SWAP replacement, inline displacement to nearby BAK, directory creation, traversal-time user-entry SWAP recovery, BAK and TMP retention cleanup, and dry-run suppression of peer-side mutations.\n\nThe module does not decide which paths should exist, schedule copy retries or active-copy slots, store snapshot data, connect peers, or render progress.";
const SUMMARY: &str = "operations: Own peer-side mutation sequences other than abstract sync decision-making. The module composes connected transport operations into recoverable user-file SWAP replacement, inline displacement to nearby BAK, directory creation, traversal-time user-entry SWAP recovery, BAK and TMP retention cleanup, and dry-run suppression of peer-side mutations.\n\nThe module does not decide which paths should exist, schedule copy retries or active-copy slots, store snapshot data, connect peers, or render progress.";
const BUFFER_SIZE: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationsStatus {
    pub name: &'static str,
    pub purpose: &'static str,
}

pub fn status() -> OperationsStatus {
    OperationsStatus {
        name: "operations",
        purpose: PURPOSE,
    }
}

pub fn summary() -> &'static str {
    SUMMARY
}

pub trait OperationExecutor {
    fn recover_directory_swaps(
        &self,
        peer: &PeerSession,
        directory: &RelPath,
    ) -> OperationResult<RecoveryReport>;

    fn displace_to_bak(
        &self,
        peer: &PeerSession,
        path: &RelPath,
        timestamp: Timestamp,
    ) -> OperationResult<DisplacementReport>;

    fn create_directory(
        &self,
        peer: &PeerSession,
        path: &RelPath,
    ) -> OperationResult<DirectoryCreationReport>;

    fn cleanup_retention(
        &self,
        peer: &PeerSession,
        directory: &RelPath,
        now: Timestamp,
        keep_bak_days: u32,
        keep_tmp_days: u32,
    ) -> OperationResult<CleanupReport>;

    fn execute_copy_attempt(
        &self,
        source_peer: &PeerSession,
        source_path: &RelPath,
        destination_peer: &PeerSession,
        destination_path: &RelPath,
        winning_meta: &EntryMeta,
    ) -> crate::CopyResult;
}

pub fn executor<'a>(
    config: &'a RunConfig,
    diagnostics: &'a dyn DiagnosticSink,
    progress: &'a dyn ProgressSink,
) -> DefaultOperationExecutor<'a> {
    DefaultOperationExecutor {
        config,
        diagnostics,
        progress,
    }
}

pub type OperationResult<T> = Result<T, OperationError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryReport {
    pub peer_id: PeerId,
    pub directory: RelPath,
    pub recovered_entries: u64,
    pub dry_run: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplacementReport {
    pub peer_id: PeerId,
    pub original_path: RelPath,
    pub bak_path: RelPath,
    pub dry_run: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryCreationReport {
    pub peer_id: PeerId,
    pub path: RelPath,
    pub dry_run: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupReport {
    pub peer_id: PeerId,
    pub directory: RelPath,
    pub removed_targets: Vec<CleanupTarget>,
    pub retained_targets: Vec<CleanupTarget>,
    pub nonfatal_failures: Vec<CleanupFailure>,
    pub dry_run: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupTarget {
    pub kind: CleanupTargetKind,
    pub path: RelPath,
    pub timestamp: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupTargetKind {
    Bak,
    Tmp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupFailure {
    pub target: Option<CleanupTarget>,
    pub error: TransportError,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationError {
    pub peer_id: PeerId,
    pub context: OperationErrorContext,
    pub error: TransportError,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationErrorContext {
    RecoverDirectorySwaps {
        directory: RelPath,
    },
    DisplaceToBak {
        path: RelPath,
    },
    CreateDirectory {
        path: RelPath,
    },
    CleanupRetention {
        directory: RelPath,
        target: Option<CleanupTarget>,
    },
}

pub struct DefaultOperationExecutor<'a> {
    config: &'a RunConfig,
    diagnostics: &'a dyn DiagnosticSink,
    progress: &'a dyn ProgressSink,
}

impl DefaultOperationExecutor<'_> {
    pub fn recover_directory_swaps(
        &self,
        peer: &PeerSession,
        directory: &RelPath,
    ) -> OperationResult<RecoveryReport> {
        OperationExecutor::recover_directory_swaps(self, peer, directory)
    }

    pub fn displace_to_bak(
        &self,
        peer: &PeerSession,
        path: &RelPath,
        timestamp: Timestamp,
    ) -> OperationResult<DisplacementReport> {
        OperationExecutor::displace_to_bak(self, peer, path, timestamp)
    }

    pub fn create_directory(
        &self,
        peer: &PeerSession,
        path: &RelPath,
    ) -> OperationResult<DirectoryCreationReport> {
        OperationExecutor::create_directory(self, peer, path)
    }

    pub fn cleanup_retention(
        &self,
        peer: &PeerSession,
        directory: &RelPath,
        now: Timestamp,
        keep_bak_days: u32,
        keep_tmp_days: u32,
    ) -> OperationResult<CleanupReport> {
        OperationExecutor::cleanup_retention(
            self,
            peer,
            directory,
            now,
            keep_bak_days,
            keep_tmp_days,
        )
    }

    pub fn execute_copy_attempt(
        &self,
        source_peer: &PeerSession,
        source_path: &RelPath,
        destination_peer: &PeerSession,
        destination_path: &RelPath,
        winning_meta: &EntryMeta,
    ) -> crate::CopyResult {
        OperationExecutor::execute_copy_attempt(
            self,
            source_peer,
            source_path,
            destination_peer,
            destination_path,
            winning_meta,
        )
    }
}

impl OperationExecutor for DefaultOperationExecutor<'_> {
    fn recover_directory_swaps(
        &self,
        peer: &PeerSession,
        directory: &RelPath,
    ) -> OperationResult<RecoveryReport> {
        let _ = (self.diagnostics, self.progress);
        if self.config.dry_run {
            return Ok(RecoveryReport {
                peer_id: peer.id,
                directory: directory.clone(),
                recovered_entries: 0,
                dry_run: true,
            });
        }

        let swap_root = child_path(directory, ".kitchensync/SWAP").map_err(|error| {
            op_error(
                peer.id,
                OperationErrorContext::RecoverDirectorySwaps {
                    directory: directory.clone(),
                },
                error,
            )
        })?;
        let entries = match peer.transport.list_dir(&swap_root) {
            Ok(entries) => entries,
            Err(TransportError::NotFound) => Vec::new(),
            Err(error) => {
                return Err(op_error(
                    peer.id,
                    OperationErrorContext::RecoverDirectorySwaps {
                        directory: directory.clone(),
                    },
                    error,
                ));
            }
        };

        let mut recovered = 0;
        for entry in entries {
            if entry.kind != EntryKind::Directory || entry.name == "snapshot.db" {
                continue;
            }
            let basename = match percent_decode_segment(&entry.name) {
                Some(value) => value,
                None => {
                    return Err(op_error(
                        peer.id,
                        OperationErrorContext::RecoverDirectorySwaps {
                            directory: directory.clone(),
                        },
                        TransportError::IoError,
                    ));
                }
            };
            recover_swap_for_parent(
                &peer.transport,
                Some(directory),
                &basename,
                fresh_timestamp(),
            )
            .map_err(|error| {
                op_error(
                    peer.id,
                    OperationErrorContext::RecoverDirectorySwaps {
                        directory: directory.clone(),
                    },
                    error,
                )
            })?;
            recovered += 1;
        }
        if peer.transport.delete_dir(&swap_root).is_ok() {
            let metadata_root = child_path(directory, ".kitchensync").map_err(|error| {
                op_error(
                    peer.id,
                    OperationErrorContext::RecoverDirectorySwaps {
                        directory: directory.clone(),
                    },
                    error,
                )
            })?;
            best_effort_remove_metadata_root(&peer.transport, &metadata_root);
        }

        Ok(RecoveryReport {
            peer_id: peer.id,
            directory: directory.clone(),
            recovered_entries: recovered,
            dry_run: false,
        })
    }

    fn displace_to_bak(
        &self,
        peer: &PeerSession,
        path: &RelPath,
        timestamp: Timestamp,
    ) -> OperationResult<DisplacementReport> {
        let (parent, basename) = split_parent_basename(path);
        let bak_dir = bak_timestamp_dir(parent.as_ref(), &timestamp).map_err(|error| {
            op_error(
                peer.id,
                OperationErrorContext::DisplaceToBak { path: path.clone() },
                error,
            )
        })?;
        let bak_path = append_segment(&bak_dir, &basename).map_err(|error| {
            op_error(
                peer.id,
                OperationErrorContext::DisplaceToBak { path: path.clone() },
                error,
            )
        })?;

        if self.config.dry_run {
            return Ok(DisplacementReport {
                peer_id: peer.id,
                original_path: path.clone(),
                bak_path,
                dry_run: true,
            });
        }

        peer.transport
            .create_dir(&bak_dir)
            .and_then(|()| peer.transport.rename_no_overwrite(path, &bak_path))
            .map_err(|error| {
                op_error(
                    peer.id,
                    OperationErrorContext::DisplaceToBak { path: path.clone() },
                    error,
                )
            })?;

        Ok(DisplacementReport {
            peer_id: peer.id,
            original_path: path.clone(),
            bak_path,
            dry_run: false,
        })
    }

    fn create_directory(
        &self,
        peer: &PeerSession,
        path: &RelPath,
    ) -> OperationResult<DirectoryCreationReport> {
        if !self.config.dry_run {
            peer.transport.create_dir(path).map_err(|error| {
                op_error(
                    peer.id,
                    OperationErrorContext::CreateDirectory { path: path.clone() },
                    error,
                )
            })?;
        }

        Ok(DirectoryCreationReport {
            peer_id: peer.id,
            path: path.clone(),
            dry_run: self.config.dry_run,
        })
    }

    fn cleanup_retention(
        &self,
        peer: &PeerSession,
        directory: &RelPath,
        now: Timestamp,
        keep_bak_days: u32,
        keep_tmp_days: u32,
    ) -> OperationResult<CleanupReport> {
        let mut report = CleanupReport {
            peer_id: peer.id,
            directory: directory.clone(),
            removed_targets: Vec::new(),
            retained_targets: Vec::new(),
            nonfatal_failures: Vec::new(),
            dry_run: self.config.dry_run,
        };

        if self.config.dry_run {
            return Ok(report);
        }

        cleanup_kind(
            &peer.transport,
            directory,
            &now,
            keep_bak_days,
            CleanupTargetKind::Bak,
            false,
            &mut report,
        );
        cleanup_kind(
            &peer.transport,
            directory,
            &now,
            keep_tmp_days,
            CleanupTargetKind::Tmp,
            false,
            &mut report,
        );

        Ok(report)
    }

    fn execute_copy_attempt(
        &self,
        source_peer: &PeerSession,
        source_path: &RelPath,
        destination_peer: &PeerSession,
        destination_path: &RelPath,
        winning_meta: &EntryMeta,
    ) -> crate::CopyResult {
        execute_copy(
            self.config.dry_run,
            self.progress,
            source_peer,
            source_path,
            destination_peer,
            destination_path,
            winning_meta,
        )
    }
}

fn execute_copy(
    dry_run: bool,
    progress: &dyn ProgressSink,
    source_peer: &PeerSession,
    source_path: &RelPath,
    destination_peer: &PeerSession,
    destination_path: &RelPath,
    winning_meta: &EntryMeta,
) -> crate::CopyResult {
    let mut result = crate::CopyResult {
        source_peer_id: source_peer.id,
        source_path: source_path.clone(),
        destination_peer_id: destination_peer.id,
        destination_path: destination_path.clone(),
        bytes_copied: 0,
        completed: false,
        failed_phase: None,
        error: None,
    };

    let progress_key = copy_progress_key(destination_peer, destination_path);
    let basename = split_parent_basename(destination_path).1;
    let total_bytes = (winning_meta.kind == EntryKind::File && winning_meta.byte_size >= 0)
        .then_some(winning_meta.byte_size as u64);
    progress.publish(ProgressEvent::CopyStarted {
        destination: progress_key.clone(),
        basename: basename.clone(),
        total_bytes,
    });

    let mut reader = match source_peer.transport.open_read(source_path) {
        Ok(reader) => reader,
        Err(error) => {
            progress.publish(ProgressEvent::CopyRemoved {
                destination: progress_key,
            });
            return failed(result, TransferPhase::ReadSource, error);
        }
    };

    if dry_run {
        return match drain_reader(&mut reader, progress, &progress_key, &basename, total_bytes) {
            Ok(bytes) => {
                result.bytes_copied = bytes;
                result.completed = true;
                progress.publish(ProgressEvent::CopyFinished {
                    destination: progress_key,
                });
                result
            }
            Err(error) => {
                progress.publish(ProgressEvent::CopyRemoved {
                    destination: progress_key,
                });
                failed(result, TransferPhase::ReadSource, error)
            }
        };
    }

    let (parent, basename) = split_parent_basename(destination_path);
    if let Err(error) = recover_swap_for_parent(
        &destination_peer.transport,
        parent.as_ref(),
        &basename,
        fresh_timestamp(),
    ) {
        progress.publish(ProgressEvent::CopyRemoved {
            destination: progress_key,
        });
        return failed(result, TransferPhase::WriteSwapNew, error);
    }

    let swap = match swap_paths(parent.as_ref(), &basename) {
        Ok(paths) => paths,
        Err(error) => {
            progress.publish(ProgressEvent::CopyRemoved {
                destination: progress_key,
            });
            return failed(result, TransferPhase::WriteSwapNew, error);
        }
    };

    let mut writer = match destination_peer.transport.open_write(&swap.new_path) {
        Ok(writer) => writer,
        Err(error) => {
            progress.publish(ProgressEvent::CopyRemoved {
                destination: progress_key,
            });
            return failed(result, TransferPhase::WriteSwapNew, error);
        }
    };

    match copy_stream(
        &mut reader,
        &mut writer,
        progress,
        &progress_key,
        &basename,
        total_bytes,
    ) {
        Ok(bytes) => result.bytes_copied = bytes,
        Err((phase, error)) => {
            best_effort_remove_swap_new(&destination_peer.transport, &swap);
            progress.publish(ProgressEvent::CopyRemoved {
                destination: progress_key,
            });
            return failed(result, phase, error);
        }
    }

    if let Err(error) = writer.close() {
        best_effort_remove_swap_new(&destination_peer.transport, &swap);
        progress.publish(ProgressEvent::CopyRemoved {
            destination: progress_key,
        });
        return failed(result, TransferPhase::WriteSwapNew, error);
    }

    let destination_exists = match destination_peer.transport.stat(destination_path) {
        Ok(meta) => meta.kind == EntryKind::File,
        Err(TransportError::NotFound) => false,
        Err(error) => {
            best_effort_remove_swap_new(&destination_peer.transport, &swap);
            progress.publish(ProgressEvent::CopyRemoved {
                destination: progress_key,
            });
            return failed(result, TransferPhase::MoveExistingToSwapOld, error);
        }
    };

    if destination_exists {
        if let Err(error) = destination_peer
            .transport
            .rename_no_overwrite(destination_path, &swap.old_path)
        {
            best_effort_remove_swap_new(&destination_peer.transport, &swap);
            progress.publish(ProgressEvent::CopyRemoved {
                destination: progress_key,
            });
            return failed(result, TransferPhase::MoveExistingToSwapOld, error);
        }
    }

    if let Err(error) = destination_peer
        .transport
        .rename_no_overwrite(&swap.new_path, destination_path)
    {
        progress.publish(ProgressEvent::CopyRemoved {
            destination: progress_key,
        });
        return failed(result, TransferPhase::RenameFinal, error);
    }

    result.completed = true;
    progress.publish(ProgressEvent::CopyFinished {
        destination: progress_key.clone(),
    });

    if let Err(error) = destination_peer
        .transport
        .set_mod_time(destination_path, winning_meta.mod_time.clone())
    {
        return failed(result, TransferPhase::SetModTime, error);
    }

    if destination_exists {
        let timestamp = fresh_timestamp();
        let bak_dir = match bak_timestamp_dir(parent.as_ref(), &timestamp) {
            Ok(path) => path,
            Err(error) => return failed(result, TransferPhase::ArchiveOld, error),
        };
        let bak_path = match append_segment(&bak_dir, &basename) {
            Ok(path) => path,
            Err(error) => return failed(result, TransferPhase::ArchiveOld, error),
        };
        if let Err(error) = destination_peer
            .transport
            .create_dir(&bak_dir)
            .and_then(|()| {
                destination_peer
                    .transport
                    .rename_no_overwrite(&swap.old_path, &bak_path)
            })
        {
            return failed(result, TransferPhase::ArchiveOld, error);
        }
    }

    if let Err(error) = cleanup_swap_dirs(&destination_peer.transport, &swap) {
        return failed(result, TransferPhase::Cleanup, error);
    }

    result
}

fn failed(
    mut result: crate::CopyResult,
    phase: TransferPhase,
    error: TransportError,
) -> crate::CopyResult {
    result.failed_phase = Some(phase);
    result.error = Some(error);
    result
}

fn copy_stream(
    reader: &mut dyn Read,
    writer: &mut dyn Write,
    progress: &dyn ProgressSink,
    progress_key: &str,
    basename: &str,
    total_bytes: Option<u64>,
) -> Result<u64, (TransferPhase, TransportError)> {
    let mut buffer = [0_u8; BUFFER_SIZE];
    let mut bytes = 0;
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|_| (TransferPhase::ReadSource, TransportError::IoError))?;
        if read == 0 {
            break;
        }
        writer
            .write_all(&buffer[..read])
            .map_err(|_| (TransferPhase::WriteSwapNew, TransportError::IoError))?;
        bytes += read as u64;
        progress.publish(ProgressEvent::CopyProgress {
            destination: progress_key.to_string(),
            basename: basename.to_string(),
            transferred_bytes: bytes,
            total_bytes,
        });
    }
    Ok(bytes)
}

fn drain_reader(
    reader: &mut dyn Read,
    progress: &dyn ProgressSink,
    progress_key: &str,
    basename: &str,
    total_bytes: Option<u64>,
) -> Result<u64, TransportError> {
    let mut buffer = [0_u8; BUFFER_SIZE];
    let mut bytes = 0;
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|_| TransportError::IoError)?;
        if read == 0 {
            break;
        }
        bytes += read as u64;
        progress.publish(ProgressEvent::CopyProgress {
            destination: progress_key.to_string(),
            basename: basename.to_string(),
            transferred_bytes: bytes,
            total_bytes,
        });
    }
    Ok(bytes)
}

fn copy_progress_key(peer: &PeerSession, path: &RelPath) -> String {
    format!("{}:{}", peer.normalized_identity.identity, path.as_str())
}

fn recover_swap_for_parent(
    transport: &TransportHandle,
    parent: Option<&RelPath>,
    basename: &str,
    timestamp: Timestamp,
) -> Result<(), TransportError> {
    let (old_path, new_path, target_path) = recovery_paths(parent, basename)?;
    let old = exists(transport, &old_path)?;
    let new = exists(transport, &new_path)?;
    let target = exists(transport, &target_path)?;

    match (old, new, target) {
        (true, true, true) => {
            archive_swap_old(transport, parent, basename, &old_path, timestamp)?;
            transport.delete_file(&new_path)?;
        }
        (true, false, true) => archive_swap_old(transport, parent, basename, &old_path, timestamp)?,
        (true, true, false) => {
            transport.rename_no_overwrite(&new_path, &target_path)?;
            archive_swap_old(transport, parent, basename, &old_path, timestamp)?;
        }
        (true, false, false) => transport.rename_no_overwrite(&old_path, &target_path)?,
        (false, true, true) => transport.delete_file(&new_path)?,
        (false, true, false) => transport.rename_no_overwrite(&new_path, &target_path)?,
        (false, false, _) => {}
    }

    let swap = swap_paths(parent, basename)?;
    cleanup_swap_entry_dir(transport, &swap)
}

fn archive_swap_old(
    transport: &TransportHandle,
    parent: Option<&RelPath>,
    basename: &str,
    old_path: &RelPath,
    timestamp: Timestamp,
) -> Result<(), TransportError> {
    let bak_dir = bak_timestamp_dir(parent, &timestamp)?;
    let bak_path = append_segment(&bak_dir, basename)?;
    transport.create_dir(&bak_dir)?;
    transport.rename_no_overwrite(old_path, &bak_path)
}

fn cleanup_kind(
    transport: &TransportHandle,
    directory: &RelPath,
    now: &Timestamp,
    keep_days: u32,
    kind: CleanupTargetKind,
    dry_run: bool,
    report: &mut CleanupReport,
) {
    let root = match cleanup_root(directory, kind) {
        Ok(path) => path,
        Err(error) => {
            report.nonfatal_failures.push(CleanupFailure {
                target: None,
                error,
            });
            return;
        }
    };
    match transport.stat(&root) {
        Ok(meta) if meta.kind == EntryKind::Directory => {}
        Ok(_) => {
            report.nonfatal_failures.push(CleanupFailure {
                target: None,
                error: TransportError::IoError,
            });
            return;
        }
        Err(TransportError::NotFound) => return,
        Err(error) => {
            report.nonfatal_failures.push(CleanupFailure {
                target: None,
                error,
            });
            return;
        }
    }
    let entries = match transport.list_dir(&root) {
        Ok(entries) => entries,
        Err(error) => {
            report.nonfatal_failures.push(CleanupFailure {
                target: None,
                error,
            });
            return;
        }
    };

    for entry in entries {
        if entry.kind != EntryKind::Directory {
            continue;
        }
        let timestamp = Timestamp(entry.name.clone());
        if parse_timestamp(&timestamp.0).is_none() {
            continue;
        }
        let target_path = match append_segment(&root, &entry.name) {
            Ok(path) => path,
            Err(error) => {
                report.nonfatal_failures.push(CleanupFailure {
                    target: None,
                    error,
                });
                continue;
            }
        };
        let target = CleanupTarget {
            kind,
            path: target_path,
            timestamp,
        };

        if timestamp_is_expired(&target.timestamp, now, keep_days) {
            if dry_run {
                report.removed_targets.push(target);
                continue;
            }
            match delete_tree(transport, &target.path) {
                Ok(()) => report.removed_targets.push(target),
                Err(error) => report.nonfatal_failures.push(CleanupFailure {
                    target: Some(target),
                    error,
                }),
            }
        } else {
            report.retained_targets.push(target);
        }
    }
}

fn delete_tree(transport: &TransportHandle, path: &RelPath) -> Result<(), TransportError> {
    match transport.stat(path)? {
        meta if meta.kind == EntryKind::File => transport.delete_file(path),
        _ => {
            for child in transport.list_dir(path)? {
                let child_path = append_segment(path, &child.name)?;
                delete_tree(transport, &child_path)?;
            }
            transport.delete_dir(path)
        }
    }
}

fn exists(transport: &TransportHandle, path: &RelPath) -> Result<bool, TransportError> {
    match transport.stat(path) {
        Ok(_) => Ok(true),
        Err(TransportError::NotFound) => Ok(false),
        Err(error) => Err(error),
    }
}

fn best_effort_remove_swap_new(transport: &TransportHandle, swap: &SwapPaths) {
    let _ = transport.delete_file(&swap.new_path);
    let _ = cleanup_swap_dirs(transport, swap);
}

fn cleanup_swap_dirs(transport: &TransportHandle, swap: &SwapPaths) -> Result<(), TransportError> {
    cleanup_swap_entry_dir(transport, swap)?;
    match transport.list_dir(&swap.swap_root) {
        Ok(entries) if entries.is_empty() => match transport.delete_dir(&swap.swap_root) {
            Ok(()) => {
                best_effort_remove_metadata_root(transport, &swap.metadata_root);
                Ok(())
            }
            Err(TransportError::NotFound) => Ok(()),
            Err(error) => Err(error),
        },
        Ok(_) | Err(TransportError::NotFound) => Ok(()),
        Err(error) => Err(error),
    }
}

fn best_effort_remove_metadata_root(transport: &TransportHandle, metadata_root: &RelPath) {
    let _ = transport.delete_dir(metadata_root);
}

fn cleanup_swap_entry_dir(
    transport: &TransportHandle,
    swap: &SwapPaths,
) -> Result<(), TransportError> {
    match transport.delete_dir(&swap.entry_dir) {
        Ok(()) | Err(TransportError::NotFound) => Ok(()),
        Err(error) => Err(error),
    }
}

struct SwapPaths {
    metadata_root: RelPath,
    swap_root: RelPath,
    entry_dir: RelPath,
    new_path: RelPath,
    old_path: RelPath,
}

fn swap_paths(parent: Option<&RelPath>, basename: &str) -> Result<SwapPaths, TransportError> {
    let encoded = percent_encode_segment(basename);
    let metadata_root = child_path_opt(parent, ".kitchensync")?;
    let swap_root = child_path_opt(parent, ".kitchensync/SWAP")?;
    let entry_dir = append_segment(&swap_root, &encoded)?;
    let new_path = append_segment(&entry_dir, "new")?;
    let old_path = append_segment(&entry_dir, "old")?;
    Ok(SwapPaths {
        metadata_root,
        swap_root,
        entry_dir,
        new_path,
        old_path,
    })
}

fn recovery_paths(
    parent: Option<&RelPath>,
    basename: &str,
) -> Result<(RelPath, RelPath, RelPath), TransportError> {
    let swap = swap_paths(parent, basename)?;
    let target = child_path_opt(parent, basename)?;
    Ok((swap.old_path, swap.new_path, target))
}

fn cleanup_root(directory: &RelPath, kind: CleanupTargetKind) -> Result<RelPath, TransportError> {
    match kind {
        CleanupTargetKind::Bak => child_path(directory, ".kitchensync/BAK"),
        CleanupTargetKind::Tmp => child_path(directory, ".kitchensync/TMP"),
    }
}

fn bak_timestamp_dir(
    parent: Option<&RelPath>,
    timestamp: &Timestamp,
) -> Result<RelPath, TransportError> {
    child_path_opt(parent, &format!(".kitchensync/BAK/{}", timestamp.0))
}

fn split_parent_basename(path: &RelPath) -> (Option<RelPath>, String) {
    match path.as_str().rsplit_once('/') {
        Some((parent, basename)) => (RelPath::new(parent.to_string()).ok(), basename.to_string()),
        None => (None, path.as_str().to_string()),
    }
}

fn child_path(parent: &RelPath, child: &str) -> Result<RelPath, TransportError> {
    child_path_opt(Some(parent), child)
}

fn child_path_opt(parent: Option<&RelPath>, child: &str) -> Result<RelPath, TransportError> {
    let value = match parent {
        Some(parent) if !parent.as_str().is_empty() => format!("{}/{}", parent.as_str(), child),
        None => child.to_string(),
        Some(_) => child.to_string(),
    };
    RelPath::new(value).map_err(|_| TransportError::IoError)
}

fn append_segment(parent: &RelPath, segment: &str) -> Result<RelPath, TransportError> {
    if segment.contains('/') || segment.is_empty() {
        return Err(TransportError::IoError);
    }
    child_path(parent, segment)
}

fn op_error(
    peer_id: PeerId,
    context: OperationErrorContext,
    error: TransportError,
) -> OperationError {
    OperationError {
        peer_id,
        context,
        error,
    }
}

fn percent_encode_segment(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-' => {
                encoded.push(byte as char)
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

fn percent_decode_segment(value: &str) -> Option<String> {
    let mut bytes = Vec::new();
    let mut chars = value.as_bytes().iter().copied();
    while let Some(byte) = chars.next() {
        if byte == b'%' {
            let hi = hex_value(chars.next()?)?;
            let lo = hex_value(chars.next()?)?;
            bytes.push((hi << 4) | lo);
        } else {
            bytes.push(byte);
        }
    }
    String::from_utf8(bytes).ok()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn timestamp_is_expired(timestamp: &Timestamp, now: &Timestamp, keep_days: u32) -> bool {
    match (parse_timestamp(&timestamp.0), parse_timestamp(&now.0)) {
        (Some(then), Some(now)) => {
            now.saturating_sub(then) > Duration::from_secs(keep_days as u64 * 86_400)
        }
        _ => false,
    }
}

fn parse_timestamp(value: &str) -> Option<Duration> {
    if value.len() != 27 {
        return None;
    }
    let (date, rest) = value.split_once('_')?;
    let (time, fraction_z) = rest.rsplit_once('_')?;
    let fraction = fraction_z.strip_suffix('Z')?;
    let mut date_parts = date.split('-');
    let year: i64 = date_parts.next()?.parse().ok()?;
    let month: i64 = date_parts.next()?.parse().ok()?;
    let day: i64 = date_parts.next()?.parse().ok()?;
    if date_parts.next().is_some() {
        return None;
    }
    let mut time_parts = time.split('-');
    let hour: i64 = time_parts.next()?.parse().ok()?;
    let minute: i64 = time_parts.next()?.parse().ok()?;
    let second: i64 = time_parts.next()?.parse().ok()?;
    if time_parts.next().is_some() {
        return None;
    }
    let micros: u64 = fraction.parse().ok()?;
    if hour < 0
        || hour > 23
        || minute < 0
        || minute > 59
        || second < 0
        || second > 60
        || micros > 999_999
        || day > days_in_month(year, month)?
    {
        return None;
    }
    let days = days_from_civil(year, month, day)?;
    let secs = days * 86_400 + hour * 3_600 + minute * 60 + second;
    if secs < 0 {
        return None;
    }
    Some(Duration::from_secs(secs as u64) + Duration::from_micros(micros))
}

fn days_in_month(year: i64, month: i64) -> Option<i64> {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => Some(31),
        4 | 6 | 9 | 11 => Some(30),
        2 if is_leap_year(year) => Some(29),
        2 => Some(28),
        _ => None,
    }
}

fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
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

fn fresh_timestamp() -> Timestamp {
    crate::snapshot::fresh_timestamp()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_names_module() {
        assert_eq!(status().name, "operations");
        assert!(summary().contains("operations"));
    }

    #[test]
    fn percent_roundtrip_preserves_segment_text() {
        let source = "space and #";
        let encoded = percent_encode_segment(source);
        assert_eq!(percent_decode_segment(&encoded).as_deref(), Some(source));
    }
}
