# /// script
# requires-python = ">=3.9"
# dependencies = []
# ///
"""
End-to-end test for reqs/021_staging-and-displacement.md

Covers: BAK/TMP staging, displacement mechanics, co-location, age-based
cleanup during traversal, default retention values, and --dry-run suppression.

Requirements exercised:
  021.1  BAK/<timestamp>/ directory created before rename
  021.2  Displaced entry renamed to BAK/<timestamp>/<basename>
  021.3  Directory displaced as single rename, subtree preserved
  021.4  BAK/ at parent of displaced entry, not sync root
  021.5  # not reasonably testable: triggering a rename failure requires
         # sabotaging the filesystem (write-protected parent), which conflicts
         # with the testing principle of not breaking the environment
  021.6  # not reasonably testable: same reason as 021.5
  021.7  # not directly observable: TMP dirs may be created and fully cleaned
         # within a single run; cleanup tests (021.12, 021.15) prove the
         # .kitchensync/TMP/ path is the cleanup target
  021.8  # not reasonably testable: concurrent TMP UUID distinctness requires
         # live observation of overlapping transfers
  021.9  .kitchensync/ inspected at each level (proven by cleanup behavior)
  021.10 BAK/TMP purged even though .kitchensync/ excluded from sync listings
  021.11 Old BAK/<timestamp>/ entries removed
  021.12 Old TMP/<timestamp>/ entries removed
  021.13 Age derived from timestamp component of directory name
  021.14 Fresh BAK entries left in place
  021.15 Fresh TMP entries left in place
  021.16 SWAP/ not purged by age-based cleanup
  021.17 Default --keep-bak-days is 90
  021.18 Default --keep-tmp-days is 2
  021.19 --dry-run skips BAK/TMP cleanup
"""

import sys
import subprocess
import tempfile
import pathlib
import datetime

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

EXE = pathlib.Path("/home/ace/Desktop/prjx/kitchensync/released/kitchensync.exe")

_failures: list[str] = []


def _check(condition: bool, msg: str) -> None:
    if not condition:
        _failures.append(msg)
        print(f"  FAIL: {msg}")


def _ts(days_ago: int) -> str:
    """Return a KitchenSync timestamp string for 'days_ago' days ago (UTC).

    Format: YYYY-MM-DD_HH-mm-ss_ffffffZ  (from specs/sync.md, database.md ref)
    """
    dt = datetime.datetime.now(datetime.timezone.utc) - datetime.timedelta(days=days_ago)
    return dt.strftime("%Y-%m-%d_%H-%M-%S") + f"_{dt.microsecond:06d}Z"


def _sync(*args: object, timeout: int = 60) -> tuple[int, str, str]:
    """Run the released kitchensync executable and return (rc, stdout, stderr)."""
    cmd = [str(EXE)] + [str(a) for a in args]
    r = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=timeout)
    return r.returncode, r.stdout.decode("utf-8", errors="replace"), r.stderr.decode("utf-8", errors="replace")


def _bak_ts_dirs(peer: pathlib.Path) -> list[pathlib.Path]:
    bak = peer / ".kitchensync" / "BAK"
    if not bak.is_dir():
        return []
    return [d for d in bak.iterdir() if d.is_dir()]


# ---------------------------------------------------------------------------
# 021.1, 021.2 -- displacement creates BAK/<timestamp>/<basename>
# ---------------------------------------------------------------------------
def test_021_1_2_displacement_creates_bak_entry() -> None:
    print("test_021_1_2: displacement creates BAK/<timestamp>/<basename>")
    with tempfile.TemporaryDirectory() as tmp:
        r = pathlib.Path(tmp)
        pa = r / "pa"   # canon
        pb = r / "pb"   # non-canon, starts without a snapshot -> auto-subordinate
        pa.mkdir()
        (pa / "marker.txt").write_text("marker")
        pb.mkdir()
        # extra.txt is only on pb; canon lacks it -> canon lacks file -> displace on pb
        (pb / "extra.txt").write_text("extra file content")

        rc, out, err = _sync(f"+{pa}", str(pb))
        if rc != 0:
            _check(False, f"021.1/2: sync failed (exit {rc}); stdout={out!r}")
            return

        # 021.2: original path must be gone
        _check(
            not (pb / "extra.txt").exists(),
            "021.2: extra.txt must not remain at original path after displacement",
        )

        # 021.1: BAK/<timestamp>/ directory must exist
        ts_dirs = _bak_ts_dirs(pb)
        _check(len(ts_dirs) >= 1, "021.1: .kitchensync/BAK/<timestamp>/ must be created before displacement rename")

        # 021.2: displaced basename appears under the timestamp directory
        if ts_dirs:
            _check(
                any((d / "extra.txt").exists() for d in ts_dirs),
                "021.2: extra.txt must appear as .kitchensync/BAK/<timestamp>/extra.txt",
            )


