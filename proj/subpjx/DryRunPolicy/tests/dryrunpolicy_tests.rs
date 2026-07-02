use dryrunpolicy::{
    new, DryRunLocalSnapshotCompletionDecision, DryRunMissingPeerRootDecision,
    DryRunMissingPeerSnapshotDecision, DryRunPeerMutation, DryRunPeerMutationDecision,
};

#[test]
fn dry_run_policy_preserves_reads_and_skips_peer_mutations() {
    let subject = new();

    assert!(subject.should_connect_to_peer_urls());
    assert!(!subject.may_create_missing_peer_root());
    assert!(!subject.may_create_missing_peer_root_parent());
    assert_eq!(
        DryRunMissingPeerRootDecision::UrlUnreachable,
        subject.decide_missing_peer_root()
    );

    assert!(!subject.may_run_peer_snapshot_swap_recovery());
    assert!(subject.should_download_existing_peer_snapshot());
    assert_eq!(
        DryRunMissingPeerSnapshotDecision::CreateEmptyLocalTemporarySnapshot,
        subject.decide_missing_peer_snapshot()
    );

    assert!(subject.should_list_peer_directories());
    assert!(!subject.may_run_peer_user_data_swap_recovery());
    assert!(subject.may_update_local_temporary_snapshot_databases());

    assert!(subject.should_exercise_copy_queue());
    assert!(subject.should_acquire_active_copy_slot());
    assert!(subject.should_read_copy_source_file());
    assert!(subject.should_apply_copy_retry_limit());
    assert!(subject.should_emit_copy_progress_events());
    assert!(subject.should_emit_failed_copy_progress_events());
    assert!(subject.should_emit_planned_removal_or_displacement_events());

    for mutation in [
        DryRunPeerMutation::CreateDirectory,
        DryRunPeerMutation::CreateFile,
        DryRunPeerMutation::WriteFileContent,
        DryRunPeerMutation::RenameEntry,
        DryRunPeerMutation::DeleteDestinationFile,
        DryRunPeerMutation::DisplaceDestinationToBak,
        DryRunPeerMutation::SetModificationTime,
        DryRunPeerMutation::UploadSnapshot,
        DryRunPeerMutation::CleanBakStorage,
        DryRunPeerMutation::CleanTmpStorage,
    ] {
        assert_eq!(
            DryRunPeerMutationDecision::SkipPlannedAction,
            subject.decide_peer_mutation(mutation)
        );
    }

    assert_eq!(
        DryRunLocalSnapshotCompletionDecision::KeepLocalTemporaryOnly,
        subject.decide_local_snapshot_completion()
    );
    assert!(
        subject.output_marker().text.contains("dry run"),
        "dry-run output marker must contain the required phrase"
    );
}
