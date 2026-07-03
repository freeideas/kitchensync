use std::path::PathBuf;
use std::time::{Duration, UNIX_EPOCH};

use formatrules::{
    new, FormatRules, FormatRulesDeletionEstimateUpdate, FormatRulesPeerIdentityRequest,
    FormatRulesValidationErrorKind,
};

fn request(peer_url: &str, os_username: Option<&str>) -> FormatRulesPeerIdentityRequest {
    FormatRulesPeerIdentityRequest {
        peer_url: peer_url.to_string(),
        current_working_directory: PathBuf::from("/tmp/kitchensync/root"),
        os_username: os_username.map(str::to_string),
    }
}

#[test]
fn normalizes_file_peer_identity_from_scheme_less_argument() {
    let subject = new();

    let identity = subject
        .normalize_peer_identity(request("peers/local", Some("ace")))
        .unwrap();

    assert_eq!(identity, "file:///tmp/kitchensync/root/peers/local");
}

#[test]
fn normalizes_sftp_peer_identity_components() {
    let subject = new();

    let identity = subject
        .normalize_peer_identity(request(
            "SFTP://Example.COM:22//repo//Team/%7Ealice/?ignored=true",
            Some("ace"),
        ))
        .unwrap();

    assert_eq!(identity, "sftp://ace@example.com/repo/Team/~alice");
}

#[test]
fn preserves_reserved_percent_escapes_in_peer_identity() {
    let subject = new();

    let identity = subject
        .normalize_peer_identity(request("sftp://bob@example.com/a%2Fb", Some("ace")))
        .unwrap();

    assert_eq!(identity, "sftp://bob@example.com/a%2Fb");
}

#[test]
fn rejects_sftp_peer_identity_without_required_os_username() {
    let subject = new();

    let error = subject
        .normalize_peer_identity(request("sftp://example.com/repo", None))
        .unwrap_err();

    assert_eq!(error.kind, FormatRulesValidationErrorKind::MissingOsUsername);
}

#[test]
fn rejects_invalid_peer_url_text() {
    let subject = new();

    let error = subject
        .normalize_peer_identity(request("sftp://[::1/repo", Some("ace")))
        .unwrap_err();

    assert_eq!(error.kind, FormatRulesValidationErrorKind::InvalidPeerUrl);
}

#[test]
fn accepts_relative_path_text_unchanged() {
    let subject = new();

    let path = subject.validate_relative_path("docs/readme.txt").unwrap();

    assert_eq!(path, "docs/readme.txt");
}

#[test]
fn rejects_malformed_relative_paths() {
    let subject = new();

    for path in [
        "",
        "/docs",
        "docs/",
        "docs//readme.txt",
        "docs\\readme.txt",
        "docs/./readme.txt",
        "docs/../readme.txt",
        "docs/readme.txt\0",
    ] {
        let error = subject.validate_relative_path(path).unwrap_err();
        assert_eq!(
            error.kind,
            FormatRulesValidationErrorKind::InvalidRelativePath,
            "{path:?}"
        );
    }
}

#[test]
fn returns_snapshot_ids_for_nested_and_root_entries() {
    let subject = new();

    let nested = subject.snapshot_path_ids("docs/readme.txt").unwrap();
    assert_eq!(nested.id, "K5EzsWuLZ04");
    assert_eq!(nested.parent_id, "H41WPg3SlMv");
    assert_snapshot_id_shape(&nested.id);
    assert_snapshot_id_shape(&nested.parent_id);

    let root_entry = subject.snapshot_path_ids("docs").unwrap();
    assert_eq!(root_entry.id, "H41WPg3SlMv");
    assert_eq!(root_entry.parent_id, "JyBskcNRrBK");
    assert_snapshot_id_shape(&root_entry.id);
    assert_snapshot_id_shape(&root_entry.parent_id);
}

#[test]
fn rejects_snapshot_id_for_sync_root() {
    let subject = new();

    let error = subject.snapshot_path_ids("").unwrap_err();

    assert_eq!(error.kind, FormatRulesValidationErrorKind::RootSnapshotPath);
}

#[test]
fn rejects_malformed_snapshot_paths() {
    let subject = new();

    let error = subject.snapshot_path_ids("docs//readme.txt").unwrap_err();

    assert_eq!(error.kind, FormatRulesValidationErrorKind::InvalidRelativePath);
}