# ---------------------------------------------------------------------------
# 021.3 -- directory displacement preserves entire subtree via single rename
# ---------------------------------------------------------------------------
def test_021_3_directory_subtree_preserved() -> None:
    print("test_021_3: displacing a directory preserves its subtree")
    with tempfile.TemporaryDirectory() as tmp:
        r = pathlib.Path(tmp)
        pa = r / "pa"
        pb = r / "pb"
        pa.mkdir()
        (pa / "marker.txt").write_text("marker")
        pb.mkdir()
        # extra_dir/ only on pb -> canon lacks it -> displace the whole directory
        (pb / "extra_dir").mkdir()
        (pb / "extra_dir" / "file_a.txt").write_text("file a content")
        (pb / "extra_dir" / "sub").mkdir()
        (pb / "extra_dir" / "sub" / "file_b.txt").write_text("file b content")

        rc, out, err = _sync(f"+{pa}", str(pb))
        if rc != 0:
            _check(False, f"021.3: sync failed (exit {rc}); stdout={out!r}")
            return

        _check(
            not (pb / "extra_dir").exists(),
            "021.3: extra_dir must not remain at original path after displacement",
        )

        ts_dirs = _bak_ts_dirs(pb)
        _check(len(ts_dirs) >= 1, "021.3: BAK/<timestamp>/ must exist after directory displacement")
        if ts_dirs:
            displaced = ts_dirs[0] / "extra_dir"
            _check(displaced.is_dir(), "021.3: displaced entry must be a directory under BAK/<timestamp>/")
            _check(
                (displaced / "file_a.txt").exists(),
                "021.3: file_a.txt must be preserved inside displaced directory (single-rename subtree)",
            )
            _check(
                (displaced / "sub" / "file_b.txt").exists(),
                "021.3: sub/file_b.txt must be preserved inside displaced directory (subtree fully intact)",
            )


# ---------------------------------------------------------------------------
# 021.4 -- BAK/ is co-located at the parent directory of the displaced entry
# ---------------------------------------------------------------------------
def test_021_4_bak_colocated_at_parent() -> None:
    print("test_021_4: BAK/ co-located at parent of displaced entry, not sync root")
    with tempfile.TemporaryDirectory() as tmp:
        r = pathlib.Path(tmp)
        pa = r / "pa"
        pb = r / "pb"
        pa.mkdir()
        (pa / "subdir").mkdir()
        (pa / "subdir" / "marker.txt").write_text("marker")
        pb.mkdir()
        (pb / "subdir").mkdir()
        # extra.txt is in subdir on pb only -> displaced to subdir/.kitchensync/BAK/
        (pb / "subdir" / "extra.txt").write_text("extra in subdir")

        rc, out, err = _sync(f"+{pa}", str(pb))
        if rc != 0:
            _check(False, f"021.4: sync failed (exit {rc}); stdout={out!r}")
            return

        # BAK must appear under subdir/.kitchensync/, not root .kitchensync/
        sub_bak = pb / "subdir" / ".kitchensync" / "BAK"
        _check(sub_bak.is_dir(), "021.4: BAK/ must be at subdir/.kitchensync/ (co-located at entry's parent)")

        sub_ts_dirs = [d for d in sub_bak.iterdir() if d.is_dir()] if sub_bak.is_dir() else []
        _check(
            any((d / "extra.txt").exists() for d in sub_ts_dirs),
            "021.4: extra.txt must appear under subdir/.kitchensync/BAK/<timestamp>/",
        )

        # extra.txt must NOT appear under root-level BAK/
        root_bak = pb / ".kitchensync" / "BAK"
        if root_bak.is_dir():
            root_ts_dirs = [d for d in root_bak.iterdir() if d.is_dir()]
            _check(
                not any((d / "extra.txt").exists() for d in root_ts_dirs),
                "021.4: extra.txt must NOT appear under root-level BAK/ (displacement must be at parent level)",
            )


