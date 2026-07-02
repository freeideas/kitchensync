use std::sync::Arc;
use crate::api::*;

struct DryRunPolicyImpl;

impl DryRunPolicy for DryRunPolicyImpl {
    fn should_connect_to_peer_urls(&self) -> bool {
        true
    }

    fn may_create_missing_peer_root(&self) -> bool {
        false
    }

    fn may_create_missing_peer_root_parent(&self) -> bool {
        false
    }

    fn decide_missing_peer_root(&self) -> DryRunMissingPeerRootDecision {
        DryRunMissingPeerRootDecision::UrlUnreachable
    }

    fn may_run_peer_snapshot_swap_recovery(&self) -> bool {
        false
    }

    fn should_download_existing_peer_snapshot(&self) -> bool {
        true
    }

    fn decide_missing_peer_snapshot(&self) -> DryRunMissingPeerSnapshotDecision {
        DryRunMissingPeerSnapshotDecision::CreateEmptyLocalTemporarySnapshot
    }

    fn should_list_peer_directories(&self) -> bool {
        true
    }

    fn may_run_peer_user_data_swap_recovery(&self) -> bool {
        false
    }

    fn may_update_local_temporary_snapshot_databases(&self) -> bool {
        true
    }

    fn should_exercise_copy_queue(&self) -> bool {
        true
    }

    fn should_acquire_active_copy_slot(&self) -> bool {
        true
    }

    fn should_read_copy_source_file(&self) -> bool {
        true
    }

    fn should_apply_copy_retry_limit(&self) -> bool {
        true
    }

    fn should_emit_copy_progress_events(&self) -> bool {
        true
    }

    fn should_emit_failed_copy_progress_events(&self) -> bool {
        true
    }

    fn should_emit_planned_removal_or_displacement_events(&self) -> bool {
        true
    }

    fn decide_peer_mutation(&self, _mutation: DryRunPeerMutation) -> DryRunPeerMutationDecision {
        DryRunPeerMutationDecision::SkipPlannedAction
    }

    fn decide_local_snapshot_completion(&self) -> DryRunLocalSnapshotCompletionDecision {
        DryRunLocalSnapshotCompletionDecision::KeepLocalTemporaryOnly
    }

    fn output_marker(&self) -> DryRunOutputMarker {
        DryRunOutputMarker {
            text: String::from("dry run"),
        }
    }
}

pub fn new() -> std::sync::Arc<dyn DryRunPolicy> {
    Arc::new(DryRunPolicyImpl)
}
