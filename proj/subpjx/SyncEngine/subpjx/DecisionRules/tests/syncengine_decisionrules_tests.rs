use syncengine_decisionrules::{
    new, DecisionRules, Decision, DecidedType, Conform,
    PeerInput, PeerRole, LiveEntry, PeerRow, PeerOutcome,
};

// Timestamps in YYYY-MM-DD_HH-mm-ss_ffffffZ (microsecond resolution)
const T0: &str    = "2024-01-01_12-00-00_000000Z"; // baseline
const T_3S: &str  = "2024-01-01_12-00-03_000000Z"; // T0 + 3 s  (within 5 s)
const T_4S: &str  = "2024-01-01_12-00-04_000000Z"; // T0 + 4 s  (within 5 s)
const T_6S: &str  = "2024-01-01_12-00-06_000000Z"; // T0 + 6 s  (outside 5 s)
const T_10S: &str = "2024-01-01_12-00-10_000000Z"; // T0 + 10 s (outside 5 s)
const T_20S: &str = "2024-01-01_12-00-20_000000Z"; // T0 + 20 s

// ---- builder helpers ----

fn fe(byte_size: i64, mod_time: &str) -> LiveEntry {
    LiveEntry::File { byte_size, mod_time: mod_time.to_owned() }
}

fn sr(byte_size: i64, mod_time: &str) -> Option<PeerRow> {
    Some(PeerRow { byte_size, mod_time: mod_time.to_owned(), deleted_time: None, last_seen: None })
}

fn drow(byte_size: i64, mod_time: &str, deleted_time: &str) -> Option<PeerRow> {
    Some(PeerRow {
        byte_size,
        mod_time: mod_time.to_owned(),
        deleted_time: Some(deleted_time.to_owned()),
        last_seen: None,
    })
}

fn ur(byte_size: i64, mod_time: &str, last_seen: Option<&str>) -> Option<PeerRow> {
    Some(PeerRow {
        byte_size,
        mod_time: mod_time.to_owned(),
        deleted_time: None,
        last_seen: last_seen.map(|s| s.to_owned()),
    })
}

fn dir_row(mod_time: &str) -> Option<PeerRow> {
    Some(PeerRow { byte_size: -1, mod_time: mod_time.to_owned(), deleted_time: None, last_seen: None })
}

fn mk(peer: &str, role: PeerRole, live: LiveEntry, row: Option<PeerRow>) -> PeerInput {
    PeerInput { peer: peer.to_owned(), role, live, row }
}

fn ct(peer: &str, live: LiveEntry, row: Option<PeerRow>) -> PeerInput {
    mk(peer, PeerRole::Contributing, live, row)
}

fn ca(peer: &str, live: LiveEntry, row: Option<PeerRow>) -> PeerInput {
    mk(peer, PeerRole::Canon, live, row)
}

fn sb(peer: &str, live: LiveEntry, row: Option<PeerRow>) -> PeerInput {
    mk(peer, PeerRole::Subordinate, live, row)
}

fn act<'a>(d: &'a Decision, peer: &str) -> &'a PeerOutcome {
    d.actions.iter().find(|a| a.peer == peer).expect("peer missing from actions")
}

// ---- 007: roles and canon override ----

#[test]
fn req_007_1_canon_version_propagated_over_differing_peer() {
    // Canon A has a newer, larger file; contributing B has an older file.
    // A's version must win unconditionally.
    let dr = new();
    let peers = vec![
        ca("A", fe(200, T_10S), None),
        ct("B", fe(100, T0), sr(100, T0)),
    ];
    let d = dr.decide(&peers);
    assert!(matches!(d.agreed_type, DecidedType::File));
    assert_eq!(d.winner.as_deref(), Some("A"));
    assert!(matches!(act(&d, "B").conform, Conform::CopyWinner));
}