# ---------------------------------------------------------------------------
# 021.9, 021.10, 021.11, 021.13 -- cleanup removes old BAK entries
# .kitchensync/ is excluded from sync listings but its BAK/ is still inspected
# ---------------------------------------------------------------------------
def test_021_9_10_11_13_cleanup_removes_old_bak() -> None:
    print("test_021_9_10_11_13: cleanup removes old BAK entry during traversal")
    with tempfile.TemporaryDirectory() as tmp:
        r = pathlib.Path(tmp)
        pa = r / "pa"
        pb = r / "pb"
        pa.mkdir()
        (pa / "marker.txt").write_text("marker")
        pb.mkdir()

        # 021.13: age is read from the timestamp component of the directory name.
        # 100-day-old timestamp with --keep-bak-days 30 -> must be purged.
        old_ts = _ts(100)
        old_bak_dir = pb / ".kitchensync" / "BAK" / old_ts
        old_bak_dir.mkdir(parents=True)
        (old_bak_dir / "old_displaced.txt").write_text("old")

        rc, out, err = _sync(f"+{pa}", str(pb), "--keep-bak-days", "30")
        if rc != 0:
            _check(False, f"021.11: sync failed (exit {rc}); stdout={out!r}")
            return

        # 021.9/021.10/021.11/021.13
        _check(
            not old_bak_dir.exists(),
            f"021.11: BAK entry {old_ts!r} (100 days old) must be purged with --keep-bak-days 30"
            " (021.10: .kitchensync/ is excluded from sync but its BAK/ must still be cleaned)",
        )


# ---------------------------------------------------------------------------
# 021.12, 021.13 -- cleanup removes old TMP entries
# ---------------------------------------------------------------------------
def test_021_12_13_cleanup_removes_old_tmp() -> None:
    print("test_021_12_13: cleanup removes old TMP entry during traversal")
    with tempfile.TemporaryDirectory() as tmp:
        r = pathlib.Path(tmp)
        pa = r / "pa"
        pb = r / "pb"
        pa.mkdir()
        (pa / "marker.txt").write_text("marker")
        pb.mkdir()

        old_ts = _ts(10)   # 10 days old; --keep-tmp-days 2 -> must be purged
        old_tmp_dir = pb / ".kitchensync" / "TMP" / old_ts
        old_tmp_dir.mkdir(parents=True)
        (old_tmp_dir / "stale_data.bin").write_bytes(b"\x00" * 16)

        rc, out, err = _sync(f"+{pa}", str(pb), "--keep-tmp-days", "2")
        if rc != 0:
            _check(False, f"021.12: sync failed (exit {rc}); stdout={out!r}")
            return

        _check(
            not old_tmp_dir.exists(),
            f"021.12: TMP entry {old_ts!r} (10 days old) must be purged with --keep-tmp-days 2",
        )


# ---------------------------------------------------------------------------
# 021.14 -- fresh BAK entries are left in place
# ---------------------------------------------------------------------------
def test_021_14_fresh_bak_preserved() -> None:
    print("test_021_14: fresh BAK entries are kept")
    with tempfile.TemporaryDirectory() as tmp:
        r = pathlib.Path(tmp)
        pa = r / "pa"
        pb = r / "pb"
        pa.mkdir()
        (pa / "marker.txt").write_text("marker")
        pb.mkdir()

        fresh_ts = _ts(1)   # 1 day old, well within 30-day limit
        fresh_bak_dir = pb / ".kitchensync" / "BAK" / fresh_ts
        fresh_bak_dir.mkdir(parents=True)
        (fresh_bak_dir / "recent_item.txt").write_text("recent")

        rc, out, err = _sync(f"+{pa}", str(pb), "--keep-bak-days", "30")
        if rc != 0:
            _check(False, f"021.14: sync failed (exit {rc}); stdout={out!r}")
            return

        _check(
            fresh_bak_dir.exists(),
            f"021.14: BAK entry {fresh_ts!r} (1 day old) must NOT be purged with --keep-bak-days 30",
        )


