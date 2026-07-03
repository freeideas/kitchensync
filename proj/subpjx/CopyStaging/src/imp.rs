use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, SystemTime};

use crate::api::*;
use peertransportsurface::{PeerReadChunk, PeerTransportError};

const COPY_BUFFER_BYTES: usize = 64 * 1024;

struct CopyStagingImpl {
    formatrules: Arc<dyn formatrules::FormatRules>,
    peertransportsurface: Arc<dyn peertransportsurface::PeerTransportSurface>,
    copy_slots: Arc<CopySlotState>,
}

struct CopySlotState {
    active: Mutex<u64>,
    changed: Condvar,
}

impl CopySlotState {
    fn acquire(&self, max_copies: u64) -> u64 {
        let max_copies = max_copies.max(1);
        let mut active = self.active.lock().expect("copy slot mutex poisoned");
        while *active >= max_copies {
            active = self
                .changed
                .wait(active)
                .expect("copy slot mutex poisoned");
        }
        *active += 1;
        *active
    }

    fn release(&self) -> u64 {
        let mut active = self.active.lock().expect("copy slot mutex poisoned");
        *active = active.saturating_sub(1);
        let value = *active;
        self.changed.notify_all();
        value
    }
}

impl CopyStagingImpl {
    fn copy_attempt(
        &self,
        request: &CopyStagingCopyRequest,
    ) -> Result<(), (CopyStagingFailurePhase, Option<PeerTransportError>, bool)> {
        let (parent, basename) = split_parent_basename(&request.destination_path);
        let swap = self
            .formatrules
            .user_swap_paths(parent.as_deref(), &basename)
            .map_err(|_| (CopyStagingFailurePhase::Cleanup, None, false))?;

        self.recover_named_swap(&request.destination_peer, parent.as_deref(), &basename)
            .map_err(|error| (CopyStagingFailurePhase::Cleanup, Some(error), false))?;

        let mut read = self
            .peertransportsurface
            .open_read(&request.source_peer.root, &request.source_path)
            .map_err(|error| (CopyStagingFailurePhase::ReadSource, Some(error), false))?;

        let mut write = self
            .peertransportsurface
            .open_write(&request.destination_peer.root, &swap.new_path)
            .map_err(|error| (CopyStagingFailurePhase::WriteSwapNew, Some(error), false))?;

        loop {
            match self
                .peertransportsurface
                .read(&mut read, COPY_BUFFER_BYTES)
                .map_err(|error| (CopyStagingFailurePhase::ReadSource, Some(error), false))?
            {
                PeerReadChunk::Bytes(bytes) => self
                    .peertransportsurface
                    .write(&mut write, &bytes)
                    .map_err(|error| (CopyStagingFailurePhase::WriteSwapNew, Some(error), false))?,
                PeerReadChunk::Eof => break,
            }
        }

        self.peertransportsurface
            .close_read(read)
            .map_err(|error| (CopyStagingFailurePhase::ReadSource, Some(error), false))?;
        self.peertransportsurface
            .close_write(write)
            .map_err(|error| (CopyStagingFailurePhase::WriteSwapNew, Some(error), false))?;

        let live_exists = match self.path_exists(&request.destination_peer, &request.destination_path) {
            Ok(value) => value,
            Err(error) => {
                let _ = self
                    .peertransportsurface
                    .delete_file(&request.destination_peer.root, &swap.new_path);
                return Err((CopyStagingFailurePhase::MoveExistingToSwapOld, Some(error), false));
            }
        };

        let mut old_exists = false;
        if live_exists {
            if let Err(error) = self.peertransportsurface.rename(
                &request.destination_peer.root,
                &request.destination_path,
                &swap.old_path,
            ) {
                let _ = self
                    .peertransportsurface
                    .delete_file(&request.destination_peer.root, &swap.new_path);
                return Err((
                    CopyStagingFailurePhase::MoveExistingToSwapOld,
                    Some(error),
                    false,
                ));
            }
            old_exists = true;
        }

        if let Err(error) = self.peertransportsurface.rename(
            &request.destination_peer.root,
            &swap.new_path,
            &request.destination_path,
        ) {
            if !old_exists {
                let _ = self
                    .peertransportsurface
                    .delete_file(&request.destination_peer.root, &swap.new_path);
            }
            return Err((CopyStagingFailurePhase::RenameFinal, Some(error), old_exists));
        }

        self.peertransportsurface
            .set_mod_time(
                &request.destination_peer.root,
                &request.destination_path,
                request.winning_mod_time,
            )
            .map_err(|error| (CopyStagingFailurePhase::SetModTime, Some(error), true))?;

        if old_exists {
            self.archive_swap_old(&request.destination_peer, parent.as_deref(), &basename, &swap.old_path)
                .map_err(|error| (CopyStagingFailurePhase::ArchiveOld, Some(error), true))?;
        }

        self.peertransportsurface
            .delete_dir(&request.destination_peer.root, &swap.directory_path)
            .map_err(|error| (CopyStagingFailurePhase::Cleanup, Some(error), true))?;
        Ok(())
    }