#[test]
fn req_007_2_subordinate_entries_do_not_affect_outcome() {
    // B is subordinate with a newer, larger file.
    // The outcome must be the same as if B were absent.
    let dr = new();
    let with_sub = vec![
        ct("A", fe(100, T0), sr(100, T0)),
        sb("B", fe(200, T_10S), None),
    ];
    let without_b = vec![
        ct("A", fe(100, T0), sr(100, T0)),
    ];
    let d1 = dr.decide(&with_sub);
    let d2 = dr.decide(&without_b);
    assert_eq!(d1.winner.as_deref(), Some("A"));
    assert_eq!(d2.winner.as_deref(), Some("A"));
    // B is conformed to A's version
    assert!(matches!(act(&d1, "B").conform, Conform::CopyWinner));
}

// ---- 010: per-peer classification ----

#[test]
fn req_010_1_unchanged_file_not_recopied_between_matching_peers() {
    // Both peers match their rows and each other (T_3S within 5 s of T0) -> no copy.
    let dr = new();
    let peers = vec![
        ct("A", fe(100, T0),  sr(100, T0)),
        ct("B", fe(100, T_3S), sr(100, T0)),
    ];
    let d = dr.decide(&peers);
    assert!(matches!(act(&d, "A").conform, Conform::Nothing));
    assert!(matches!(act(&d, "B").conform, Conform::Nothing));
}

#[test]
fn req_010_2_modified_byte_size_differs_version_is_propagated() {
    // A has byte_size 200 but its row recorded 100 -> treated as modified.
    // At the same mod_time, the larger live byte_size (200) governs selection.
    let dr = new();
    let peers = vec![
        ct("A", fe(200, T0), sr(100, T0)), // byte_size differs from row
        ct("B", fe(100, T0), sr(100, T0)),
    ];
    let d = dr.decide(&peers);
    assert_eq!(d.winner.as_deref(), Some("A")); // larger byte_size wins
    assert!(matches!(act(&d, "B").conform, Conform::CopyWinner));
}

#[test]
fn req_010_3_modified_mod_time_over_5s_version_is_propagated() {
    // A's live mod_time is 6 s after its row's mod_time -> treated as modified.
    // A's newer live mod_time wins.
    let dr = new();
    let peers = vec![
        ct("A", fe(100, T_6S), sr(100, T0)), // mod_time differs >5 s from row
        ct("B", fe(100, T0),  sr(100, T0)),
    ];
    let d = dr.decide(&peers);
    assert_eq!(d.winner.as_deref(), Some("A"));
    assert!(matches!(act(&d, "B").conform, Conform::CopyWinner));
}

#[test]
fn req_010_4_resurrection_file_propagated_tombstone_ignored() {
    // A has a live file but its row carries deleted_time (resurrection).
    // The tombstone must not trigger deletion; the live file must propagate.
    let dr = new();
    let peers = vec![
        ct("A", fe(100, T0), drow(100, T0, T_6S)), // live file with tombstoned row
        ct("B", LiveEntry::Absent, None),          // no opinion
    ];
    let d = dr.decide(&peers);
    assert!(matches!(d.agreed_type, DecidedType::File));
    assert_eq!(d.winner.as_deref(), Some("A"));
    assert!(matches!(act(&d, "B").conform, Conform::CopyWinner));
}

#[test]
fn req_010_5_new_file_no_row_propagated_to_peers_that_lack_it() {
    // A has a live file with no snapshot row -> classified new -> propagated.
    let dr = new();
    let peers = vec![
        ct("A", fe(100, T0), None), // new: live file, no row
        ct("B", LiveEntry::Absent, None),
    ];
    let d = dr.decide(&peers);
    assert!(matches!(d.agreed_type, DecidedType::File));
    assert_eq!(d.winner.as_deref(), Some("A"));
    assert!(matches!(act(&d, "B").conform, Conform::CopyWinner));
}

#[test]
fn req_010_6_deleted_row_deleted_time_is_the_deletion_estimate() {
    // A is absent with deleted_time T_10S -> classified deleted; T_10S is the estimate.
    // T_10S exceeds file mod_time T0 by 10 s > 5 s -> deletion wins.
    let dr = new();
    let peers = vec![
        ct("A", LiveEntry::Absent, drow(100, T0, T_10S)),
        ct("B", fe(100, T0), sr(100, T0)),
    ];
    let d = dr.decide(&peers);
    assert!(matches!(d.agreed_type, DecidedType::Absent));
    assert!(act(&d, "B").displace);
}