# ---------------------------------------------------------------------------
# 021.15 -- fresh TMP entries are left in place
# ---------------------------------------------------------------------------
def test_021_15_fresh_tmp_preserved() -> None:
    print("test_021_15: fresh TMP entries are kept")
    with tempfile.TemporaryDirectory() as tmp:
        r = pathlib.Path(tmp)
        pa = r / "pa"
        pb = r / "pb"
        pa.mkdir()
        (pa / "marker.txt").write_text("marker")
        pb.mkdir()

        fresh_ts = _ts(0)   # current time, well within 2-day limit
        fresh_tmp_dir = pb / ".kitchensync" / "TMP" / fresh_ts
        fresh_tmp_dir.mkdir(parents=True)
        (fresh_tmp_dir / "recent_data.bin").write_bytes(b"\x00" * 8)

        rc, out, err = _sync(f"+{pa}", str(pb), "--keep-tmp-days", "2")
        if rc != 0:
            _check(False, f"021.15: sync failed (exit {rc}); stdout={out!r}")
            return

        _check(
            fresh_tmp_dir.exists(),
            f"021.15: TMP entry {fresh_ts!r} (0 days old) must NOT be purged with --keep-tmp-days 2",
        )


# ---------------------------------------------------------------------------
# 021.16 -- SWAP/ is never purged by age-based cleanup
# ---------------------------------------------------------------------------
def test_021_16_swap_not_purged_by_age() -> None:
    print("test_021_16: SWAP/ not removed by age-based BAK/TMP cleanup")
    with tempfile.TemporaryDirectory() as tmp:
        r = pathlib.Path(tmp)
        pa = r / "pa"
        pb = r / "pb"
        pa.mkdir()
        (pa / "marker.txt").write_text("marker")
        pb.mkdir()

        # Old BAK entry to confirm that cleanup IS running
        old_ts = _ts(100)
        old_bak_dir = pb / ".kitchensync" / "BAK" / old_ts
        old_bak_dir.mkdir(parents=True)
        (old_bak_dir / "old.txt").write_text("old")

        # Empty SWAP/ container: no subdirectory entries, so SWAP recovery has
        # nothing to process.  Age-based cleanup must not remove the directory.
        swap_container = pb / ".kitchensync" / "SWAP"
        swap_container.mkdir(parents=True)

        rc, out, err = _sync(f"+{pa}", str(pb), "--keep-bak-days", "1")
        if rc != 0:
            _check(False, f"021.16: sync failed (exit {rc}); stdout={out!r}")
            return

        # Confirm cleanup ran (otherwise 021.16 is moot)
        _check(
            not old_bak_dir.exists(),
            "021.16 precondition: old BAK entry must be purged (confirms age-based cleanup ran)",
        )

        # SWAP/ must survive the cleanup pass that removed BAK
        _check(
            swap_container.exists(),
            "021.16: .kitchensync/SWAP/ must not be removed by age-based BAK/TMP cleanup",
        )


# ---------------------------------------------------------------------------
# 021.17 -- default --keep-bak-days is 90
# ---------------------------------------------------------------------------
def test_021_17_default_keep_bak_days_90() -> None:
    print("test_021_17: default --keep-bak-days is 90 days")
    with tempfile.TemporaryDirectory() as tmp:
        r = pathlib.Path(tmp)
        pa = r / "pa"
        pb = r / "pb"
        pa.mkdir()
        (pa / "marker.txt").write_text("marker")
        pb.mkdir()

        ts_91 = _ts(91)  # 91 days old -> older than default 90 -> must be purged
        old_bak = pb / ".kitchensync" / "BAK" / ts_91
        old_bak.mkdir(parents=True)
        (old_bak / "too_old.txt").write_text("too old")

        ts_89 = _ts(89)  # 89 days old -> within default 90 -> must be kept
        fresh_bak = pb / ".kitchensync" / "BAK" / ts_89
        fresh_bak.mkdir(parents=True)
        (fresh_bak / "still_fresh.txt").write_text("still fresh")

        # Run without --keep-bak-days to exercise the default (90 days)
        rc, out, err = _sync(f"+{pa}", str(pb))
        if rc != 0:
            _check(False, f"021.17: sync failed (exit {rc}); stdout={out!r}")
            return

        _check(
            not old_bak.exists(),
            f"021.17: BAK entry {ts_91!r} (91 days old) must be purged with default 90-day retention",
        )
        _check(
            fresh_bak.exists(),
            f"021.17: BAK entry {ts_89!r} (89 days old) must be kept with default 90-day retention",
        )


