use copyqueue_stagingcleanup::PeerFs;
use copyqueue_stagingcleanup::StagingCleanup;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// A fixed reference point for "now": Unix epoch 1_000_000_000 (2001-09-09).
const NOW_SECS: u64 = 1_000_000_000;

fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(NOW_SECS)
}

fn ts_secs_ago(secs: u64) -> String {
    (NOW_SECS - secs).to_string()
}

fn ts_days_ago(days: u64) -> String {
    ts_secs_ago(days * 86400)
}

struct FakeFs {
    entries: HashMap<String, Vec<String>>,
    removed: Mutex<Vec<String>>,
}

impl FakeFs {
    fn new(entries: HashMap<String, Vec<String>>) -> Self {
        FakeFs { entries, removed: Mutex::new(Vec::new()) }
    }

    fn removed_paths(&self) -> Vec<String> {
        self.removed.lock().unwrap().clone()
    }
}

impl PeerFs for FakeFs {
    fn list(&self, path: &str) -> Vec<String> {
        self.entries.get(path).cloned().unwrap_or_default()
    }

    fn remove(&self, path: &str) {
        self.removed.lock().unwrap().push(path.to_string());
    }
}

fn make_entries(bak: Vec<&str>, tmp: Vec<&str>) -> HashMap<String, Vec<String>> {
    let mut m = HashMap::new();
    m.insert(".kitchensync/BAK".to_string(), bak.into_iter().map(String::from).collect());
    m.insert(".kitchensync/TMP".to_string(), tmp.into_iter().map(String::from).collect());
    m
}

// 021.11: BAK entry older than bak_keep_days is removed.
#[test]
fn bak_entry_older_than_limit_is_removed() {
    let old = ts_days_ago(91);
    let fs = FakeFs::new(make_entries(vec![&old], vec![]));

    let subject = copyqueue_stagingcleanup::new();
    subject.cleanup(&fs, "", Some(90), Some(2), now(), false);

    let removed = fs.removed_paths();
    assert!(
        removed.contains(&format!(".kitchensync/BAK/{}", old)),
        "expected old BAK entry to be removed; removed={:?}", removed
    );
}

// 021.12: TMP entry older than tmp_keep_days is removed.
#[test]
fn tmp_entry_older_than_limit_is_removed() {
    let old = ts_days_ago(3);
    let fs = FakeFs::new(make_entries(vec![], vec![&old]));

    let subject = copyqueue_stagingcleanup::new();
    subject.cleanup(&fs, "", Some(90), Some(2), now(), false);

    let removed = fs.removed_paths();
    assert!(
        removed.contains(&format!(".kitchensync/TMP/{}", old)),
        "expected old TMP entry to be removed; removed={:?}", removed
    );
}

// 021.13: Age is judged from the timestamp in the directory name, not filesystem mtime.
// We control "now" to be only 50 days after the entry timestamp, so the entry looks
// fresh by name even if wall-clock mtime could differ. Then we advance "now" to 100
// days after and the same entry is removed -- showing that only the named timestamp
// drives the decision.
#[test]
fn age_is_judged_from_directory_name_timestamp() {
    let entry_secs: u64 = 500_000_000;
    let entry_name = entry_secs.to_string();

    let fifty_days_later = UNIX_EPOCH + Duration::from_secs(entry_secs + 50 * 86400);
    let fs1 = FakeFs::new(make_entries(vec![&entry_name], vec![]));
    let subject = copyqueue_stagingcleanup::new();
    subject.cleanup(&fs1, "", Some(90), Some(2), fifty_days_later, false);
    assert!(
        fs1.removed_paths().is_empty(),
        "entry should be kept when named timestamp is only 50 days old relative to now"
    );

    let hundred_days_later = UNIX_EPOCH + Duration::from_secs(entry_secs + 100 * 86400);
    let fs2 = FakeFs::new(make_entries(vec![&entry_name], vec![]));
    subject.cleanup(&fs2, "", Some(90), Some(2), hundred_days_later, false);
    assert!(
        !fs2.removed_paths().is_empty(),
        "entry should be removed when named timestamp is 100 days old relative to now"
    );
}

// 021.14: BAK entry not older than bak_keep_days is left in place.
#[test]
fn recent_bak_entry_is_left_in_place() {
    let fresh = ts_days_ago(10);
    let fs = FakeFs::new(make_entries(vec![&fresh], vec![]));

    let subject = copyqueue_stagingcleanup::new();
    subject.cleanup(&fs, "", Some(90), Some(2), now(), false);

    assert!(fs.removed_paths().is_empty(), "10-day-old BAK entry must not be removed with 90-day limit");
}