#[test]
fn req_010_7_absent_unconfirmed_null_deleted_time_not_treated_as_deletion() {
    // A is absent with a live row (no deleted_time) -> absent-unconfirmed, not a deletion.
    // File on B must survive.
    let dr = new();
    let peers = vec![
        ct("A", LiveEntry::Absent, ur(100, T0, None)),
        ct("B", fe(100, T0), sr(100, T0)),
    ];
    let d = dr.decide(&peers);
    assert!(matches!(d.agreed_type, DecidedType::File));
    assert_eq!(d.winner.as_deref(), Some("B"));
}

#[test]
fn req_010_8_no_row_absent_peer_does_not_remove_file() {
    // A is absent with no row at all -> no-opinion; must not cause B's file to be removed.
    let dr = new();
    let peers = vec![
        ct("A", LiveEntry::Absent, None), // no row
        ct("B", fe(100, T0), sr(100, T0)),
    ];
    let d = dr.decide(&peers);
    assert!(matches!(d.agreed_type, DecidedType::File));
    assert_eq!(d.winner.as_deref(), Some("B"));
}

// ---- 011: file decision ----

#[test]
fn req_011_1_canon_file_copied_to_all_peers_including_subordinates() {
    let dr = new();
    let peers = vec![
        ca("A", fe(100, T0), None),
        ct("B", LiveEntry::Absent, None),
        sb("C", LiveEntry::Absent, None),
    ];
    let d = dr.decide(&peers);
    assert_eq!(d.winner.as_deref(), Some("A"));
    assert!(matches!(act(&d, "B").conform, Conform::CopyWinner));
    assert!(matches!(act(&d, "C").conform, Conform::CopyWinner));
}

#[test]
fn req_011_2_canon_absent_file_removed_from_all_peers() {
    let dr = new();
    let peers = vec![
        ca("A", LiveEntry::Absent, None),
        ct("B", fe(100, T0), sr(100, T0)),
        sb("C", fe(100, T0), sr(100, T0)),
    ];
    let d = dr.decide(&peers);
    assert!(matches!(d.agreed_type, DecidedType::Absent));
    assert!(act(&d, "B").displace);
    assert!(act(&d, "C").displace);
}

#[test]
fn req_011_3_all_contributing_unchanged_matching_no_copy_among_them() {
    // A and B both match their rows and each other -> neither needs a copy.
    let dr = new();
    let peers = vec![
        ct("A", fe(100, T0),  sr(100, T0)),
        ct("B", fe(100, T_3S), sr(100, T0)), // T_3S within 5 s
    ];
    let d = dr.decide(&peers);
    assert!(matches!(act(&d, "A").conform, Conform::Nothing));
    assert!(matches!(act(&d, "B").conform, Conform::Nothing));
}

#[test]
fn req_011_4_all_unchanged_file_copied_to_active_peer_lacking_it() {
    // A is unchanged; subordinate B lacks the file -> B receives it.
    let dr = new();
    let peers = vec![
        ct("A", fe(100, T0), sr(100, T0)),
        sb("B", LiveEntry::Absent, None),
    ];
    let d = dr.decide(&peers);
    assert!(matches!(d.agreed_type, DecidedType::File));
    assert!(matches!(act(&d, "B").conform, Conform::CopyWinner));
}

#[test]
fn req_011_5_differing_modified_versions_newest_mod_time_wins() {
    // B is 10 s newer than A; B wins and A must receive B's version.
    let dr = new();
    let peers = vec![
        ct("A", fe(100, T0),   sr(100, T0)),
        ct("B", fe(100, T_10S), sr(100, T0)), // modified: mod_time changed
    ];
    let d = dr.decide(&peers);
    assert_eq!(d.winner.as_deref(), Some("B"));
    assert!(matches!(act(&d, "A").conform, Conform::CopyWinner));
}