# ---------------------------------------------------------------------------
# 021.18 -- default --keep-tmp-days is 2
# ---------------------------------------------------------------------------
def test_021_18_default_keep_tmp_days_2() -> None:
    print("test_021_18: default --keep-tmp-days is 2 days")
    with tempfile.TemporaryDirectory() as tmp:
        r = pathlib.Path(tmp)
        pa = r / "pa"
        pb = r / "pb"
        pa.mkdir()
        (pa / "marker.txt").write_text("marker")
        pb.mkdir()

        ts_3 = _ts(3)   # 3 days old -> older than default 2 -> must be purged
        old_tmp = pb / ".kitchensync" / "TMP" / ts_3
        old_tmp.mkdir(parents=True)
        (old_tmp / "stale.bin").write_bytes(b"\x00" * 8)

        ts_1 = _ts(1)   # 1 day old -> within default 2 -> must be kept
        fresh_tmp = pb / ".kitchensync" / "TMP" / ts_1
        fresh_tmp.mkdir(parents=True)
        (fresh_tmp / "recent.bin").write_bytes(b"\x00" * 8)

        # Run without --keep-tmp-days to exercise the default (2 days)
        rc, out, err = _sync(f"+{pa}", str(pb))
        if rc != 0:
            _check(False, f"021.18: sync failed (exit {rc}); stdout={out!r}")
            return

        _check(
            not old_tmp.exists(),
            f"021.18: TMP entry {ts_3!r} (3 days old) must be purged with default 2-day retention",
        )
        _check(
            fresh_tmp.exists(),
            f"021.18: TMP entry {ts_1!r} (1 day old) must be kept with default 2-day retention",
        )


# ---------------------------------------------------------------------------
# 021.19 -- --dry-run skips BAK/TMP cleanup on peers
# ---------------------------------------------------------------------------
def test_021_19_dry_run_skips_cleanup() -> None:
    print("test_021_19: --dry-run skips BAK/TMP cleanup")
    with tempfile.TemporaryDirectory() as tmp:
        r = pathlib.Path(tmp)
        pa = r / "pa"
        pb = r / "pb"
        pa.mkdir()
        (pa / "marker.txt").write_text("marker")
        pb.mkdir()
        (pb / "marker.txt").write_text("marker")  # both roots exist for dry-run

        # Old BAK and TMP entries that would be purged in a normal run
        old_ts = _ts(100)
        old_bak = pb / ".kitchensync" / "BAK" / old_ts
        old_bak.mkdir(parents=True)
        (old_bak / "old.txt").write_text("old")

        old_tmp = pb / ".kitchensync" / "TMP" / old_ts
        old_tmp.mkdir(parents=True)
        (old_tmp / "old_tmp.bin").write_bytes(b"\x00" * 8)

        rc, out, err = _sync(
            f"+{pa}", str(pb),
            "--dry-run",
            "--keep-bak-days", "1",
            "--keep-tmp-days", "1",
        )
        if rc != 0:
            _check(False, f"021.19: dry-run sync failed (exit {rc}); stdout={out!r}")
            return

        _check(
            old_bak.exists(),
            f"021.19: BAK entry {old_ts!r} must NOT be purged in --dry-run mode",
        )
        _check(
            old_tmp.exists(),
            f"021.19: TMP entry {old_ts!r} must NOT be purged in --dry-run mode",
        )


def main() -> None:
    print("=== test_021_staging_and_displacement ===")
    if not EXE.exists():
        print(f"FATAL: executable not found: {EXE}")
        sys.exit(1)

    test_021_1_2_displacement_creates_bak_entry()
    test_021_3_directory_subtree_preserved()
    test_021_4_bak_colocated_at_parent()
    test_021_9_10_11_13_cleanup_removes_old_bak()
    test_021_12_13_cleanup_removes_old_tmp()
    test_021_14_fresh_bak_preserved()
    test_021_15_fresh_tmp_preserved()
    test_021_16_swap_not_purged_by_age()
    test_021_17_default_keep_bak_days_90()
    test_021_18_default_keep_tmp_days_2()
    test_021_19_dry_run_skips_cleanup()

    print()
    if _failures:
        for f in _failures:
            print(f"FAIL: {f}")
        sys.exit(1)
    print("All checks passed.")
    sys.exit(0)


if __name__ == "__main__":
    main()