    fn dry_run_read_source(
        &self,
        request: &CopyStagingCopyRequest,
    ) -> Result<(), (CopyStagingFailurePhase, Option<PeerTransportError>, bool)> {
        let mut read = self
            .peertransportsurface
            .open_read(&request.source_peer.root, &request.source_path)
            .map_err(|error| (CopyStagingFailurePhase::ReadSource, Some(error), false))?;
        loop {
            match self
                .peertransportsurface
                .read(&mut read, COPY_BUFFER_BYTES)
                .map_err(|error| (CopyStagingFailurePhase::ReadSource, Some(error), false))?
            {
                PeerReadChunk::Bytes(_) => {}
                PeerReadChunk::Eof => break,
            }
        }
        self.peertransportsurface
            .close_read(read)
            .map_err(|error| (CopyStagingFailurePhase::ReadSource, Some(error), false))
    }

    fn path_exists(
        &self,
        peer: &CopyStagingPeer,
        path: &str,
    ) -> Result<bool, PeerTransportError> {
        match self.peertransportsurface.stat(&peer.root, path) {
            Ok(_) => Ok(true),
            Err(PeerTransportError::NotFound) => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn archive_swap_old(
        &self,
        peer: &CopyStagingPeer,
        parent: Option<&str>,
        basename: &str,
        old_path: &str,
    ) -> Result<(), PeerTransportError> {
        let timestamp = self.formatrules.current_timestamp();
        let bak_dir = self
            .formatrules
            .bak_directory_path(parent, &timestamp)
            .map_err(|_| PeerTransportError::IoError)?;
        self.peertransportsurface.create_dir(&peer.root, &bak_dir)?;
        self.peertransportsurface
            .rename(&peer.root, old_path, &format!("{bak_dir}{basename}"))
    }

    fn recover_named_swap(
        &self,
        peer: &CopyStagingPeer,
        parent: Option<&str>,
        basename: &str,
    ) -> Result<(), PeerTransportError> {
        let swap = self
            .formatrules
            .user_swap_paths(parent, basename)
            .map_err(|_| PeerTransportError::IoError)?;
        let live_path = join_parent_basename(parent, basename);
        let old_exists = self.path_exists(peer, &swap.old_path)?;
        let new_exists = self.path_exists(peer, &swap.new_path)?;
        let live_exists = self.path_exists(peer, &live_path)?;

        match (old_exists, new_exists, live_exists) {
            (true, true, true) => {
                self.peertransportsurface.delete_file(&peer.root, &swap.new_path)?;
                self.archive_swap_old(peer, parent, basename, &swap.old_path)?;
            }
            (true, false, true) => {
                self.archive_swap_old(peer, parent, basename, &swap.old_path)?;
            }
            (true, true, false) => {
                self.peertransportsurface
                    .rename(&peer.root, &swap.new_path, &live_path)?;
                self.archive_swap_old(peer, parent, basename, &swap.old_path)?;
            }
            (true, false, false) => {
                self.peertransportsurface
                    .rename(&peer.root, &swap.old_path, &live_path)?;
            }
            (false, true, true) => {
                self.peertransportsurface.delete_file(&peer.root, &swap.new_path)?;
            }
            (false, true, false) => {
                self.peertransportsurface
                    .rename(&peer.root, &swap.new_path, &live_path)?;
            }
            (false, false, _) => {}
        }

        match self.peertransportsurface.delete_dir(&peer.root, &swap.directory_path) {
            Ok(()) | Err(PeerTransportError::NotFound) => Ok(()),
            Err(error) => Err(error),
        }
    }

    fn recursive_delete(&self, peer: &CopyStagingPeer, path: &str) -> Result<(), PeerTransportError> {
        let metadata = self.peertransportsurface.stat(&peer.root, path)?;
        if metadata.is_dir {
            for entry in self.peertransportsurface.list_dir(&peer.root, path)? {
                self.recursive_delete(peer, &join_path(path, &entry.child_name))?;
            }
            self.peertransportsurface.delete_dir(&peer.root, path)
        } else {
            self.peertransportsurface.delete_file(&peer.root, path)
        }
    }

    fn cleanup_timestamp_dir(
        &self,
        peer: &CopyStagingPeer,
        root_path: &str,
        keep_days: u64,
        now: SystemTime,
    ) -> Result<(), PeerTransportError> {
        let entries = match self.peertransportsurface.list_dir(&peer.root, root_path) {
            Ok(entries) => entries,
            Err(PeerTransportError::NotFound) => return Ok(()),
            Err(error) => return Err(error),
        };

        for entry in entries {
            if !entry.is_dir {
                continue;
            }
            let Ok(timestamp) = self.formatrules.parse_timestamp(&entry.child_name) else {
                continue;
            };
            let created = self.formatrules.timestamp_system_time(&timestamp);
            let age = now.duration_since(created).unwrap_or(Duration::ZERO);
            if age > Duration::from_secs(keep_days.saturating_mul(24 * 60 * 60)) {
                self.recursive_delete(peer, &join_path(root_path, &entry.child_name))?;
            }
        }
        Ok(())
    }

    fn cleanup_copy_swap_new(&self, request: &CopyStagingCopyRequest) {
        let (parent, basename) = split_parent_basename(&request.destination_path);
        let Ok(swap) = self
            .formatrules
            .user_swap_paths(parent.as_deref(), &basename)
        else {
            return;
        };
        let _ = self
            .peertransportsurface
            .delete_file(&request.destination_peer.root, &swap.new_path);
    }
}

impl CopyStaging for CopyStagingImpl {
    fn copy_file(&self, request: CopyStagingCopyRequest) -> CopyStagingCopyResult {
        let mut output_lines = Vec::new();
        let mut attempts = 0;
        let max_attempts = request.options.retries_copy.max(1);

        while attempts < max_attempts {
            attempts += 1;
            let active = self.copy_slots.acquire(request.options.max_copies);
            if request.options.verbosity == CopyStagingVerbosity::Trace {
                output_lines.push(format!(
                    "copy-slots active={}/{}",
                    active,
                    request.options.max_copies.max(1)
                ));
            }

            let attempt = if request.options.mode == CopyStagingRunMode::DryRun {
                self.dry_run_read_source(&request)
            } else {
                self.copy_attempt(&request)
            };

            let active = self.copy_slots.release();
            if request.options.verbosity == CopyStagingVerbosity::Trace {
                output_lines.push(format!(
                    "copy-slots active={}/{}",
                    active,
                    request.options.max_copies.max(1)
                ));
            }

            match attempt {
                Ok(()) => {
                    push_copy_progress(&mut output_lines, request.options.verbosity, &request.relative_path);
                    return CopyStagingCopyResult {
                        destination_peer_index: request.destination_peer.peer_index,
                        destination_peer_url: request.destination_peer.peer_url,
                        relative_path: request.relative_path,
                        status: if request.options.mode == CopyStagingRunMode::DryRun {
                            CopyStagingCopyStatus::PlannedDryRun
                        } else {
                            CopyStagingCopyStatus::Completed
                        },
                        attempts,
                        output_lines,
                        diagnostics: Vec::new(),
                    };
                }
                Err((phase, error, no_retry)) if no_retry => {
                    return failed_copy(request, attempts, output_lines, phase, error, false);
                }
                Err((phase, error, _)) if attempts >= max_attempts => {
                    self.cleanup_copy_swap_new(&request);
                    return failed_copy(request, attempts, output_lines, phase, error, true);
                }
                Err(_) => {
                    self.cleanup_copy_swap_new(&request);
                }
            }
        }

        failed_copy(
            request,
            attempts,
            output_lines,
            CopyStagingFailurePhase::ReadSource,
            None,
            true,
        )
    }

    fn recover_user_swap(
        &self,
        request: CopyStagingDirectoryRequest,
    ) -> CopyStagingSwapRecoveryResult {
        if request.options.mode == CopyStagingRunMode::DryRun {
            return CopyStagingSwapRecoveryResult {
                peer_index: request.peer.peer_index,
                directory_relative_path: request.directory_relative_path,
                status: CopyStagingSwapRecoveryStatus::SkippedDryRun,
                output_lines: Vec::new(),
                diagnostics: Vec::new(),
            };
        }

        let swap_root = metadata_child_path(request.directory_relative_path.as_deref(), "SWAP");
        let entries = match self.peertransportsurface.list_dir(&request.peer.root, &swap_root) {
            Ok(entries) => entries,
            Err(PeerTransportError::NotFound) => {
                return CopyStagingSwapRecoveryResult {
                    peer_index: request.peer.peer_index,
                    directory_relative_path: request.directory_relative_path,
                    status: CopyStagingSwapRecoveryStatus::Recovered,
                    output_lines: Vec::new(),
                    diagnostics: Vec::new(),
                };
            }
            Err(error) => {
                return failed_recovery(request, Some(error));
            }
        };

        for entry in entries {
            if !entry.is_dir {
                continue;
            }
            let Some(basename) = percent_decode_segment(&entry.child_name) else {
                return failed_recovery(request, None);
            };
            if let Err(error) = self.recover_named_swap(
                &request.peer,
                request.directory_relative_path.as_deref(),
                &basename,
            ) {
                return failed_recovery(request, Some(error));
            }
        }

        CopyStagingSwapRecoveryResult {
            peer_index: request.peer.peer_index,
            directory_relative_path: request.directory_relative_path,
            status: CopyStagingSwapRecoveryStatus::Recovered,
            output_lines: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn displace_to_bak(
        &self,
        request: CopyStagingDisplacementRequest,
    ) -> CopyStagingDisplacementResult {
        let mut output_lines = Vec::new();
        if request.options.mode == CopyStagingRunMode::DryRun {
            push_delete_progress(&mut output_lines, request.options.verbosity, &request.relative_path);
            return CopyStagingDisplacementResult {
                peer_index: request.peer.peer_index,
                peer_url: request.peer.peer_url,
                relative_path: request.relative_path,
                status: CopyStagingDisplacementStatus::PlannedDryRun,
                output_lines,
                diagnostics: Vec::new(),
            };
        }

        let (parent, basename) = split_parent_basename(&request.relative_path);
        let result = (|| {
            let timestamp = self.formatrules.current_timestamp();
            let bak_dir = self
                .formatrules
                .bak_directory_path(parent.as_deref(), &timestamp)
                .map_err(|_| PeerTransportError::IoError)?;
            self.peertransportsurface.create_dir(&request.peer.root, &bak_dir)?;
            self.peertransportsurface.rename(
                &request.peer.root,
                &request.relative_path,
                &format!("{bak_dir}{basename}"),
            )
        })();

        match result {
            Ok(()) => {
                push_delete_progress(&mut output_lines, request.options.verbosity, &request.relative_path);
                CopyStagingDisplacementResult {
                    peer_index: request.peer.peer_index,
                    peer_url: request.peer.peer_url,
                    relative_path: request.relative_path,
                    status: CopyStagingDisplacementStatus::Displaced,
                    output_lines,
                    diagnostics: Vec::new(),
                }
            }
            Err(error) => CopyStagingDisplacementResult {
                peer_index: request.peer.peer_index,
                peer_url: request.peer.peer_url.clone(),
                relative_path: request.relative_path.clone(),
                status: CopyStagingDisplacementStatus::Failed,
                output_lines,
                diagnostics: vec![CopyStagingDiagnostic {
                    level: CopyStagingDiagnosticLevel::Error,
                    peer_index: request.peer.peer_index,
                    peer_url: request.peer.peer_url,
                    relative_path: Some(request.relative_path),
                    kind: CopyStagingDiagnosticKind::DisplacementFailed {
                        transport_error: Some(error),
                    },
                }],
            },
        }
    }

    fn cleanup_metadata(
        &self,
        request: CopyStagingDirectoryRequest,
    ) -> CopyStagingCleanupResult {
        if request.options.mode == CopyStagingRunMode::DryRun {
            return CopyStagingCleanupResult {
                peer_index: request.peer.peer_index,
                directory_relative_path: request.directory_relative_path,
                status: CopyStagingCleanupStatus::SkippedDryRun,
                output_lines: Vec::new(),
                diagnostics: Vec::new(),
            };
        }

        let bak_root = metadata_child_path(request.directory_relative_path.as_deref(), "BAK");
        let tmp_root = metadata_child_path(request.directory_relative_path.as_deref(), "TMP");
        let now = SystemTime::now();
        let result = self
            .cleanup_timestamp_dir(&request.peer, &bak_root, request.options.keep_bak_days, now)
            .and_then(|_| {
                self.cleanup_timestamp_dir(&request.peer, &tmp_root, request.options.keep_tmp_days, now)
            });

        match result {
            Ok(()) => CopyStagingCleanupResult {
                peer_index: request.peer.peer_index,
                directory_relative_path: request.directory_relative_path,
                status: CopyStagingCleanupStatus::Completed,
                output_lines: Vec::new(),
                diagnostics: Vec::new(),
            },
            Err(error) => CopyStagingCleanupResult {
                peer_index: request.peer.peer_index,
                directory_relative_path: request.directory_relative_path.clone(),
                status: CopyStagingCleanupStatus::Failed,
                output_lines: Vec::new(),
                diagnostics: vec![CopyStagingDiagnostic {
                    level: CopyStagingDiagnosticLevel::Error,
                    peer_index: request.peer.peer_index,
                    peer_url: request.peer.peer_url,
                    relative_path: request.directory_relative_path,
                    kind: CopyStagingDiagnosticKind::CleanupFailed {
                        transport_error: Some(error),
                    },
                }],
            },
        }
    }
}

fn failed_copy(
    request: CopyStagingCopyRequest,
    attempts: u64,
    output_lines: Vec<String>,
    phase: CopyStagingFailurePhase,
    error: Option<PeerTransportError>,
    exhausted: bool,
) -> CopyStagingCopyResult {
    let mut diagnostics = vec![CopyStagingDiagnostic {
        level: CopyStagingDiagnosticLevel::Error,
        peer_index: request.destination_peer.peer_index,
        peer_url: request.destination_peer.peer_url.clone(),
        relative_path: Some(request.relative_path.clone()),
        kind: CopyStagingDiagnosticKind::TransferFailed {
            phase,
            transport_error: error,
        },
    }];
    if exhausted {
        diagnostics.push(CopyStagingDiagnostic {
            level: CopyStagingDiagnosticLevel::Error,
            peer_index: request.destination_peer.peer_index,
            peer_url: request.destination_peer.peer_url.clone(),
            relative_path: Some(request.relative_path.clone()),
            kind: CopyStagingDiagnosticKind::CopyTriesExhausted,
        });
    }

    CopyStagingCopyResult {
        destination_peer_index: request.destination_peer.peer_index,
        destination_peer_url: request.destination_peer.peer_url,
        relative_path: request.relative_path,
        status: CopyStagingCopyStatus::Failed,
        attempts,
        output_lines,
        diagnostics,
    }
}

fn failed_recovery(
    request: CopyStagingDirectoryRequest,
    error: Option<PeerTransportError>,
) -> CopyStagingSwapRecoveryResult {
    CopyStagingSwapRecoveryResult {
        peer_index: request.peer.peer_index,
        directory_relative_path: request.directory_relative_path.clone(),
        status: CopyStagingSwapRecoveryStatus::Failed,
        output_lines: Vec::new(),
        diagnostics: vec![CopyStagingDiagnostic {
            level: CopyStagingDiagnosticLevel::Error,
            peer_index: request.peer.peer_index,
            peer_url: request.peer.peer_url,
            relative_path: request.directory_relative_path,
            kind: CopyStagingDiagnosticKind::SwapRecoveryFailed {
                transport_error: error,
            },
        }],
    }
}

fn push_copy_progress(lines: &mut Vec<String>, verbosity: CopyStagingVerbosity, relative_path: &str) {
    if verbosity != CopyStagingVerbosity::Error {
        lines.push(format!("C {relative_path}"));
    }
}

fn push_delete_progress(lines: &mut Vec<String>, verbosity: CopyStagingVerbosity, relative_path: &str) {
    if verbosity != CopyStagingVerbosity::Error {
        lines.push(format!("X {relative_path}"));
    }
}

fn split_parent_basename(path: &str) -> (Option<String>, String) {
    match path.rsplit_once('/') {
        Some((parent, basename)) => (Some(parent.to_string()), basename.to_string()),
        None => (None, path.to_string()),
    }
}

fn join_parent_basename(parent: Option<&str>, basename: &str) -> String {
    match parent {
        Some(parent) => format!("{parent}/{basename}"),
        None => basename.to_string(),
    }
}

fn join_path(parent: &str, child: &str) -> String {
    if parent.is_empty() {
        child.to_string()
    } else {
        format!("{parent}/{child}")
    }
}

fn metadata_child_path(parent: Option<&str>, child: &str) -> String {
    match parent {
        Some(parent) => format!("{parent}/.kitchensync/{child}"),
        None => format!(".kitchensync/{child}"),
    }
}

fn percent_decode_segment(segment: &str) -> Option<String> {
    let bytes = segment.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return None;
            }
            let high = hex_value(bytes[index + 1])?;
            let low = hex_value(bytes[index + 2])?;
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

pub fn new(
    formatrules: Arc<dyn formatrules::FormatRules>,
    peertransportsurface: Arc<dyn peertransportsurface::PeerTransportSurface>,
) -> Arc<dyn CopyStaging> {
    Arc::new(CopyStagingImpl {
        formatrules,
        peertransportsurface,
        copy_slots: Arc::new(CopySlotState {
            active: Mutex::new(0),
            changed: Condvar::new(),
        }),
    })
}