#[test]
fn req_011_6_new_on_multiple_peers_newest_mod_time_governs() {
    // A and B are both new (no rows); B is newer -> B wins; A and C receive it.
    let dr = new();
    let peers = vec![
        ct("A", fe(100, T0),    None), // new
        ct("B", fe(100, T_10S), None), // new, newer
        ct("C", LiveEntry::Absent, None),
    ];
    let d = dr.decide(&peers);
    assert_eq!(d.winner.as_deref(), Some("B"));
    assert!(matches!(act(&d, "A").conform, Conform::CopyWinner));
    assert!(matches!(act(&d, "C").conform, Conform::CopyWinner));
}

#[test]
fn req_011_7_most_recent_deletion_estimate_governs() {
    // A's estimate T_3S (3 s, would not win) and B's estimate T_10S (10 s, wins).
    // Most recent (B's) must be used: deletion wins.
    let dr = new();
    let peers = vec![
        ct("A", LiveEntry::Absent, drow(100, T0, T_3S)),
        ct("B", LiveEntry::Absent, drow(100, T0, T_10S)),
        ct("C", fe(100, T0), sr(100, T0)),
    ];
    let d = dr.decide(&peers);
    assert!(matches!(d.agreed_type, DecidedType::Absent));
    assert!(act(&d, "C").displace);
}

#[test]
fn req_011_8_deletion_estimate_over_5s_removes_file() {
    // Deletion estimate T_10S exceeds file mod_time T0 by 10 s -> deletion wins.
    let dr = new();
    let peers = vec![
        ct("A", LiveEntry::Absent, drow(100, T0, T_10S)),
        ct("B", fe(100, T0), sr(100, T0)),
    ];
    let d = dr.decide(&peers);
    assert!(matches!(d.agreed_type, DecidedType::Absent));
    assert!(act(&d, "B").displace);
    assert!(matches!(act(&d, "B").conform, Conform::Nothing));
}

#[test]
fn req_011_9_deletion_estimate_within_5s_file_kept_and_copied() {
    // Deletion estimate T_3S is only 3 s after file mod_time T0 -> file kept; A receives it.
    let dr = new();
    let peers = vec![
        ct("A", LiveEntry::Absent, drow(100, T0, T_3S)),
        ct("B", fe(100, T0), sr(100, T0)),
    ];
    let d = dr.decide(&peers);
    assert!(matches!(d.agreed_type, DecidedType::File));
    assert_eq!(d.winner.as_deref(), Some("B"));
    assert!(matches!(act(&d, "A").conform, Conform::CopyWinner));
}

#[test]
fn req_011_10_absent_unconfirmed_last_seen_over_5s_casts_deletion_vote() {
    // A's last_seen (T_10S) exceeds file mod_time (T0) by 10 s > 5 s -> A casts a vote.
    // That vote is the deletion estimate; deletion wins.
    let dr = new();
    let peers = vec![
        ct("A", LiveEntry::Absent, ur(100, T0, Some(T_10S))),
        ct("B", fe(100, T0), sr(100, T0)),
    ];
    let d = dr.decide(&peers);
    assert!(matches!(d.agreed_type, DecidedType::Absent));
    assert!(act(&d, "B").displace);
}

#[test]
fn req_011_11_absent_unconfirmed_last_seen_within_5s_no_deletion_vote() {
    // A's last_seen (T_3S) is only 3 s after file mod_time (T0) -> no deletion vote.
    // File survives and is re-copied to A.
    let dr = new();
    let peers = vec![
        ct("A", LiveEntry::Absent, ur(100, T0, Some(T_3S))),
        ct("B", fe(100, T0), sr(100, T0)),
    ];
    let d = dr.decide(&peers);
    assert!(matches!(d.agreed_type, DecidedType::File));
    assert!(matches!(act(&d, "A").conform, Conform::CopyWinner));
}

#[test]
fn req_011_12_same_mod_time_larger_byte_size_wins() {
    let dr = new();
    let peers = vec![
        ct("A", fe(100, T0), sr(100, T0)),
        ct("B", fe(200, T0), sr(200, T0)), // same mod_time, larger byte_size
    ];
    let d = dr.decide(&peers);
    assert_eq!(d.winner.as_deref(), Some("B"));
    assert!(matches!(act(&d, "A").conform, Conform::CopyWinner));
}

