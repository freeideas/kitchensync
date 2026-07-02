use std::sync::Arc;
use crate::api::*;

struct StdoutReporterImpl;

fn info_visible(verbosity: StdoutVerbosity) -> bool {
    matches!(
        verbosity,
        StdoutVerbosity::Info | StdoutVerbosity::Debug | StdoutVerbosity::Trace
    )
}

fn error_kind_label(kind: StdoutErrorKind) -> &'static str {
    match kind {
        StdoutErrorKind::ArgumentError => "argument_error",
        StdoutErrorKind::NoSnapshotsAndNoCanon => "no_snapshots_and_no_canon",
        StdoutErrorKind::UnreachablePeer => "unreachable_peer",
        StdoutErrorKind::DirectoryListingFailure => "directory_listing_failure",
        StdoutErrorKind::CanonPeerUnreachable => "canon_peer_unreachable",
        StdoutErrorKind::FewerThanTwoReachablePeers => "fewer_than_two_reachable_peers",
        StdoutErrorKind::NoContributingPeerReachable => "no_contributing_peer_reachable",
        StdoutErrorKind::TransferFailureBeforeSwapOld => "transfer_failure_before_swap_old",
        StdoutErrorKind::TransferFailureAfterSwapOld => "transfer_failure_after_swap_old",
        StdoutErrorKind::ArchiveOldFailure => "archive_old_failure",
        StdoutErrorKind::DisplacementFailure => "displacement_failure",
        StdoutErrorKind::TmpOrSwapStagingFailure => "tmp_or_swap_staging_failure",
        StdoutErrorKind::SetModTimeFailure => "set_mod_time_failure",
        StdoutErrorKind::SnapshotUploadFailureBeforeSwapOld => {
            "snapshot_upload_failure_before_swap_old"
        }
        StdoutErrorKind::SnapshotUploadFailureAfterSwapOld => {
            "snapshot_upload_failure_after_swap_old"
        }
    }
}

fn transfer_phase_label(phase: StdoutFileTransferPhase) -> &'static str {
    match phase {
        StdoutFileTransferPhase::ReadSource => "read_source",
        StdoutFileTransferPhase::WriteSwapNew => "write_swap_new",
        StdoutFileTransferPhase::MoveExistingToSwapOld => "move_existing_to_swap_old",
        StdoutFileTransferPhase::RenameFinal => "rename_final",
        StdoutFileTransferPhase::SetModTime => "set_mod_time",
        StdoutFileTransferPhase::ArchiveOld => "archive_old",
        StdoutFileTransferPhase::Cleanup => "cleanup",
    }
}

impl StdoutReporter for StdoutReporterImpl {
    fn report_argument_validation_failure(
        &self,
        _verbosity: StdoutVerbosity,
        error_message: String,
        help_text: String,
    ) {
        println!("{}", error_message);
        print!("{}", help_text);
    }

    fn report_first_sync_requires_authoritative_peer(&self, _verbosity: StdoutVerbosity) {
        println!("First sync? Mark the authoritative peer with a leading +");
    }

    fn report_no_contributing_peer_reachable(&self, _verbosity: StdoutVerbosity) {
        println!("No contributing peer reachable - cannot make sync decisions");
    }

    fn report_error_diagnostic(
        &self,
        _verbosity: StdoutVerbosity,
        diagnostic: StdoutErrorDiagnostic,
    ) {
        println!(
            "{}: {}",
            error_kind_label(diagnostic.kind),
            diagnostic.details
        );
    }

    fn report_failed_file_transfer(
        &self,
        _verbosity: StdoutVerbosity,
        diagnostic: StdoutFailedFileTransferDiagnostic,
    ) {
        match diagnostic.transport_error_category {
            Some(category) => println!(
                "failed file transfer: relpath={} destination={} phase={} category={}",
                diagnostic.relpath,
                diagnostic.destination_peer_url,
                transfer_phase_label(diagnostic.phase),
                category
            ),
            None => println!(
                "failed file transfer: relpath={} destination={} phase={}",
                diagnostic.relpath,
                diagnostic.destination_peer_url,
                transfer_phase_label(diagnostic.phase)
            ),
        }
    }

    fn report_copy_progress(&self, verbosity: StdoutVerbosity, relpath: String) {
        if info_visible(verbosity) {
            println!("C {}", relpath);
        }
    }

    fn report_displacement_progress(&self, verbosity: StdoutVerbosity, relpath: String) {
        if info_visible(verbosity) {
            println!("X {}", relpath);
        }
    }

    fn report_copy_slots(&self, verbosity: StdoutVerbosity, active: u32, max: u32) {
        if verbosity == StdoutVerbosity::Trace {
            println!("copy-slots active={}/{}", active, max);
        }
    }

    fn report_completion(&self, _verbosity: StdoutVerbosity, message: String) {
        println!("{}", message);
    }
}

pub fn new() -> std::sync::Arc<dyn StdoutReporter> {
    Arc::new(StdoutReporterImpl)
}
