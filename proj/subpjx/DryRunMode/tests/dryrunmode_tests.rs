use dryrunmode::{
    new, DryRunMode, DryRunModeCopyWorkPolicy,
    DryRunModePeerScheme::{File, Sftp},
    DryRunModeRootState::{Exists, Missing},
    DryRunModeSnapshotDownloadOutcome::{Failed, Found, NotFound},
    DryRunModeSnapshotStartupDecision::{
        CreateEmptyLocalTemporarySnapshot, ExcludePeerWithErrorDiagnostic,
        UseLiveSnapshotAsLocalTemporary,
    },
    DryRunModeStartupRootDecision::{FailCandidateWithoutCreatingRoot, UseExistingRoot},
    DryRunModeWorkDecision::{
        AllowLocalWorkingWrite, AllowPeerRead, SuppressPeerWritePlannedSuccess,
    },
    DryRunModeWorkKind::{
        CleanPeerBakTmp, ConnectToExistingRoot, CreateOrUpdateLocalTemporarySnapshot,
        CreatePeerDirectory, CreatePeerMetadataDirectory, DeletePeerEntry, DisplacePeerEntryToBak,
        DownloadSnapshot, ListDirectory, ReadSourceFile, RecoverPeerSnapshotSwap,
        RecoverPeerUserFileSwap, RenamePeerEntry, SetPeerModificationTime, StatPath,
        UploadPeerSnapshot, WritePeerFile,
    },
};

#[test]
fn output_line_is_exactly_dry_run() {
    let subject = new();

    assert_eq!(subject.dry_run_output_line(), "dry run");
}

#[test]
fn output_line_is_idempotent() {
    let subject = new();

    assert_eq!(subject.dry_run_output_line(), "dry run");
    assert_eq!(subject.dry_run_output_line(), "dry run");
}

#[test]
fn existing_roots_remain_eligible_for_startup() {
    let subject = new();

    assert_eq!(subject.startup_root_decision(File, Exists), UseExistingRoot);
    assert_eq!(subject.startup_root_decision(Sftp, Exists), UseExistingRoot);
}

#[test]
fn missing_roots_fail_without_creation() {
    let subject = new();

    assert_eq!(
        subject.startup_root_decision(File, Missing),
        FailCandidateWithoutCreatingRoot
    );
    assert_eq!(
        subject.startup_root_decision(Sftp, Missing),
        FailCandidateWithoutCreatingRoot
    );
}

#[test]
fn found_snapshot_is_used_as_local_temporary_snapshot() {
    let subject = new();

    assert_eq!(
        subject.snapshot_startup_decision(Found),
        UseLiveSnapshotAsLocalTemporary
    );
}

#[test]
fn missing_snapshot_creates_empty_local_temporary_snapshot() {
    let subject = new();

    assert_eq!(
        subject.snapshot_startup_decision(NotFound),
        CreateEmptyLocalTemporarySnapshot
    );
}

#[test]
fn failed_snapshot_download_excludes_peer_with_error_diagnostic() {
    let subject = new();

    assert_eq!(
        subject.snapshot_startup_decision(Failed),
        ExcludePeerWithErrorDiagnostic
    );
}

#[test]
fn peer_read_work_is_allowed() {
    let subject = new();

    for work in [
        ConnectToExistingRoot,
        ListDirectory,
        StatPath,
        DownloadSnapshot,
        ReadSourceFile,
    ] {
        assert_eq!(subject.classify_work(work), AllowPeerRead);
    }
}

#[test]
fn local_temporary_snapshot_work_is_allowed() {
    let subject = new();

    assert_eq!(
        subject.classify_work(CreateOrUpdateLocalTemporarySnapshot),
        AllowLocalWorkingWrite
    );
}

#[test]
fn peer_write_work_is_suppressed_as_planned_success() {
    let subject = new();

    for work in [
        CreatePeerDirectory,
        CreatePeerMetadataDirectory,
        WritePeerFile,
        RenamePeerEntry,
        DeletePeerEntry,
        DisplacePeerEntryToBak,
        SetPeerModificationTime,
        RecoverPeerSnapshotSwap,
        RecoverPeerUserFileSwap,
        CleanPeerBakTmp,
        UploadPeerSnapshot,
    ] {
        assert_eq!(subject.classify_work(work), SuppressPeerWritePlannedSuccess);
    }
}

#[test]
fn work_classification_is_idempotent() {
    let subject = new();

    assert_eq!(subject.classify_work(ListDirectory), AllowPeerRead);
    assert_eq!(subject.classify_work(ListDirectory), AllowPeerRead);
    assert_eq!(
        subject.classify_work(CreateOrUpdateLocalTemporarySnapshot),
        AllowLocalWorkingWrite
    );
    assert_eq!(
        subject.classify_work(CreateOrUpdateLocalTemporarySnapshot),
        AllowLocalWorkingWrite
    );
    assert_eq!(
        subject.classify_work(WritePeerFile),
        SuppressPeerWritePlannedSuccess
    );
    assert_eq!(
        subject.classify_work(WritePeerFile),
        SuppressPeerWritePlannedSuccess
    );
}

#[test]
fn copy_work_policy_preserves_real_copy_flow_and_progress() {
    let subject = new();

    assert_eq!(
        subject.copy_work_policy(),
        DryRunModeCopyWorkPolicy {
            acquire_copy_slots: true,
            read_sources: true,
            apply_normal_retry_limit: true,
            emit_copy_progress: true,
            emit_delete_progress: true,
        }
    );
}