#[test]
fn req_011_13_no_row_absent_peer_does_not_affect_winner_selection() {
    // C is absent with no row (no-opinion); winner is decided between A and B only.
    let dr = new();
    let with_c = vec![
        ct("A", fe(100, T0),    sr(100, T0)),
        ct("B", fe(200, T_10S), sr(200, T_10S)),
        ct("C", LiveEntry::Absent, None), // no row
    ];
    let without_c = vec![
        ct("A", fe(100, T0),    sr(100, T0)),
        ct("B", fe(200, T_10S), sr(200, T_10S)),
    ];
    let d1 = dr.decide(&with_c);
    let d2 = dr.decide(&without_c);
    assert_eq!(d1.winner.as_deref(), Some("B"));
    assert_eq!(d2.winner.as_deref(), Some("B"));
}

#[test]
fn req_011_14_no_row_absent_peer_receives_winning_file() {
    // B has no row and no file -> receives the winning file from A.
    let dr = new();
    let peers = vec![
        ct("A", fe(100, T0), sr(100, T0)),
        ct("B", LiveEntry::Absent, None), // no row
    ];
    let d = dr.decide(&peers);
    assert_eq!(d.winner.as_deref(), Some("A"));
    assert!(matches!(act(&d, "B").conform, Conform::CopyWinner));
}

#[test]
fn req_011_15_no_copy_to_peer_already_matching_winner() {
    // B's file (T_4S, 4 s after T0) is within 5 s and same byte_size -> already matches winner.
    let dr = new();
    let peers = vec![
        ct("A", fe(100, T0),  sr(100, T0)),
        ct("B", fe(100, T_4S), sr(100, T0)), // T_4S within 5 s of T0
    ];
    let d = dr.decide(&peers);
    assert!(!act(&d, "B").displace);
    assert!(matches!(act(&d, "B").conform, Conform::Nothing));
}

#[test]
fn req_011_16_within_5s_of_max_mod_time_treated_as_tied() {
    // A is 3 s behind B (within 5 s tie window) and has a larger byte_size -> A wins.
    let dr = new();
    let peers = vec![
        ct("A", fe(200, T_3S), sr(200, T_3S)), // within 5 s of T_6S
        ct("B", fe(100, T_6S), sr(100, T_6S)), // max mod_time
    ];
    let d = dr.decide(&peers);
    assert_eq!(d.winner.as_deref(), Some("A"));
}

#[test]
fn req_011_17_more_than_5s_behind_max_mod_time_loses() {
    // A is 10 s behind B -> A loses even though A has a larger byte_size.
    let dr = new();
    let peers = vec![
        ct("A", fe(200, T0),    sr(200, T0)),    // 10 s behind
        ct("B", fe(100, T_10S), sr(100, T_10S)), // max mod_time
    ];
    let d = dr.decide(&peers);
    assert_eq!(d.winner.as_deref(), Some("B"));
    assert!(matches!(act(&d, "A").conform, Conform::CopyWinner));
}

// ---- 012: directory decision ----

#[test]
fn req_012_1_contributing_dir_creates_on_every_peer_that_lacks_it() {
    let dr = new();
    let peers = vec![
        ct("A", LiveEntry::Directory, None),
        ct("B", LiveEntry::Absent, None),
    ];
    let d = dr.decide(&peers);
    assert!(matches!(d.agreed_type, DecidedType::Directory));
    assert!(matches!(act(&d, "B").conform, Conform::CreateDirectory));
}

#[test]
fn req_012_2_directory_decision_based_on_existence_not_mod_time() {
    // A has a directory live; B is absent with a row carrying a very different mod_time.
    // Outcome depends only on existence: B must get CreateDirectory.
    let dr = new();
    let peers = vec![
        ct("A", LiveEntry::Directory, dir_row(T0)),
        ct("B", LiveEntry::Absent, dir_row(T_20S)), // different mod_time, irrelevant
    ];
    let d = dr.decide(&peers);
    assert!(matches!(d.agreed_type, DecidedType::Directory));
    assert!(matches!(act(&d, "B").conform, Conform::CreateDirectory));
}