fn assert_snapshot_id_shape(id: &str) {
    assert_eq!(id.len(), 11);
    assert!(id.bytes().all(|byte| byte.is_ascii_alphanumeric()));
}

#[test]
fn parses_and_formats_only_the_specified_timestamp_shape() {
    let subject = new();

    let parsed = subject
        .parse_timestamp("2024-01-01_12-00-00_123456Z")
        .unwrap();
    assert_eq!(
        subject.timestamp_text(&parsed),
        "2024-01-01_12-00-00_123456Z"
    );

    let formatted =
        subject.format_timestamp(UNIX_EPOCH + Duration::new(1_704_110_400, 123_456_000));
    assert_eq!(
        subject.timestamp_text(&formatted),
        "2024-01-01_12-00-00_123456Z"
    );
    assert_eq!(
        subject.timestamp_system_time(&parsed),
        UNIX_EPOCH + Duration::new(1_704_110_400, 123_456_000)
    );
}

#[test]
fn rejects_invalid_timestamp_text() {
    let subject = new();

    for timestamp in [
        "2024-01-01T12:00:00Z",
        "2024-01-01_12-00-00_12345Z",
        "2024-01-01_12-00-00_1234567Z",
        "2024-01-01_12-00-00_123456+00:00",
    ] {
        let error = subject.parse_timestamp(timestamp).unwrap_err();
        assert_eq!(
            error.kind,
            FormatRulesValidationErrorKind::InvalidTimestamp,
            "{timestamp:?}"
        );
    }
}

#[test]
fn generates_current_timestamps_in_strictly_increasing_order() {
    let subject = new();

    let first = subject.current_timestamp();
    let second = subject.current_timestamp();

    assert!(subject.timestamp_system_time(&second) > subject.timestamp_system_time(&first));
}

#[test]
fn deletion_estimate_helpers_copy_existing_timestamps() {
    let subject = new();
    let last_seen = subject
        .parse_timestamp("2024-01-01_12-00-00_000001Z")
        .unwrap();
    let existing_deleted_time = subject
        .parse_timestamp("2024-01-01_12-00-01_000001Z")
        .unwrap();

    let update = subject.confirmed_absence_deleted_time(&last_seen, None);
    assert_eq!(
        update,
        FormatRulesDeletionEstimateUpdate::Write(last_seen.clone())
    );

    let update =
        subject.confirmed_absence_deleted_time(&last_seen, Some(&existing_deleted_time));
    assert_eq!(update, FormatRulesDeletionEstimateUpdate::NoWrite);

    let displaced = subject.displacement_deleted_time(&last_seen);
    assert_eq!(
        subject.timestamp_text(&displaced),
        subject.timestamp_text(&last_seen)
    );

    let cascaded = subject.displacement_cascade_deleted_time(&displaced);
    assert_eq!(
        subject.timestamp_text(&cascaded),
        subject.timestamp_text(&last_seen)
    );
}

#[test]
fn formats_metadata_paths() {
    let subject = new();
    let timestamp = subject
        .parse_timestamp("2024-01-01_12-00-00_123456Z")
        .unwrap();

    let root_bak = subject.bak_directory_path(None, &timestamp).unwrap();
    assert_eq!(
        root_bak,
        ".kitchensync/BAK/2024-01-01_12-00-00_123456Z/"
    );

    let nested_bak = subject
        .bak_directory_path(Some("docs"), &timestamp)
        .unwrap();
    assert_eq!(
        nested_bak,
        "docs/.kitchensync/BAK/2024-01-01_12-00-00_123456Z/"
    );

    assert_eq!(
        subject.tmp_directory_path(&timestamp),
        ".kitchensync/TMP/2024-01-01_12-00-00_123456Z/"
    );

    let user_swap = subject.user_swap_paths(Some("docs"), "report 1.txt").unwrap();
    assert_eq!(
        user_swap.directory_path,
        "docs/.kitchensync/SWAP/report%201.txt"
    );
    assert_eq!(
        user_swap.new_path,
        "docs/.kitchensync/SWAP/report%201.txt/new"
    );
    assert_eq!(
        user_swap.old_path,
        "docs/.kitchensync/SWAP/report%201.txt/old"
    );

    let snapshot_swap = subject.snapshot_swap_paths();
    assert_eq!(snapshot_swap.new_path, ".kitchensync/SWAP/snapshot.db/new");
    assert_eq!(snapshot_swap.old_path, ".kitchensync/SWAP/snapshot.db/old");
}

