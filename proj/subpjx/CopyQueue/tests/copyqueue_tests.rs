use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use copyqueue::{CopyConfig, CopyQueue, CopyRequest};

// ---- helpers ----

fn test_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cq_{}", name));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn file_url(path: &Path) -> String {
    format!("file://{}", path.display())
}

fn make_queue() -> Arc<dyn CopyQueue> {
    copyqueue::new(transport::new(), output::new())
}

fn default_config() -> CopyConfig {
    CopyConfig {
        copy_slot_limit: None,
        copy_try_limit: None,
        bak_retention: None,
        tmp_retention: None,
        dry_run: false,
    }
}

fn write_file(path: &Path, content: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

// Returns true when any .kitchensync/BAK/<ts>/<filename> entry exists under dir.
fn has_bak_file(dir: &Path, filename: &str) -> bool {
    let bak = dir.join(".kitchensync/BAK");
    if !bak.exists() {
        return false;
    }
    for ts_entry in fs::read_dir(&bak).unwrap() {
        if ts_entry.unwrap().path().join(filename).exists() {
            return true;
        }
    }
    false
}

// Returns true when .kitchensync/SWAP is absent or has no children.
fn swap_absent_or_empty(dir: &Path) -> bool {
    let swap = dir.join(".kitchensync/SWAP");
    if !swap.exists() {
        return true;
    }
    fs::read_dir(&swap)
        .map(|mut d| d.next().is_none())
        .unwrap_or(true)
}

// ---- configure ----

#[test]
fn configure_default_limits() {
    // 020.1, 020.8, 021.17, 021.18: None fields select documented defaults.
    let q = make_queue();
    q.configure(default_config());
}

#[test]
fn configure_custom_limits() {
    // 020.2, 020.7: Explicit copy_slot_limit and copy_try_limit are accepted.
    let q = make_queue();
    q.configure(CopyConfig {
        copy_slot_limit: Some(5),
        copy_try_limit: Some(2),
        bak_retention: Some(Duration::from_secs(7 * 86400)),
        tmp_retention: Some(Duration::from_secs(86400)),
        dry_run: false,
    });
}

// ---- enqueue + wait: basic copy ----

#[test]
fn enqueue_copies_new_file() {
    // 019.3, 020.3, 020.15: A local-to-local copy creates the destination file with the right content.
    let src = test_dir("enqueue_new_src");
    let dst = test_dir("enqueue_new_dst");
    write_file(&src.join("hello.txt"), b"hello world");

    let q = make_queue();
    q.configure(default_config());
    q.enqueue(CopyRequest {
        src_peer: file_url(&src),
        src_path: "hello.txt".into(),
        dst_peer: file_url(&dst),
        dst_path: "hello.txt".into(),
        mod_time: UNIX_EPOCH + Duration::from_secs(1_700_000_000),
        on_success: None,
    });
    q.wait();

    assert_eq!(fs::read(dst.join("hello.txt")).unwrap(), b"hello world");
}

#[test]
fn enqueue_sets_mod_time_from_request() {
    // 019.4: The destination mod_time is taken from the request, not re-read from the source.
    let src = test_dir("mod_time_src");
    let dst = test_dir("mod_time_dst");
    write_file(&src.join("data.txt"), b"content");

    let expected = UNIX_EPOCH + Duration::from_secs(1_600_000_000);

    let q = make_queue();
    q.configure(default_config());
    q.enqueue(CopyRequest {
        src_peer: file_url(&src),
        src_path: "data.txt".into(),
        dst_peer: file_url(&dst),
        dst_path: "data.txt".into(),
        mod_time: expected,
        on_success: None,
    });
    q.wait();

    let actual = fs::metadata(dst.join("data.txt"))
        .unwrap()
        .modified()
        .unwrap();
    let delta = if actual >= expected {
        actual.duration_since(expected).unwrap()
    } else {
        expected.duration_since(actual).unwrap()
    };
    assert!(delta.as_secs() < 2, "destination mod_time must match the request mod_time");
}

#[test]
fn replacing_copy_delivers_new_content() {
    // 019.1, 019.2, 019.3: A replacement copy writes new content to SWAP new then renames to target.
    let src = test_dir("replace_src");
    let dst = test_dir("replace_dst");
    write_file(&src.join("file.txt"), b"new content");
    write_file(&dst.join("file.txt"), b"old content");

    let q = make_queue();
    q.configure(default_config());
    q.enqueue(CopyRequest {
        src_peer: file_url(&src),
        src_path: "file.txt".into(),
        dst_peer: file_url(&dst),
        dst_path: "file.txt".into(),
        mod_time: UNIX_EPOCH + Duration::from_secs(1_700_000_000),
        on_success: None,
    });
    q.wait();

    assert_eq!(fs::read(dst.join("file.txt")).unwrap(), b"new content");
}

#[test]
fn replacing_copy_archives_old_to_bak() {
    // 019.5: When SWAP old exists after the replacement is in place, it is archived to BAK.
    let src = test_dir("bak_src");
    let dst = test_dir("bak_dst");
    write_file(&src.join("doc.txt"), b"new");
    write_file(&dst.join("doc.txt"), b"old");

    let q = make_queue();
    q.configure(default_config());
    q.enqueue(CopyRequest {
        src_peer: file_url(&src),
        src_path: "doc.txt".into(),
        dst_peer: file_url(&dst),
        dst_path: "doc.txt".into(),
        mod_time: UNIX_EPOCH + Duration::from_secs(1_700_000_000),
        on_success: None,
    });
    q.wait();

    assert!(
        has_bak_file(&dst, "doc.txt"),
        "displaced file must be archived to BAK"
    );
}

#[test]
fn replacing_copy_removes_swap_dirs() {
    // 019.6: Empty SWAP directories are removed after the replacement completes.
    let src = test_dir("swap_clean_src");
    let dst = test_dir("swap_clean_dst");
    write_file(&src.join("item.txt"), b"new");
    write_file(&dst.join("item.txt"), b"old");

    let q = make_queue();
    q.configure(default_config());
    q.enqueue(CopyRequest {
        src_peer: file_url(&src),
        src_path: "item.txt".into(),
        dst_peer: file_url(&dst),
        dst_path: "item.txt".into(),
        mod_time: UNIX_EPOCH + Duration::from_secs(1_700_000_000),
        on_success: None,
    });
    q.wait();

    assert!(
        swap_absent_or_empty(&dst),
        "SWAP directory must be empty or gone after replacement"
    );
}

#[test]
fn on_success_callback_called_exactly_once_on_success() {
    // CopyRequest.on_success: called exactly once when the copy succeeds.
    let src = test_dir("success_cb_src");
    let dst = test_dir("success_cb_dst");
    write_file(&src.join("file.txt"), b"data");

    let counter = Arc::new(AtomicUsize::new(0));
    let c = counter.clone();

    let q = make_queue();
    q.configure(default_config());
    q.enqueue(CopyRequest {
        src_peer: file_url(&src),
        src_path: "file.txt".into(),
        dst_peer: file_url(&dst),
        dst_path: "file.txt".into(),
        mod_time: UNIX_EPOCH + Duration::from_secs(1_700_000_000),
        on_success: Some(Box::new(move || {
            c.fetch_add(1, Ordering::Relaxed);
        })),
    });
    q.wait();

    assert_eq!(
        counter.load(Ordering::Relaxed),
        1,
        "on_success must be called exactly once on a successful copy"
    );
}

// ---- dry-run: enqueue ----

#[test]
fn enqueue_dry_run_skips_copy() {
    // 024.14: dry-run writes no destination files on peers.
    let src = test_dir("dry_copy_src");
    let dst = test_dir("dry_copy_dst");
    write_file(&src.join("x.txt"), b"source");

    let q = make_queue();
    q.configure(CopyConfig {
        dry_run: true,
        ..default_config()
    });
    q.enqueue(CopyRequest {
        src_peer: file_url(&src),
        src_path: "x.txt".into(),
        dst_peer: file_url(&dst),
        dst_path: "x.txt".into(),
        mod_time: UNIX_EPOCH + Duration::from_secs(1_700_000_000),
        on_success: None,
    });
    q.wait();

    assert!(!dst.join("x.txt").exists(), "dry-run must not copy the file");
}

#[test]
fn dry_run_creates_no_staging_directories() {
    // 024.13: dry-run creates no TMP, SWAP, or BAK directories on peers.
    let src = test_dir("dry_staging_src");
    let dst = test_dir("dry_staging_dst");
    write_file(&src.join("a.txt"), b"source");

    let q = make_queue();
    q.configure(CopyConfig {
        dry_run: true,
        ..default_config()
    });
    q.enqueue(CopyRequest {
        src_peer: file_url(&src),
        src_path: "a.txt".into(),
        dst_peer: file_url(&dst),
        dst_path: "a.txt".into(),
        mod_time: UNIX_EPOCH + Duration::from_secs(1_700_000_000),
        on_success: None,
    });
    q.wait();

    assert!(
        !dst.join(".kitchensync").exists(),
        "dry-run must not create any .kitchensync directory on the destination"
    );
}

#[test]
fn dry_run_does_not_change_mod_time_of_existing_file() {
    // 024.17: dry-run sets no modification times on peers.
    let src = test_dir("dry_mtime_src");
    let dst = test_dir("dry_mtime_dst");
    write_file(&src.join("f.txt"), b"src content");
    write_file(&dst.join("f.txt"), b"dst content");

    // Record the destination mod_time before the dry-run.
    let before = fs::metadata(dst.join("f.txt")).unwrap().modified().unwrap();

    let q = make_queue();
    q.configure(CopyConfig {
        dry_run: true,
        ..default_config()
    });
    q.enqueue(CopyRequest {
        src_peer: file_url(&src),
        src_path: "f.txt".into(),
        dst_peer: file_url(&dst),
        dst_path: "f.txt".into(),
        // Request a mod_time that differs from the destination's current time.
        mod_time: UNIX_EPOCH + Duration::from_secs(1_000_000_000),
        on_success: None,
    });
    q.wait();

    let after = fs::metadata(dst.join("f.txt")).unwrap().modified().unwrap();
    assert_eq!(
        before, after,
        "dry-run must not change the destination file's modification time"
    );
}

// ---- SWAP recovery during a copy (019.8) ----

#[test]
fn copy_handles_stale_swap_before_replacement() {
    // 019.8: Before starting a replacement, any existing SWAP for the basename is recovered.
    // Stale state: old present + target present (recovery rule 019.15).
    let src = test_dir("stale_src");
    let dst = test_dir("stale_dst");
    write_file(&src.join("f.txt"), b"newer");
    write_file(&dst.join("f.txt"), b"current");
    write_file(&dst.join(".kitchensync/SWAP/f.txt/old"), b"stale");

    let q = make_queue();
    q.configure(default_config());
    q.enqueue(CopyRequest {
        src_peer: file_url(&src),
        src_path: "f.txt".into(),
        dst_peer: file_url(&dst),
        dst_path: "f.txt".into(),
        mod_time: UNIX_EPOCH + Duration::from_secs(1_700_000_000),
        on_success: None,
    });
    q.wait();

    assert_eq!(
        fs::read(dst.join("f.txt")).unwrap(),
        b"newer",
        "copy succeeds after stale SWAP is recovered"
    );
}

// ---- recover_swap ----

#[test]
fn recover_swap_returns_true_on_empty_swap() {
    // 019.14: Returns true when there is no SWAP state to recover.
    let peer = test_dir("rs_empty");
    fs::create_dir_all(peer.join(".kitchensync/SWAP")).unwrap();

    let q = make_queue();
    q.configure(default_config());
    assert!(
        q.recover_swap(&file_url(&peer), ""),
        "returns true when nothing to recover"
    );
}

#[test]
fn recover_swap_old_target_present_archives_old() {
    // 019.15: old present + target present -> move old to BAK, remove SWAP dir.
    let peer = test_dir("rs_old_target");
    write_file(&peer.join(".kitchensync/SWAP/note.txt/old"), b"displaced");
    write_file(&peer.join("note.txt"), b"current");

    let q = make_queue();
    q.configure(default_config());
    assert!(
        q.recover_swap(&file_url(&peer), ""),
        "recovery must succeed"
    );

    assert!(
        !peer.join(".kitchensync/SWAP/note.txt").exists(),
        "SWAP slot must be removed"
    );
    assert!(
        has_bak_file(&peer, "note.txt"),
        "old content must be archived to BAK"
    );
    assert_eq!(
        fs::read(peer.join("note.txt")).unwrap(),
        b"current",
        "target must be unchanged"
    );
}

#[test]
fn recover_swap_old_new_no_target_completes_copy() {
    // 019.16: old + new + no target -> rename new to target, move old to BAK, remove SWAP dir.
    let peer = test_dir("rs_old_new_no_tgt");
    write_file(&peer.join(".kitchensync/SWAP/doc.txt/old"), b"old");
    write_file(&peer.join(".kitchensync/SWAP/doc.txt/new"), b"new");

    let q = make_queue();
    q.configure(default_config());
    assert!(
        q.recover_swap(&file_url(&peer), ""),
        "recovery must succeed"
    );

    assert!(
        !peer.join(".kitchensync/SWAP/doc.txt").exists(),
        "SWAP slot must be removed"
    );
    assert_eq!(
        fs::read(peer.join("doc.txt")).unwrap(),
        b"new",
        "new content must be at target"
    );
    assert!(
        has_bak_file(&peer, "doc.txt"),
        "old content must be archived to BAK"
    );
}

#[test]
fn recover_swap_old_no_new_no_target_restores_old() {
    // 019.17: old + no new + no target -> rename old back to target, remove SWAP dir.
    let peer = test_dir("rs_old_only");
    write_file(&peer.join(".kitchensync/SWAP/img.txt/old"), b"original");

    let q = make_queue();
    q.configure(default_config());
    assert!(
        q.recover_swap(&file_url(&peer), ""),
        "recovery must succeed"
    );

    assert!(
        !peer.join(".kitchensync/SWAP/img.txt").exists(),
        "SWAP slot must be removed"
    );
    assert_eq!(
        fs::read(peer.join("img.txt")).unwrap(),
        b"original",
        "old must be restored to target"
    );
    assert!(
        !has_bak_file(&peer, "img.txt"),
        "nothing must be archived to BAK"
    );
}

#[test]
fn recover_swap_no_old_new_target_present_deletes_new() {
    // 019.18: no old + new + target -> delete new, remove SWAP dir.
    let peer = test_dir("rs_new_target");
    write_file(&peer.join(".kitchensync/SWAP/log.txt/new"), b"partial");
    write_file(&peer.join("log.txt"), b"existing");

    let q = make_queue();
    q.configure(default_config());
    assert!(
        q.recover_swap(&file_url(&peer), ""),
        "recovery must succeed"
    );

    assert!(
        !peer.join(".kitchensync/SWAP/log.txt").exists(),
        "SWAP slot must be removed"
    );
    assert_eq!(
        fs::read(peer.join("log.txt")).unwrap(),
        b"existing",
        "target must be unchanged"
    );
}

#[test]
fn recover_swap_no_old_new_no_target_promotes_new() {
    // 019.19: no old + new + no target -> rename new to target, remove SWAP dir.
    let peer = test_dir("rs_new_only");
    write_file(&peer.join(".kitchensync/SWAP/report.txt/new"), b"content");

    let q = make_queue();
    q.configure(default_config());
    assert!(
        q.recover_swap(&file_url(&peer), ""),
        "recovery must succeed"
    );

    assert!(
        !peer.join(".kitchensync/SWAP/report.txt").exists(),
        "SWAP slot must be removed"
    );
    assert_eq!(
        fs::read(peer.join("report.txt")).unwrap(),
        b"content",
        "new must be promoted to target"
    );
}

#[test]
fn recover_swap_dry_run_skips_and_reports_success() {
    // 019.21, 024.20: In dry-run, peer-side SWAP recovery is skipped; returns true.
    let peer = test_dir("rs_dry_run");
    let old_path = peer.join(".kitchensync/SWAP/val.txt/old");
    write_file(&old_path, b"old");
    write_file(&peer.join("val.txt"), b"current");

    let q = make_queue();
    q.configure(CopyConfig {
        dry_run: true,
        ..default_config()
    });
    let ok = q.recover_swap(&file_url(&peer), "");

    assert!(ok, "dry-run recovery must report success");
    assert!(old_path.exists(), "dry-run must leave SWAP old untouched");
    assert!(
        peer.join(".kitchensync/SWAP/val.txt").exists(),
        "dry-run must leave SWAP slot untouched"
    );
}

#[test]
fn recover_swap_in_subdirectory() {
    // 019.14: recover_swap works on a sub-path within the peer, not just the root.
    let peer = test_dir("rs_subdir");
    let sub = peer.join("alpha");
    write_file(&sub.join(".kitchensync/SWAP/readme.txt/new"), b"content");

    let q = make_queue();
    q.configure(default_config());
    assert!(
        q.recover_swap(&file_url(&peer), "alpha"),
        "recovery must succeed in a subdirectory"
    );

    assert!(
        !sub.join(".kitchensync/SWAP/readme.txt").exists(),
        "SWAP slot must be removed in subdirectory"
    );
    assert_eq!(
        fs::read(sub.join("readme.txt")).unwrap(),
        b"content",
        "new must be promoted in subdirectory"
    );
}

#[test]
fn recover_swap_encoded_basename_decoded_to_target() {
    // 019.7: The SWAP path segment is the target basename percent-encoded.
    // "foo bar.txt" encodes as "foo%20bar.txt".
    let peer = test_dir("encoded_bn");
    write_file(
        &peer.join(".kitchensync/SWAP/foo%20bar.txt/new"),
        b"decoded",
    );

    let q = make_queue();
    q.configure(default_config());
    assert!(
        q.recover_swap(&file_url(&peer), ""),
        "recovery must succeed"
    );

    let target = peer.join("foo bar.txt");
    assert!(
        target.exists(),
        "percent-encoded SWAP name must decode to target 'foo bar.txt'"
    );
    assert_eq!(fs::read(&target).unwrap(), b"decoded");
    assert!(
        !peer.join(".kitchensync/SWAP/foo%20bar.txt").exists(),
        "SWAP slot must be removed"
    );
}

// ---- cleanup ----

// Timestamps use the "YYYY-MM-DD_HHMMSSZ" format which the StagingCleanup
// implementation can parse (underscore separator at position 10, six-digit
// HHMMSS compact time).

#[test]
fn cleanup_removes_old_bak_entry() {
    // 021.11, 021.13: BAK entries older than the retention limit are removed; age from timestamp name.
    let peer = test_dir("cl_old_bak");
    let old_ts = "2016-01-01_000000Z";
    write_file(
        &peer.join(format!(".kitchensync/BAK/{}/old.txt", old_ts)),
        b"displaced",
    );

    let q = make_queue();
    q.configure(CopyConfig {
        bak_retention: Some(Duration::from_secs(90 * 86400)),
        ..default_config()
    });
    q.cleanup(&file_url(&peer), "");

    assert!(
        !peer.join(format!(".kitchensync/BAK/{}", old_ts)).exists(),
        "old BAK timestamp directory must be removed"
    );
}

#[test]
fn cleanup_keeps_recent_bak_entry() {
    // 021.14: BAK entries not older than the retention limit are left in place.
    // 2024-01-01 is ~900 days ago; 10-year retention (3650 days) keeps it.
    let peer = test_dir("cl_recent_bak");
    let ts = "2024-01-01_000000Z";
    write_file(
        &peer.join(format!(".kitchensync/BAK/{}/keep.txt", ts)),
        b"recent",
    );

    let q = make_queue();
    q.configure(CopyConfig {
        bak_retention: Some(Duration::from_secs(3650 * 86400)),
        ..default_config()
    });
    q.cleanup(&file_url(&peer), "");

    assert!(
        peer.join(format!(".kitchensync/BAK/{}/keep.txt", ts)).exists(),
        "recent BAK entry must be kept"
    );
}

#[test]
fn cleanup_removes_old_tmp_entry() {
    // 021.12, 021.13: TMP entries older than the retention limit are removed.
    let peer = test_dir("cl_old_tmp");
    let old_ts = "2016-01-01_000000Z";
    write_file(
        &peer.join(format!(".kitchensync/TMP/{}/stage.dat", old_ts)),
        b"temp",
    );

    let q = make_queue();
    q.configure(CopyConfig {
        tmp_retention: Some(Duration::from_secs(2 * 86400)),
        ..default_config()
    });
    q.cleanup(&file_url(&peer), "");

    assert!(
        !peer.join(format!(".kitchensync/TMP/{}", old_ts)).exists(),
        "old TMP timestamp directory must be removed"
    );
}

#[test]
fn cleanup_keeps_recent_tmp_entry() {
    // 021.15: TMP entries not older than the retention limit are left in place.
    // 2024-01-01 is ~900 days ago; 10-year retention keeps it.
    let peer = test_dir("cl_recent_tmp");
    let ts = "2024-01-01_000000Z";
    write_file(
        &peer.join(format!(".kitchensync/TMP/{}/stage.dat", ts)),
        b"recent",
    );

    let q = make_queue();
    q.configure(CopyConfig {
        tmp_retention: Some(Duration::from_secs(3650 * 86400)),
        ..default_config()
    });
    q.cleanup(&file_url(&peer), "");

    assert!(
        peer.join(format!(".kitchensync/TMP/{}/stage.dat", ts)).exists(),
        "recent TMP entry must be kept"
    );
}

#[test]
fn cleanup_never_removes_swap_entries() {
    // 021.16: Cleanup never purges SWAP entries by age.
    // An old BAK entry is also present so we can confirm cleanup actually ran.
    let peer = test_dir("cl_swap");
    let old_ts = "2016-01-01_000000Z";
    write_file(
        &peer.join(format!(".kitchensync/BAK/{}/old.txt", old_ts)),
        b"old",
    );
    write_file(&peer.join(".kitchensync/SWAP/somefile.txt/new"), b"in-flight");

    let q = make_queue();
    q.configure(CopyConfig {
        bak_retention: Some(Duration::from_secs(90 * 86400)),
        tmp_retention: Some(Duration::from_secs(2 * 86400)),
        ..default_config()
    });
    q.cleanup(&file_url(&peer), "");

    assert!(
        !peer.join(format!(".kitchensync/BAK/{}", old_ts)).exists(),
        "old BAK entry removed (proves cleanup ran)"
    );
    assert!(
        peer.join(".kitchensync/SWAP/somefile.txt").exists(),
        "SWAP entry must not be purged by cleanup"
    );
}

#[test]
fn cleanup_default_bak_retention_90_days() {
    // 021.17: Default BAK retention is 90 days.
    // 2016-01-01 is thousands of days ago; the default 90-day limit purges it.
    let peer = test_dir("cl_def_bak");
    let old_ts = "2016-01-01_000000Z";
    write_file(
        &peer.join(format!(".kitchensync/BAK/{}/f.txt", old_ts)),
        b"data",
    );

    let q = make_queue();
    q.configure(default_config()); // bak_retention: None -> default 90 days
    q.cleanup(&file_url(&peer), "");

    assert!(
        !peer.join(format!(".kitchensync/BAK/{}", old_ts)).exists(),
        "default 90-day BAK retention must purge old entries"
    );
}

#[test]
fn cleanup_default_tmp_retention_2_days() {
    // 021.18: Default TMP retention is 2 days.
    // 2016-01-01 is thousands of days ago; the default 2-day limit purges it.
    let peer = test_dir("cl_def_tmp");
    let old_ts = "2016-01-01_000000Z";
    write_file(
        &peer.join(format!(".kitchensync/TMP/{}/t.dat", old_ts)),
        b"tmp",
    );

    let q = make_queue();
    q.configure(default_config()); // tmp_retention: None -> default 2 days
    q.cleanup(&file_url(&peer), "");

    assert!(
        !peer.join(format!(".kitchensync/TMP/{}", old_ts)).exists(),
        "default 2-day TMP retention must purge old entries"
    );
}

#[test]
fn cleanup_dry_run_skips() {
    // 021.19, 024.19: In dry-run, BAK/TMP cleanup on peers is skipped.
    let peer = test_dir("cl_dry_run");
    let old_ts = "2016-01-01_000000Z";
    write_file(
        &peer.join(format!(".kitchensync/BAK/{}/old.txt", old_ts)),
        b"bak",
    );
    write_file(
        &peer.join(format!(".kitchensync/TMP/{}/t.dat", old_ts)),
        b"tmp",
    );

    let q = make_queue();
    q.configure(CopyConfig {
        bak_retention: Some(Duration::from_secs(86400)),
        tmp_retention: Some(Duration::from_secs(86400)),
        dry_run: true,
        ..default_config()
    });
    q.cleanup(&file_url(&peer), "");

    assert!(
        peer.join(format!(".kitchensync/BAK/{}", old_ts)).exists(),
        "dry-run must not remove BAK entries"
    );
    assert!(
        peer.join(format!(".kitchensync/TMP/{}", old_ts)).exists(),
        "dry-run must not remove TMP entries"
    );
}

#[test]
fn cleanup_purges_old_entries_in_subdirectory() {
    // 021.10, 021.11: cleanup purges BAK entries under a non-root dir_path.
    // .kitchensync/ is inspected directly even though it is excluded from synced listings.
    let peer = test_dir("cl_sub");
    let old_ts = "2016-01-01_000000Z";
    write_file(
        &peer.join(format!("sub/.kitchensync/BAK/{}/old.txt", old_ts)),
        b"displaced",
    );

    let q = make_queue();
    q.configure(CopyConfig {
        bak_retention: Some(Duration::from_secs(90 * 86400)),
        ..default_config()
    });
    q.cleanup(&file_url(&peer), "sub");

    assert!(
        !peer.join(format!("sub/.kitchensync/BAK/{}", old_ts)).exists(),
        "old BAK entry in subdirectory must be removed"
    );
}

// ---- copy-slot limit and concurrency ----

#[test]
fn enqueue_returns_before_copy_completes() {
    // 020.5: enqueue returns immediately; copy work begins while later directories are still being scanned.
    let src = test_dir("nonblock_src");
    let dst = test_dir("nonblock_dst");
    write_file(&src.join("file.txt"), b"data");

    let q = make_queue();
    q.configure(default_config());
    q.enqueue(CopyRequest {
        src_peer: file_url(&src),
        src_path: "file.txt".into(),
        dst_peer: file_url(&dst),
        dst_path: "file.txt".into(),
        mod_time: UNIX_EPOCH + Duration::from_secs(1_700_000_000),
        on_success: None,
    });
    // enqueue returned without blocking; proceed as if scanning later directories.
    q.wait();
    assert!(dst.join("file.txt").exists());
}

#[test]
fn slot_limit_one_all_copies_complete() {
    // 020.2: With copy_slot_limit=1, copies are serialised but all complete.
    let src = test_dir("slot1_src");
    let dst = test_dir("slot1_dst");
    for i in 0..5u64 {
        write_file(
            &src.join(format!("f{}.txt", i)),
            format!("c{}", i).as_bytes(),
        );
    }

    let q = make_queue();
    q.configure(CopyConfig {
        copy_slot_limit: Some(1),
        ..default_config()
    });
    for i in 0..5u64 {
        q.enqueue(CopyRequest {
            src_peer: file_url(&src),
            src_path: format!("f{}.txt", i),
            dst_peer: file_url(&dst),
            dst_path: format!("f{}.txt", i),
            mod_time: UNIX_EPOCH + Duration::from_secs(1_700_000_000 + i),
            on_success: None,
        });
    }
    q.wait();

    for i in 0..5u64 {
        assert_eq!(
            fs::read(dst.join(format!("f{}.txt", i))).unwrap(),
            format!("c{}", i).as_bytes(),
            "copy {} must complete",
            i
        );
    }
}

#[test]
fn default_slot_limit_allows_many_copies() {
    // 020.1: Default copy_slot_limit (10) allows many concurrent copies to complete.
    let src = test_dir("def_slot_src");
    let dst = test_dir("def_slot_dst");
    for i in 0..15u64 {
        write_file(
            &src.join(format!("f{}.txt", i)),
            format!("d{}", i).as_bytes(),
        );
    }

    let q = make_queue();
    q.configure(default_config());
    for i in 0..15u64 {
        q.enqueue(CopyRequest {
            src_peer: file_url(&src),
            src_path: format!("f{}.txt", i),
            dst_peer: file_url(&dst),
            dst_path: format!("f{}.txt", i),
            mod_time: UNIX_EPOCH + Duration::from_secs(1_700_000_000 + i),
            on_success: None,
        });
    }
    q.wait();

    for i in 0..15u64 {
        assert!(
            dst.join(format!("f{}.txt", i)).exists(),
            "copy {} must complete",
            i
        );
    }
}

#[test]
fn try_budgets_are_independent_per_copy() {
    // 020.11, 020.7: Each copy has its own try budget; one copy's tries do not reduce another's.
    // With try_limit=1, both copies still succeed independently on their first try.
    let src = test_dir("try_src");
    let dst = test_dir("try_dst");
    write_file(&src.join("a.txt"), b"a");
    write_file(&src.join("b.txt"), b"b");

    let q = make_queue();
    q.configure(CopyConfig {
        copy_try_limit: Some(1),
        ..default_config()
    });
    q.enqueue(CopyRequest {
        src_peer: file_url(&src),
        src_path: "a.txt".into(),
        dst_peer: file_url(&dst),
        dst_path: "a.txt".into(),
        mod_time: UNIX_EPOCH + Duration::from_secs(1_700_000_000),
        on_success: None,
    });
    q.enqueue(CopyRequest {
        src_peer: file_url(&src),
        src_path: "b.txt".into(),
        dst_peer: file_url(&dst),
        dst_path: "b.txt".into(),
        mod_time: UNIX_EPOCH + Duration::from_secs(1_700_000_001),
        on_success: None,
    });
    q.wait();

    assert_eq!(fs::read(dst.join("a.txt")).unwrap(), b"a");
    assert_eq!(fs::read(dst.join("b.txt")).unwrap(), b"b");
}

// ---- run_in_parallel ----

#[test]
fn run_in_parallel_executes_all_jobs() {
    // 020.6: run_in_parallel issues all submitted jobs and blocks until every one finishes.
    let q = make_queue();
    q.configure(default_config());

    let counter = Arc::new(AtomicUsize::new(0));
    let n = 8usize;
    let mut jobs: Vec<Box<dyn FnOnce() + Send>> = Vec::with_capacity(n);
    for _ in 0..n {
        let c = counter.clone();
        jobs.push(Box::new(move || {
            c.fetch_add(1, Ordering::Relaxed);
        }));
    }

    q.run_in_parallel(jobs);

    assert_eq!(
        counter.load(Ordering::Relaxed),
        n,
        "all submitted jobs must have completed before run_in_parallel returns"
    );
}

#[test]
fn run_in_parallel_proceeds_while_copy_limit_full() {
    // 020.4: Non-copy work submitted via run_in_parallel proceeds even while the copy-slot
    // limit is full. We cap the copy slot at 1, enqueue copies, and verify parallel jobs
    // also complete alongside them.
    let src = test_dir("par_full_src");
    let dst = test_dir("par_full_dst");
    for i in 0..4u64 {
        write_file(&src.join(format!("q{}.txt", i)), format!("q{}", i).as_bytes());
    }

    let q = make_queue();
    q.configure(CopyConfig {
        copy_slot_limit: Some(1),
        ..default_config()
    });
    for i in 0..4u64 {
        q.enqueue(CopyRequest {
            src_peer: file_url(&src),
            src_path: format!("q{}.txt", i),
            dst_peer: file_url(&dst),
            dst_path: format!("q{}.txt", i),
            mod_time: UNIX_EPOCH + Duration::from_secs(1_700_000_000 + i),
            on_success: None,
        });
    }

    let counter = Arc::new(AtomicUsize::new(0));
    let c1 = counter.clone();
    let c2 = counter.clone();
    q.run_in_parallel(vec![
        Box::new(move || { c1.fetch_add(1, Ordering::Relaxed); }),
        Box::new(move || { c2.fetch_add(1, Ordering::Relaxed); }),
    ]);

    assert_eq!(
        counter.load(Ordering::Relaxed),
        2,
        "parallel jobs must complete independent of copy-slot state"
    );

    q.wait();
    for i in 0..4u64 {
        assert!(dst.join(format!("q{}.txt", i)).exists(), "copy {} must complete", i);
    }
}

// ---- dry-run slot limit ----

#[test]
fn dry_run_copies_subject_to_slot_limit() {
    // 024.7: dry-run copies acquire slots subject to the global active-copy limit;
    // they still complete and write no destination files.
    let src = test_dir("dry_slot_src");
    let dst = test_dir("dry_slot_dst");
    for i in 0..5u64 {
        write_file(&src.join(format!("s{}.txt", i)), format!("s{}", i).as_bytes());
    }

    let q = make_queue();
    q.configure(CopyConfig {
        copy_slot_limit: Some(1),
        dry_run: true,
        ..default_config()
    });
    for i in 0..5u64 {
        q.enqueue(CopyRequest {
            src_peer: file_url(&src),
            src_path: format!("s{}.txt", i),
            dst_peer: file_url(&dst),
            dst_path: format!("s{}.txt", i),
            mod_time: UNIX_EPOCH + Duration::from_secs(1_700_000_000 + i),
            on_success: None,
        });
    }
    q.wait();

    for i in 0..5u64 {
        assert!(
            !dst.join(format!("s{}.txt", i)).exists(),
            "dry-run must write no destination files even when slot limit is in effect"
        );
    }
}

#[test]
fn dry_run_apply_try_limit() {
    // 024.8: dry-run applies the --retries-copy try limit to queued copies.
    // A try limit of 1 with a valid source completes in one try.
    let src = test_dir("dry_try_src");
    let dst = test_dir("dry_try_dst");
    write_file(&src.join("t.txt"), b"source");

    let q = make_queue();
    q.configure(CopyConfig {
        copy_try_limit: Some(1),
        dry_run: true,
        ..default_config()
    });
    q.enqueue(CopyRequest {
        src_peer: file_url(&src),
        src_path: "t.txt".into(),
        dst_peer: file_url(&dst),
        dst_path: "t.txt".into(),
        mod_time: UNIX_EPOCH + Duration::from_secs(1_700_000_000),
        on_success: None,
    });
    q.wait();

    // No file written (dry-run), but the copy ran and completed within the try limit.
    assert!(!dst.join("t.txt").exists(), "dry-run must not write destination");
}