#[test]
fn req_012_3_no_live_dir_contributing_row_absent_displaces() {
    // A contributed a directory row but is now absent -> displace from any peer still holding it.
    let dr = new();
    let peers = vec![
        ct("A", LiveEntry::Absent, dir_row(T0)),
        sb("C", LiveEntry::Directory, None), // still has it
    ];
    let d = dr.decide(&peers);
    assert!(matches!(d.agreed_type, DecidedType::Absent));
    assert!(act(&d, "C").displace);
}

#[test]
fn req_012_4_no_row_contributing_peer_does_not_block_displacement() {
    // A has a directory row and is absent -> displacement is triggered.
    // B has no row -> does not block displacement.
    let dr = new();
    let peers = vec![
        ct("A", LiveEntry::Absent, dir_row(T0)),
        ct("B", LiveEntry::Absent, None), // no row, must not block
        sb("C", LiveEntry::Directory, None),
    ];
    let d = dr.decide(&peers);
    assert!(matches!(d.agreed_type, DecidedType::Absent));
    assert!(act(&d, "C").displace);
}

#[test]
fn req_012_5_no_live_no_row_displaces_only_subordinate_peers() {
    // No contributing peer has the directory live or a row for it.
    // Only the subordinate peer that still has it (C) is displaced.
    let dr = new();
    let peers = vec![
        ct("A", LiveEntry::Absent, None),
        ct("B", LiveEntry::Absent, None),
        sb("C", LiveEntry::Directory, None),
    ];
    let d = dr.decide(&peers);
    assert!(!act(&d, "A").displace);
    assert!(!act(&d, "B").displace);
    assert!(act(&d, "C").displace);
}

#[test]
fn req_012_6_canon_has_directory_creates_on_all_lacking_peers() {
    let dr = new();
    let peers = vec![
        ca("A", LiveEntry::Directory, None),
        ct("B", LiveEntry::Absent, None),
        sb("C", LiveEntry::Absent, None),
    ];
    let d = dr.decide(&peers);
    assert!(matches!(d.agreed_type, DecidedType::Directory));
    assert!(matches!(act(&d, "B").conform, Conform::CreateDirectory));
    assert!(matches!(act(&d, "C").conform, Conform::CreateDirectory));
}

#[test]
fn req_012_7_canon_lacks_directory_displaces_from_all_peers_that_have_it() {
    let dr = new();
    let peers = vec![
        ca("A", LiveEntry::Absent, None),
        ct("B", LiveEntry::Directory, None),
        sb("C", LiveEntry::Directory, None),
    ];
    let d = dr.decide(&peers);
    assert!(matches!(d.agreed_type, DecidedType::Absent));
    assert!(act(&d, "B").displace);
    assert!(act(&d, "C").displace);
}

#[test]
fn req_012_8_canon_has_file_conflict_displaces_conflicting_directories() {
    let dr = new();
    let peers = vec![
        ca("A", fe(100, T0), None),
        ct("B", LiveEntry::Directory, None), // conflict
    ];
    let d = dr.decide(&peers);
    assert!(matches!(d.agreed_type, DecidedType::File));
    assert!(act(&d, "B").displace);
}

#[test]
fn req_012_9_canon_has_file_conflict_syncs_canon_file_to_all_peers() {
    let dr = new();
    let peers = vec![
        ca("A", fe(100, T0), None),
        ct("B", LiveEntry::Directory, None),
    ];
    let d = dr.decide(&peers);
    assert_eq!(d.winner.as_deref(), Some("A"));
    assert!(act(&d, "B").displace);
    assert!(matches!(act(&d, "B").conform, Conform::CopyWinner));
}

#[test]
fn req_012_10_canon_has_dir_conflict_displaces_conflicting_files() {
    let dr = new();
    let peers = vec![
        ca("A", LiveEntry::Directory, None),
        ct("B", fe(100, T0), None), // conflict
    ];
    let d = dr.decide(&peers);
    assert!(matches!(d.agreed_type, DecidedType::Directory));
    assert!(act(&d, "B").displace);
}