#[test]
fn rejects_invalid_metadata_path_inputs() {
    let subject = new();
    let timestamp = subject
        .parse_timestamp("2024-01-01_12-00-00_123456Z")
        .unwrap();

    let error = subject
        .bak_directory_path(Some("docs//bad"), &timestamp)
        .unwrap_err();
    assert_eq!(error.kind, FormatRulesValidationErrorKind::InvalidRelativePath);

    for basename in ["dir/file.txt", "dir\\file.txt", "bad\0name"] {
        let error = subject.user_swap_paths(None, basename).unwrap_err();
        assert_eq!(
            error.kind,
            FormatRulesValidationErrorKind::InvalidSwapBasename,
            "{basename:?}"
        );
    }
}

#[test]
fn applies_five_second_tolerance_to_file_and_peer_mod_times() {
    let subject = new();
    let max = subject
        .parse_timestamp("2024-01-01_12-00-10_000000Z")
        .unwrap();
    let exactly_five_seconds_behind = subject
        .parse_timestamp("2024-01-01_12-00-05_000000Z")
        .unwrap();
    let more_than_five_seconds_behind = subject
        .parse_timestamp("2024-01-01_12-00-04_999999Z")
        .unwrap();
    let more_than_five_seconds_ahead = subject
        .parse_timestamp("2024-01-01_12-00-15_000001Z")
        .unwrap();

    assert!(subject.file_mod_times_same(&max, &exactly_five_seconds_behind));
    assert!(!subject.file_mod_times_same(&max, &more_than_five_seconds_behind));
    assert!(!subject.file_mod_times_same(&max, &more_than_five_seconds_ahead));

    assert!(subject.peer_mod_time_tied_with_max(
        &exactly_five_seconds_behind,
        &max
    ));
    assert!(!subject.peer_mod_time_tied_with_max(
        &more_than_five_seconds_behind,
        &max
    ));
    assert!(!subject.peer_mod_time_older_than_max(
        &exactly_five_seconds_behind,
        &max
    ));
    assert!(subject.peer_mod_time_older_than_max(
        &more_than_five_seconds_behind,
        &max
    ));
}

#[test]
fn applies_five_second_tolerance_to_deletion_estimates() {
    let subject = new();
    let file_time = subject
        .parse_timestamp("2024-01-01_12-00-10_000000Z")
        .unwrap();
    let exactly_five_seconds_newer = subject
        .parse_timestamp("2024-01-01_12-00-15_000000Z")
        .unwrap();
    let more_than_five_seconds_newer = subject
        .parse_timestamp("2024-01-01_12-00-15_000001Z")
        .unwrap();

    assert!(!subject.deletion_estimate_wins_over_file_mod_time(
        &exactly_five_seconds_newer,
        &file_time
    ));
    assert!(subject.deletion_estimate_wins_over_file_mod_time(
        &more_than_five_seconds_newer,
        &file_time
    ));
    assert!(!subject.absent_unconfirmed_file_counts_as_deletion(
        &exactly_five_seconds_newer,
        &file_time
    ));
    assert!(subject.absent_unconfirmed_file_counts_as_deletion(
        &more_than_five_seconds_newer,
        &file_time
    ));
    assert!(!subject.directory_deletion_estimate_newer_than_live_file_evidence(
        &exactly_five_seconds_newer,
        &file_time
    ));
    assert!(subject.directory_deletion_estimate_newer_than_live_file_evidence(
        &more_than_five_seconds_newer,
        &file_time
    ));
}

#[test]
fn reports_live_file_timestamp_evidence_for_directories() {
    let subject = new();
    let older_file = subject
        .parse_timestamp("2024-01-01_12-00-10_000000Z")
        .unwrap();
    let newer_file = subject
        .parse_timestamp("2024-01-01_12-00-11_000000Z")
        .unwrap();

    assert_eq!(subject.directory_live_file_timestamp_evidence(&[]), None);

    let evidence =
        subject.directory_live_file_timestamp_evidence(&[older_file, newer_file.clone()]);
    assert_eq!(evidence, Some(newer_file));
}
