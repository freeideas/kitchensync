use std::time::{Duration, UNIX_EPOCH};

use snapshotstore_snapshotidentity::{
    new, SnapshotIdentityErrorKind, SNAPSHOT_ROOT_PARENT_ID,
};

fn subject() -> std::sync::Arc<dyn snapshotstore_snapshotidentity::SnapshotIdentity> {
    new()
}

#[test]
fn path_id_returns_documented_xxhash64_base62_ids_for_relative_paths() {
    let identity = subject();

    let cases = [
        ("docs", "H41WPg3SlMv"),
        ("docs/readme.txt", "K5EzsWuLZ04"),
        ("docs/notes", "1pP6ATZM5gH"),
    ];

    for (relative_path, expected_id) in cases {
        let id = identity.path_id(relative_path).unwrap();

        assert_eq!(expected_id, id);
        assert_eq!(11, id.len());
        assert!(id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric()));
    }
}

#[test]
fn parent_path_id_uses_root_parent_for_direct_children_and_parent_directory_for_deeper_entries() {
    let identity = subject();

    assert_eq!("JyBskcNRrBK", SNAPSHOT_ROOT_PARENT_ID);
    assert_eq!(
        SNAPSHOT_ROOT_PARENT_ID,
        identity.parent_path_id("docs").unwrap()
    );
    assert_eq!(
        identity.path_id("docs").unwrap(),
        identity.parent_path_id("docs/readme.txt").unwrap()
    );
}

#[test]
fn path_operations_reject_inputs_that_do_not_name_entries_below_the_sync_root() {
    let identity = subject();
    let invalid_paths = ["", "/docs", "docs/", "docs//readme.txt", ".", "..", "docs/."];

    for relative_path in invalid_paths {
        assert_eq!(
            SnapshotIdentityErrorKind::InvalidRelativePath,
            identity.path_id(relative_path).unwrap_err().kind
        );
        assert_eq!(
            SnapshotIdentityErrorKind::InvalidRelativePath,
            identity.parent_path_id(relative_path).unwrap_err().kind
        );
    }
}

#[test]
fn supplied_utc_times_format_with_microsecond_precision_and_lexicographic_order() {
    let identity = subject();

    let early = UNIX_EPOCH + Duration::new(1_704_067_200, 123_456_789);
    let later = UNIX_EPOCH + Duration::new(1_704_067_200, 123_457_000);
    let next_second = UNIX_EPOCH + Duration::new(1_704_067_201, 0);

    let early_text = identity.format_utc_timestamp(early).unwrap();
    let later_text = identity.format_utc_timestamp(later).unwrap();
    let next_second_text = identity.format_utc_timestamp(next_second).unwrap();

    assert_eq!("2024-01-01_00-00-00_123456Z", early_text);
    assert_eq!("2024-01-01_00-00-00_123457Z", later_text);
    assert_eq!("2024-01-01_00-00-01_000000Z", next_second_text);
    assert!(early_text < later_text);
    assert!(later_text < next_second_text);
}

#[test]
fn generated_timestamps_are_unique_strictly_increasing_and_use_the_timestamp_format() {
    let identity = subject();

    let first = identity.generate_timestamp().unwrap();
    let second = identity.generate_timestamp().unwrap();
    let third = identity.generate_timestamp().unwrap();

    assert!(first < second);
    assert!(second < third);
    assert_timestamp_shape(&first);
    assert_timestamp_shape(&second);
    assert_timestamp_shape(&third);
}

fn assert_timestamp_shape(value: &str) {
    let bytes = value.as_bytes();

    assert_eq!(27, bytes.len());
    assert_eq!(b'-', bytes[4]);
    assert_eq!(b'-', bytes[7]);
    assert_eq!(b'_', bytes[10]);
    assert_eq!(b'-', bytes[13]);
    assert_eq!(b'-', bytes[16]);
    assert_eq!(b'_', bytes[19]);
    assert_eq!(b'Z', bytes[26]);

    for index in [
        0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18, 20, 21, 22, 23, 24, 25,
    ] {
        assert!(bytes[index].is_ascii_digit());
    }
}