// 021.15: TMP entry not older than tmp_keep_days is left in place.
#[test]
fn recent_tmp_entry_is_left_in_place() {
    let fresh = ts_days_ago(1);
    let fs = FakeFs::new(make_entries(vec![], vec![&fresh]));

    let subject = copyqueue_stagingcleanup::new();
    subject.cleanup(&fs, "", Some(90), Some(2), now(), false);

    assert!(fs.removed_paths().is_empty(), "1-day-old TMP entry must not be removed with 2-day limit");
}

// 021.16: SWAP entries are never removed on the basis of age.
#[test]
fn swap_entries_are_never_removed() {
    let old = ts_days_ago(1000);
    let mut entries = make_entries(vec![], vec![]);
    entries.insert(".kitchensync/SWAP".to_string(), vec![old]);
    let fs = FakeFs::new(entries);

    let subject = copyqueue_stagingcleanup::new();
    // Use tight limits (1 day) so any age-based removal would fire -- SWAP must survive.
    subject.cleanup(&fs, "", Some(1), Some(1), now(), false);

    let removed = fs.removed_paths();
    let swap_removed: Vec<_> = removed.iter().filter(|p| p.contains("SWAP")).collect();
    assert!(swap_removed.is_empty(), "SWAP entries must never be removed; removed={:?}", removed);
}

// 021.17: Default BAK retention is 90 days (None selects the default).
#[test]
fn default_bak_retention_is_90_days() {
    let old = ts_days_ago(91);
    let fresh = ts_days_ago(89);
    let fs = FakeFs::new(make_entries(vec![&old, &fresh], vec![]));

    let subject = copyqueue_stagingcleanup::new();
    subject.cleanup(&fs, "", None, Some(2), now(), false);

    let removed = fs.removed_paths();
    assert!(
        removed.iter().any(|p| p.contains(&old)),
        "91-day-old BAK entry should be removed by the 90-day default; removed={:?}", removed
    );
    assert!(
        !removed.iter().any(|p| p.contains(&fresh)),
        "89-day-old BAK entry must not be removed by the 90-day default; removed={:?}", removed
    );
}

// 021.18: Default TMP retention is 2 days (None selects the default).
#[test]
fn default_tmp_retention_is_2_days() {
    let old = ts_days_ago(3);
    let fresh = ts_days_ago(1);
    let fs = FakeFs::new(make_entries(vec![], vec![&old, &fresh]));

    let subject = copyqueue_stagingcleanup::new();
    subject.cleanup(&fs, "", Some(90), None, now(), false);

    let removed = fs.removed_paths();
    assert!(
        removed.iter().any(|p| p.contains(&old)),
        "3-day-old TMP entry should be removed by the 2-day default; removed={:?}", removed
    );
    assert!(
        !removed.iter().any(|p| p.contains(&fresh)),
        "1-day-old TMP entry must not be removed by the 2-day default; removed={:?}", removed
    );
}

// 021.19, 024.19: In dry-run, no peer state is mutated.
#[test]
fn dry_run_skips_all_cleanup() {
    let old_bak = ts_days_ago(91);
    let old_tmp = ts_days_ago(3);
    let fs = FakeFs::new(make_entries(vec![&old_bak], vec![&old_tmp]));

    let subject = copyqueue_stagingcleanup::new();
    subject.cleanup(&fs, "", Some(90), Some(2), now(), true);

    assert!(
        fs.removed_paths().is_empty(),
        "dry-run must not remove any entries; removed={:?}", fs.removed_paths()
    );
}

// 021.10: Cleanup purges .kitchensync/ even though it is excluded from sync listings.
// Exercises a non-empty dir_path to confirm the correct .kitchensync/ sub-path is used.
#[test]
fn cleanup_operates_on_kitchensync_subpath_of_given_dir() {
    let old = ts_days_ago(91);
    let mut entries = HashMap::new();
    entries.insert("docs/.kitchensync/BAK".to_string(), vec![old.clone()]);
    entries.insert("docs/.kitchensync/TMP".to_string(), vec![]);
    let fs = FakeFs::new(entries);

    let subject = copyqueue_stagingcleanup::new();
    subject.cleanup(&fs, "docs", Some(90), Some(2), now(), false);

    let removed = fs.removed_paths();
    assert!(
        removed.contains(&format!("docs/.kitchensync/BAK/{}", old)),
        "should remove aged BAK entry under a dir's .kitchensync/; removed={:?}", removed
    );
}
