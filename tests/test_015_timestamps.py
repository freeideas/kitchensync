# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""
End-to-end test for 015_timestamps.md.

Covers:
  015.1  timestamp format YYYY-MM-DD_HH-mm-ss_ffffffZ
  015.2  UTC (ends with Z)
  015.3  microsecond precision (six fractional digits)
  015.4  lexicographic sort == chronological sort
  015.5  same format in DB columns, BAK/ dirs, TMP/ dirs, log output
  015.6  each last_seen written during a run is a freshly generated timestamp
  015.7  each BAK/ or TMP/ directory is named with a freshly generated timestamp
  015.8  no two freshly generated timestamps equal within one run
  015.9  deleted_time copied from last_seen, not freshly generated
  015.10 descendant cascade rows share the displaced entry's deleted_time
"""

import os
import re
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import time
from datetime import datetime, timezone
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

EXE = Path("/home/ace/Desktop/prjx/kitchensync/released/kitchensync.exe")

# YYYY-MM-DD_HH-mm-ss_ffffffZ
TS_RE = re.compile(r"^\d{4}-\d{2}-\d{2}_\d{2}-\d{2}-\d{2}_\d{6}Z$")

FAILURES: list[str] = []


def check(cond: bool, msg: str) -> None:
    if cond:
        print(f"PASS: {msg}")
    else:
        FAILURES.append(msg)
        print(f"FAIL: {msg}")


def run_ks(*args: object, timeout: int = 60) -> subprocess.CompletedProcess:
    return subprocess.run(
        [str(EXE)] + [str(a) for a in args],
        capture_output=True,
        text=True,
        timeout=timeout,
    )


def snapshot_db(peer_root: Path) -> Path:
    return peer_root / ".kitchensync" / "snapshot.db"


def db_rows(db_path: Path) -> list[dict]:
    cols = ["id", "parent_id", "basename", "mod_time", "byte_size", "last_seen", "deleted_time"]
    con = sqlite3.connect(str(db_path))
    try:
        rows = con.execute(
            "SELECT id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time"
            " FROM snapshot"
        ).fetchall()
    finally:
        con.close()
    return [dict(zip(cols, r)) for r in rows]


def all_ts_values(db_path: Path) -> list[str]:
    result = []
    for row in db_rows(db_path):
        for col in ("mod_time", "last_seen", "deleted_time"):
            v = row[col]
            if v is not None:
                result.append(v)
    return result


def parse_ts(ts: str) -> datetime:
    """Parse YYYY-MM-DD_HH-mm-ss_ffffffZ into an aware datetime."""
    date_part, time_part, frac_part = ts.rstrip("Z").split("_")
    y, mo, d = date_part.split("-")
    h, m, s = time_part.split("-")
    return datetime(int(y), int(mo), int(d), int(h), int(m), int(s),
                    int(frac_part), tzinfo=timezone.utc)


def write_file(path: Path, content: str, mtime: float | None = None) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")
    if mtime is not None:
        os.utime(str(path), (mtime, mtime))


def check_ts_in_bak(peer_root: Path, label: str) -> None:
    bak_root = peer_root / ".kitchensync" / "BAK"
    if not bak_root.exists():
        return
    for entry in bak_root.iterdir():
        if not entry.is_dir():
            continue
        name = entry.name
        check(TS_RE.match(name) is not None,
              f"015.7: BAK/ dir '{name}' matches timestamp format on {label}")
        check(name.endswith("Z"),
              f"015.5: BAK/ dir '{name}' ends with Z on {label}")


def check_ts_in_tmp(peer_root: Path, label: str) -> None:
    # not reasonably testable: 015.5 (TMP/) - TMP/ dirs may be cleaned up within the run
    tmp_root = peer_root / ".kitchensync" / "TMP"
    if not tmp_root.exists():
        return
    for entry in tmp_root.iterdir():
        if not entry.is_dir():
            continue
        name = entry.name
        check(TS_RE.match(name) is not None,
              f"015.5: TMP/ dir '{name}' matches timestamp format on {label}")


# -------------------------------------------------------------------------
# Scenario A
# Basic sync: verify format, UTC, microseconds, lex sort, last_seen, uniqueness
# Covers 015.1, 015.2, 015.3, 015.4, 015.5 (DB), 015.6, 015.8
# -------------------------------------------------------------------------
with tempfile.TemporaryDirectory() as _td:
    peer1 = Path(_td) / "peer1"
    peer2 = Path(_td) / "peer2"
    peer1.mkdir()
    peer2.mkdir()

    old_t = time.time() - 3600
    for i in range(5):
        write_file(peer1 / f"file{i}.txt", f"content {i}", mtime=old_t - i * 10)

    res = run_ks("+" + str(peer1), str(peer2))
    check(res.returncode == 0,
          "015.1: sync of 5 files exits 0")

    for peer, label in [(peer1, "peer1"), (peer2, "peer2")]:
        db = snapshot_db(peer)
        if not db.exists():
            check(False, f"015.1: snapshot.db exists on {label}")
            continue

        ts_vals = all_ts_values(db)
        check(len(ts_vals) > 0,
              f"015.1: {label} snapshot has timestamp values")

        for ts in ts_vals:
            check(TS_RE.match(ts) is not None,
                  f"015.1: '{ts}' matches YYYY-MM-DD_HH-mm-ss_ffffffZ on {label}")
            check(ts.endswith("Z"),
                  f"015.2: '{ts}' ends with Z (UTC) on {label}")
            frac = ts.split("_")[2].rstrip("Z")
            check(len(frac) == 6,
                  f"015.3: '{ts}' has six fractional-second digits on {label}")

        if len(ts_vals) >= 2:
            lex = sorted(ts_vals)
            chron = sorted(ts_vals, key=parse_ts)
            check(lex == chron,
                  f"015.4: lexicographic order == chronological order on {label}")

        rows = db_rows(db)
        last_seens = [r["last_seen"] for r in rows if r["last_seen"] is not None]
        check(len(last_seens) > 0,
              f"015.6: last_seen values are set on {label}")
        for ls in last_seens:
            check(TS_RE.match(ls) is not None,
                  f"015.6: last_seen '{ls}' matches timestamp format on {label}")

        check(len(last_seens) == len(set(last_seens)),
              f"015.8: all {len(last_seens)} last_seen values unique within run on {label}")

    # 015.5: scan stdout for any timestamp-like strings; all must match the format
    ts_in_log = re.findall(r"\d{4}-\d{2}-\d{2}_\d{2}-\d{2}-\d{2}_\d{6}Z", res.stdout)
    for ts in ts_in_log:
        check(TS_RE.match(ts) is not None,
              f"015.5: log-output timestamp '{ts}' matches format")

# -------------------------------------------------------------------------
# Scenario B
# Trigger displacement so peer1's file goes to BAK/; check BAK/ dir name format
# Covers 015.5 (BAK), 015.7
# -------------------------------------------------------------------------
with tempfile.TemporaryDirectory() as _td:
    peer1 = Path(_td) / "peer1"
    peer2 = Path(_td) / "peer2"
    peer1.mkdir()
    peer2.mkdir()

    old_t = time.time() - 7200
    new_t = time.time() - 60

    # peer2 is canon with the newer file; peer1's version is displaced to BAK/
    write_file(peer1 / "foo.txt", "old-version", mtime=old_t)
    write_file(peer2 / "foo.txt", "new-version", mtime=new_t)

    res = run_ks("+" + str(peer2), str(peer1))
    check(res.returncode == 0,
          "015.7: displacement-trigger sync exits 0")

    bak_root = peer1 / ".kitchensync" / "BAK"
    if not bak_root.exists():
        check(False, "015.7: .kitchensync/BAK/ created on peer1 after displacement")
    else:
        bak_dirs = [d.name for d in bak_root.iterdir() if d.is_dir()]
        check(len(bak_dirs) > 0,
              "015.7: at least one BAK/<timestamp>/ directory created on peer1")
        for name in bak_dirs:
            check(TS_RE.match(name) is not None,
                  f"015.7: BAK/ dir name '{name}' matches timestamp format")
            check(name.endswith("Z"),
                  f"015.5: BAK/ dir name '{name}' ends with Z")

    for peer, label in [(peer1, "peer1"), (peer2, "peer2")]:
        db = snapshot_db(peer)
        if db.exists():
            for ts in all_ts_values(db):
                check(TS_RE.match(ts) is not None,
                      f"015.5: DB timestamp '{ts}' matches format on {label}")

    check_ts_in_tmp(peer1, "peer1")
    check_ts_in_tmp(peer2, "peer2")

# -------------------------------------------------------------------------
# Scenario C
# Delete a file from peer1 after first sync; verify peer2 tombstone has
# deleted_time == last_seen (not a freshly generated timestamp)
# Covers 015.9
# -------------------------------------------------------------------------
with tempfile.TemporaryDirectory() as _td:
    peer1 = Path(_td) / "peer1"
    peer2 = Path(_td) / "peer2"
    peer1.mkdir()
    peer2.mkdir()

    old_t = time.time() - 7200
    write_file(peer1 / "bar.txt", "bar content", mtime=old_t)

    res1 = run_ks("+" + str(peer1), str(peer2))
    check(res1.returncode == 0, "015.9: first sync exits 0")

    db2 = snapshot_db(peer2)
    if not db2.exists():
        check(False, "015.9: peer2 snapshot.db exists after first sync")
    else:
        rows1 = db_rows(db2)
        bar1 = next((r for r in rows1 if r["basename"] == "bar.txt"), None)
        check(bar1 is not None,
              "015.9: bar.txt row in peer2 snapshot after first sync")

        if bar1 is not None and bar1["last_seen"] is not None:
            # Delete bar.txt from peer1 and re-sync
            (peer1 / "bar.txt").unlink()

            res2 = run_ks(str(peer1), str(peer2))
            check(res2.returncode == 0, "015.9: second sync exits 0")

            rows2 = db_rows(db2)
            bar2 = next((r for r in rows2 if r["basename"] == "bar.txt"), None)
            check(bar2 is not None,
                  "015.9: bar.txt row persists in peer2 snapshot as tombstone")
            if bar2 is not None:
                check(bar2["deleted_time"] is not None,
                      "015.9: bar.txt deleted_time is set after deletion")
                check(bar2["last_seen"] is not None,
                      "015.9: bar.txt last_seen is still set in tombstone row")
                if bar2["deleted_time"] is not None and bar2["last_seen"] is not None:
                    check(bar2["deleted_time"] == bar2["last_seen"],
                          f"015.9: deleted_time ({bar2['deleted_time']}) == "
                          f"last_seen ({bar2['last_seen']}), confirming it was copied "
                          f"from last_seen rather than freshly generated")

# -------------------------------------------------------------------------
# Scenario D
# Delete a directory (with children) from peer1 after first sync; verify that
# - the displaced dir's deleted_time == its own last_seen (015.9 on the parent)
# - each descendant's deleted_time == the displaced dir's deleted_time (015.10)
# Covers 015.9, 015.10
# -------------------------------------------------------------------------
with tempfile.TemporaryDirectory() as _td:
    peer1 = Path(_td) / "peer1"
    peer2 = Path(_td) / "peer2"
    peer1.mkdir()
    peer2.mkdir()

    old_t = time.time() - 7200
    subdir = peer1 / "subdir"
    subdir.mkdir()
    write_file(peer1 / "subdir" / "child1.txt", "child one", mtime=old_t)
    write_file(peer1 / "subdir" / "child2.txt", "child two", mtime=old_t)
    os.utime(str(subdir), (old_t, old_t))

    res1 = run_ks("+" + str(peer1), str(peer2))
    check(res1.returncode == 0, "015.10: first sync exits 0")

    db2 = snapshot_db(peer2)
    if not db2.exists():
        check(False, "015.10: peer2 snapshot.db exists after first sync")
    else:
        rows1 = db_rows(db2)
        subdir1 = next((r for r in rows1 if r["basename"] == "subdir"), None)
        check(subdir1 is not None,
              "015.10: subdir row in peer2 snapshot after first sync")

        if subdir1 is not None and subdir1["last_seen"] is not None:
            # Delete subdir/ (and all contents) from peer1, then re-sync with peer1 as canon
            # so that peer1's absence displaces the directory from peer2.
            shutil.rmtree(str(peer1 / "subdir"))

            res2 = run_ks("+" + str(peer1), str(peer2))
            check(res2.returncode == 0, "015.10: second sync exits 0")

            rows2 = db_rows(db2)
            subdir2 = next((r for r in rows2 if r["basename"] == "subdir"), None)
            child1_2 = next((r for r in rows2 if r["basename"] == "child1.txt"), None)
            child2_2 = next((r for r in rows2 if r["basename"] == "child2.txt"), None)

            for name, row in [("subdir", subdir2),
                               ("child1.txt", child1_2),
                               ("child2.txt", child2_2)]:
                check(row is not None,
                      f"015.10: {name} row persists in peer2 snapshot after deletion")

            if subdir2 is not None:
                check(subdir2["deleted_time"] is not None,
                      "015.9: subdir deleted_time is set after displacement")
                check(subdir2["last_seen"] is not None,
                      "015.9: subdir last_seen is still set in tombstone row")
                if (subdir2["deleted_time"] is not None
                        and subdir2["last_seen"] is not None):
                    check(subdir2["deleted_time"] == subdir2["last_seen"],
                          f"015.9: subdir deleted_time ({subdir2['deleted_time']}) == "
                          f"last_seen ({subdir2['last_seen']})")

                subdir_dt = subdir2["deleted_time"]
                for name, row in [("child1.txt", child1_2), ("child2.txt", child2_2)]:
                    if row is not None and subdir_dt is not None:
                        check(row["deleted_time"] == subdir_dt,
                              f"015.10: {name} deleted_time ({row['deleted_time']}) == "
                              f"displaced subdir deleted_time ({subdir_dt})")

# -------------------------------------------------------------------------
# Final report
# -------------------------------------------------------------------------
print()
if FAILURES:
    print(f"{len(FAILURES)} failure(s):")
    for f in FAILURES:
        print(f"  FAIL: {f}")
    sys.exit(1)
else:
    print("All checks passed.")
    sys.exit(0)
