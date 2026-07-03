use std::sync::Arc;

use crate::api::*;

struct DryRunModeImpl;

impl DryRunMode for DryRunModeImpl {
    fn dry_run_output_line(&self) -> String {
        "dry run".to_string()
    }

    fn startup_root_decision(
        &self,
        _scheme: DryRunModePeerScheme,
        root_state: DryRunModeRootState,
    ) -> DryRunModeStartupRootDecision {
        match root_state {
            DryRunModeRootState::Exists => DryRunModeStartupRootDecision::UseExistingRoot,
            DryRunModeRootState::Missing => {
                DryRunModeStartupRootDecision::FailCandidateWithoutCreatingRoot
            }
        }
    }

    fn snapshot_startup_decision(
        &self,
        outcome: DryRunModeSnapshotDownloadOutcome,
    ) -> DryRunModeSnapshotStartupDecision {
        match outcome {
            DryRunModeSnapshotDownloadOutcome::Found => {
                DryRunModeSnapshotStartupDecision::UseLiveSnapshotAsLocalTemporary
            }
            DryRunModeSnapshotDownloadOutcome::NotFound => {
                DryRunModeSnapshotStartupDecision::CreateEmptyLocalTemporarySnapshot
            }
            DryRunModeSnapshotDownloadOutcome::Failed => {
                DryRunModeSnapshotStartupDecision::ExcludePeerWithErrorDiagnostic
            }
        }
    }

    fn classify_work(&self, work: DryRunModeWorkKind) -> DryRunModeWorkDecision {
        match work {
            DryRunModeWorkKind::ConnectToExistingRoot
            | DryRunModeWorkKind::ListDirectory
            | DryRunModeWorkKind::StatPath
            | DryRunModeWorkKind::DownloadSnapshot
            | DryRunModeWorkKind::ReadSourceFile => DryRunModeWorkDecision::AllowPeerRead,
            DryRunModeWorkKind::CreateOrUpdateLocalTemporarySnapshot => {
                DryRunModeWorkDecision::AllowLocalWorkingWrite
            }
            DryRunModeWorkKind::CreatePeerDirectory
            | DryRunModeWorkKind::CreatePeerMetadataDirectory
            | DryRunModeWorkKind::WritePeerFile
            | DryRunModeWorkKind::RenamePeerEntry
            | DryRunModeWorkKind::DeletePeerEntry
            | DryRunModeWorkKind::DisplacePeerEntryToBak
            | DryRunModeWorkKind::SetPeerModificationTime
            | DryRunModeWorkKind::RecoverPeerSnapshotSwap
            | DryRunModeWorkKind::RecoverPeerUserFileSwap
            | DryRunModeWorkKind::CleanPeerBakTmp
            | DryRunModeWorkKind::UploadPeerSnapshot => {
                DryRunModeWorkDecision::SuppressPeerWritePlannedSuccess
            }
        }
    }

    fn copy_work_policy(&self) -> DryRunModeCopyWorkPolicy {
        DryRunModeCopyWorkPolicy {
            acquire_copy_slots: true,
            read_sources: true,
            apply_normal_retry_limit: true,
            emit_copy_progress: true,
            emit_delete_progress: true,
        }
    }
}

pub fn new() -> std::sync::Arc<dyn DryRunMode> {
    Arc::new(DryRunModeImpl)
}