#[test]
fn req_012_11_canon_has_dir_conflict_creates_directory_on_all_peers() {
    let dr = new();
    let peers = vec![
        ca("A", LiveEntry::Directory, None),
        ct("B", fe(100, T0), None),
    ];
    let d = dr.decide(&peers);
    assert!(act(&d, "B").displace);
    assert!(matches!(act(&d, "B").conform, Conform::CreateDirectory));
}

#[test]
fn req_012_12_canon_lacks_path_displaces_from_every_peer_that_has_it() {
    let dr = new();
    let peers = vec![
        ca("A", LiveEntry::Absent, None),
        ct("B", fe(100, T0), None),
        ct("C", LiveEntry::Directory, None),
    ];
    let d = dr.decide(&peers);
    assert!(matches!(d.agreed_type, DecidedType::Absent));
    assert!(act(&d, "B").displace);
    assert!(act(&d, "C").displace);
}

#[test]
fn req_012_13_no_canon_conflict_contributing_directory_displaced() {
    let dr = new();
    let peers = vec![
        ct("A", fe(100, T0), None),
        ct("B", LiveEntry::Directory, None), // conflict
    ];
    let d = dr.decide(&peers);
    assert!(act(&d, "B").displace);
}

#[test]
fn req_012_14_no_canon_conflict_winning_file_selected_and_synced_to_all() {
    // A is older; B is newer (winner); C has a conflicting directory; D lacks the file.
    // B wins by file rules; C's directory is displaced and C receives B's file; D receives it too.
    let dr = new();
    let peers = vec![
        ct("A", fe(100, T0),    None),              // older file
        ct("B", fe(200, T_10S), None),              // newer -> winner
        ct("C", LiveEntry::Directory, None),         // conflict
        ct("D", LiveEntry::Absent, None),
    ];
    let d = dr.decide(&peers);
    assert!(matches!(d.agreed_type, DecidedType::File));
    assert_eq!(d.winner.as_deref(), Some("B"));
    assert!(act(&d, "C").displace);
    assert!(matches!(act(&d, "C").conform, Conform::CopyWinner));
    assert!(matches!(act(&d, "D").conform, Conform::CopyWinner));
}

#[test]
fn req_012_15_subordinate_file_does_not_cause_file_to_win_over_contributing_dir() {
    // B is subordinate with a file; A is contributing with a directory.
    // B's file must not trigger the file-wins-over-directory conflict rule.
    // Directory wins; B's file is displaced and B receives the directory.
    let dr = new();
    let peers = vec![
        ct("A", LiveEntry::Directory, None),
        sb("B", fe(100, T0), None), // subordinate file
    ];
    let d = dr.decide(&peers);
    assert!(matches!(d.agreed_type, DecidedType::Directory));
    assert!(act(&d, "B").displace);
    assert!(matches!(act(&d, "B").conform, Conform::CreateDirectory));
}

#[test]
fn req_012_16_subordinate_with_wrong_type_displaced_after_contributing_decision() {
    // Contributing file wins; subordinate B holds a directory (wrong type) -> displaced.
    let dr = new();
    let peers = vec![
        ct("A", fe(100, T0), None),
        sb("B", LiveEntry::Directory, None), // wrong type
    ];
    let d = dr.decide(&peers);
    assert!(matches!(d.agreed_type, DecidedType::File));
    assert!(act(&d, "B").displace);
}

#[test]
fn req_012_17_subordinate_with_wrong_type_conformed_to_decided_type() {
    // B's directory is displaced and then B is conformed to the file decision.
    let dr = new();
    let peers = vec![
        ct("A", fe(100, T0), None),
        sb("B", LiveEntry::Directory, None), // wrong type -> displaced, then copy winner
    ];
    let d = dr.decide(&peers);
    assert!(act(&d, "B").displace);
    assert!(matches!(act(&d, "B").conform, Conform::CopyWinner));
}
