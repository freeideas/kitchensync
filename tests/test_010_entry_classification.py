# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

"""End-to-end tests for per-peer entry classification (reqs/010_entry-classification.md).

Classification is an intermediate per-peer judgment whose only external proof runs
through the decision outcomes (copy / displacement).  Each test sets up snapshot
state to exercise one classification path, then runs the product and observes
C/X progress lines or the absence of them.

All second-pass syncs use --dry-run so peer files and snapshots are not
modified, giving every test a clean read of the snapshot we wrote in setup.
"""

import os
import platform
import sqlite3
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

WORKSPACE = Path("/home/ace/Desktop/prjx/kitchensync")
EXE = Path("/home/ace/Desktop/prjx/kitchensync/released/kitchensync.exe")

failures: list[str] = []


def check(condition: bool, msg: str) -> None:
    if not condition:
        failures.append(msg)


def run_sync(args: list, timeout: int = 30) -> tuple[str, str, int]:
    result = subprocess.run(
        [str(EXE)] + [str(a) for a in args],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=timeout,
    )
    return result.stdout, result.stderr, result.returncode


def snapshot_db(peer_dir: Path) -> Path:
    return peer_dir / ".kitchensync" / "snapshot.db"


def set_mtime(path: Path, t: float) -> None:
    os.utime(str(path), (t, t))


# ---------------------------------------------------------------------------
# 010.1 -- unchanged: matching mod_time (within 5 s) and byte_size -> no copy
# ---------------------------------------------------------------------------

def test_010_1_unchanged_not_recopied() -> None:
    """Live file matching snapshot on both peers must not be re-copied."""
    with tempfile.TemporaryDirectory() as tmp:
        peer_a = Path(tmp) / "peer_a"
        peer_b = Path(tmp) / "peer_b"
        peer_a.mkdir()
        peer_b.mkdir()

        content = b"stable content"
        file_t = time.time() - 300

        for peer in (peer_a, peer_b):
            (peer / "file.txt").write_bytes(content)
            set_mtime(peer / "file.txt", file_t)

        # Establish snapshots on both peers.
        stdout, stderr, rc = run_sync(["+" + str(peer_a), str(peer_b)])
        check(rc == 0, f"010.1 setup run failed: rc={rc} stdout={stdout!r}")
        check(stderr == "", f"010.1 setup run: unexpected stderr: {stderr!r}")

        # Second pass (dry): both match snapshot -> no copy.
        stdout, stderr, rc = run_sync(["--dry-run", "+" + str(peer_a), str(peer_b)])
        check(rc == 0, f"010.1 dry run failed: rc={rc} stdout={stdout!r}")
        check(
            "C file.txt" not in stdout,
            f"010.1: unchanged file (mod_time and byte_size match snapshot) must not be re-copied; stdout={stdout!r}",
        )


def test_010_1_tolerance_within_5s() -> None:
    """Mod_time within 5 s of snapshot row is still classified unchanged."""
    with tempfile.TemporaryDirectory() as tmp:
        peer_a = Path(tmp) / "peer_a"
        peer_b = Path(tmp) / "peer_b"
        peer_a.mkdir()
        peer_b.mkdir()

        content = b"stable content"
        file_t = time.time() - 300

        for peer in (peer_a, peer_b):
            (peer / "file.txt").write_bytes(content)
            set_mtime(peer / "file.txt", file_t)

        stdout, _, rc = run_sync(["+" + str(peer_a), str(peer_b)])
        check(rc == 0, f"010.1 tolerance setup failed: rc={rc}")

        # Shift peer_a's file mod_time by 3 s -- within the 5 s tolerance.
        set_mtime(peer_a / "file.txt", file_t + 3)

        stdout, _, rc = run_sync(["--dry-run", str(peer_a), str(peer_b)])
        check(rc == 0, f"010.1 tolerance dry run failed: rc={rc}")
        check(
            "C file.txt" not in stdout,
            f"010.1: mod_time within 5 s tolerance must be classified unchanged; stdout={stdout!r}",
        )


# ---------------------------------------------------------------------------
# 010.2 -- byte_size differs -> modified even when mod_time matches row
# ---------------------------------------------------------------------------

def test_010_2_byte_size_differs_is_modified() -> None:
    """Differing byte_size classifies a file as modified regardless of mod_time."""
    with tempfile.TemporaryDirectory() as tmp:
        peer_a = Path(tmp) / "peer_a"
        peer_b = Path(tmp) / "peer_b"
        peer_a.mkdir()
        peer_b.mkdir()

        content = b"original"
        file_t = time.time() - 300

        for peer in (peer_a, peer_b):
            (peer / "file.txt").write_bytes(content)
            set_mtime(peer / "file.txt", file_t)

        stdout, _, rc = run_sync(["+" + str(peer_a), str(peer_b)])
        check(rc == 0, f"010.2 setup run failed: rc={rc}")

        # Rewrite peer_a's file with MORE bytes, then restore the original mod_time.
        # Snapshot still records byte_size = len(b"original") = 8.
        (peer_a / "file.txt").write_bytes(b"original extended content")
        set_mtime(peer_a / "file.txt", file_t)

        # peer_a: live byte_size != snapshot byte_size -> modified -> copy expected.
        stdout, _, rc = run_sync(["--dry-run", str(peer_a), str(peer_b)])
        check(rc == 0, f"010.2 dry run failed: rc={rc}")
        check(
            "C file.txt" in stdout,
            f"010.2: file with changed byte_size (matching mod_time) must be classified modified and re-copied; stdout={stdout!r}",
        )


# ---------------------------------------------------------------------------
# 010.3 -- mod_time differs by more than 5 s -> modified even when byte_size matches
# ---------------------------------------------------------------------------

def test_010_3_mod_time_differs_gt5s_is_modified() -> None:
    """Mod_time more than 5 s from snapshot row classifies a file as modified."""
    with tempfile.TemporaryDirectory() as tmp:
        peer_a = Path(tmp) / "peer_a"
        peer_b = Path(tmp) / "peer_b"
        peer_a.mkdir()
        peer_b.mkdir()

        content = b"unchanged bytes"
        file_t = time.time() - 300

        for peer in (peer_a, peer_b):
            (peer / "file.txt").write_bytes(content)
            set_mtime(peer / "file.txt", file_t)

        stdout, _, rc = run_sync(["+" + str(peer_a), str(peer_b)])
        check(rc == 0, f"010.3 setup run failed: rc={rc}")

        # Advance peer_a's mod_time by 10 s -- outside the 5 s tolerance.
        # Content and byte_size are unchanged.
        set_mtime(peer_a / "file.txt", file_t + 10)

        # peer_a: |live_mod_time - snapshot_mod_time| = 10 s > 5 s -> modified.
        stdout, _, rc = run_sync(["--dry-run", str(peer_a), str(peer_b)])
        check(rc == 0, f"010.3 dry run failed: rc={rc}")
        check(
            "C file.txt" in stdout,
            f"010.3: file with mod_time diff >5 s (matching byte_size) must be classified modified and re-copied; stdout={stdout!r}",
        )


# ---------------------------------------------------------------------------
# 010.4 -- live file over tombstoned row -> resurrection (propagated, not deleted)
# ---------------------------------------------------------------------------

def test_010_4_resurrection_live_over_tombstone() -> None:
    """A live file whose snapshot row has a non-NULL deleted_time is a resurrection.

    Resurrection is classified as modified: the file is propagated, not deleted.
    """
    with tempfile.TemporaryDirectory() as tmp:
        peer_a = Path(tmp) / "peer_a"
        peer_b = Path(tmp) / "peer_b"
        peer_a.mkdir()
        peer_b.mkdir()

        content = b"resurrected file"
        file_t = time.time() - 300

        for peer in (peer_a, peer_b):
            (peer / "file.txt").write_bytes(content)
            set_mtime(peer / "file.txt", file_t)

        stdout, _, rc = run_sync(["+" + str(peer_a), str(peer_b)])
        check(rc == 0, f"010.4 setup run failed: rc={rc}")

        # Tombstone file.txt in peer_a's snapshot while the live file remains.
        snap_a = snapshot_db(peer_a)
        with sqlite3.connect(str(snap_a)) as conn:
            row = conn.execute(
                "SELECT id FROM snapshot WHERE basename='file.txt'"
            ).fetchone()
            if row:
                conn.execute(
                    "UPDATE snapshot SET deleted_time='2020-01-01_00-00-00_000000Z' WHERE id=?",
                    (row[0],),
                )
                conn.commit()

        # Remove file.txt from peer_b and delete its snapshot row so peer_b
        # contributes no opinion, leaving peer_a's resurrection as the only vote.
        (peer_b / "file.txt").unlink()
        snap_b = snapshot_db(peer_b)
        with sqlite3.connect(str(snap_b)) as conn:
            conn.execute("DELETE FROM snapshot WHERE basename='file.txt'")
            conn.commit()

        # peer_a: live file + deleted_time non-NULL -> resurrection (modified) -> copy.
        # peer_b: absent, no snapshot row -> no opinion.
        stdout, _, rc = run_sync(["--dry-run", str(peer_a), str(peer_b)])
        check(rc == 0, f"010.4 dry run failed: rc={rc}")
        check(
            "C file.txt" in stdout,
            f"010.4: resurrected file (live over tombstone) must be propagated; stdout={stdout!r}",
        )
        check(
            "X file.txt" not in stdout,
            f"010.4: resurrected file must not be displaced; stdout={stdout!r}",
        )


# ---------------------------------------------------------------------------
# 010.5 -- no snapshot row -> new, propagated to peers that lack it
# ---------------------------------------------------------------------------

def test_010_5_new_file_propagated() -> None:
    """A live file with no snapshot row is classified as new and propagated."""
    with tempfile.TemporaryDirectory() as tmp:
        peer_a = Path(tmp) / "peer_a"
        peer_b = Path(tmp) / "peer_b"
        peer_a.mkdir()
        peer_b.mkdir()

        # Seed a base file on both peers so both have snapshot history.
        base_t = time.time() - 300
        for peer in (peer_a, peer_b):
            (peer / "base.txt").write_bytes(b"base")
            set_mtime(peer / "base.txt", base_t)

        stdout, _, rc = run_sync(["+" + str(peer_a), str(peer_b)])
        check(rc == 0, f"010.5 setup run failed: rc={rc}")

        # Add a new file on peer_a only -- no snapshot row exists on either peer.
        new_t = time.time() - 200
        (peer_a / "new_file.txt").write_bytes(b"brand new")
        set_mtime(peer_a / "new_file.txt", new_t)

        # peer_a: live new_file.txt, no snapshot row -> new -> propagated.
        # peer_b: absent, no snapshot row -> no opinion.
        stdout, _, rc = run_sync(["--dry-run", str(peer_a), str(peer_b)])
        check(rc == 0, f"010.5 dry run failed: rc={rc}")
        check(
            "C new_file.txt" in stdout,
            f"010.5: file with no snapshot row must be classified new and propagated; stdout={stdout!r}",
        )


# ---------------------------------------------------------------------------
# 010.6 -- absent + non-NULL deleted_time -> deleted
# ---------------------------------------------------------------------------

def test_010_6_deleted_entry_displaces_existing() -> None:
    """Absent file with non-NULL deleted_time is classified deleted.

    When the deletion estimate exceeds the existing file's mod_time, the
    deletion wins and the file is displaced from the peer that still has it.
    """
    with tempfile.TemporaryDirectory() as tmp:
        peer_a = Path(tmp) / "peer_a"
        peer_b = Path(tmp) / "peer_b"
        peer_a.mkdir()
        peer_b.mkdir()

        # Use an old file mod_time so the sync-time last_seen will exceed it,
        # making the deletion estimate (last_seen) win over the file's mod_time.
        content = b"to be deleted"
        old_t = time.time() - 1000

        for peer in (peer_a, peer_b):
            (peer / "file.txt").write_bytes(content)
            set_mtime(peer / "file.txt", old_t)

        stdout, _, rc = run_sync(["+" + str(peer_a), str(peer_b)])
        check(rc == 0, f"010.6 setup run failed: rc={rc}")

        # Remove file from peer_a and tombstone the snapshot row.
        # deleted_time = last_seen (the sync timestamp, which is ~now >> old_t).
        (peer_a / "file.txt").unlink()
        snap_a = snapshot_db(peer_a)
        with sqlite3.connect(str(snap_a)) as conn:
            row = conn.execute(
                "SELECT id FROM snapshot WHERE basename='file.txt'"
            ).fetchone()
            if row:
                conn.execute(
                    "UPDATE snapshot SET deleted_time=last_seen WHERE id=?",
                    (row[0],),
                )
                conn.commit()

        # peer_a: absent, deleted_time = recent sync time -> deleted
        #         deletion_estimate (~now) >> old_t -> deletion wins.
        # peer_b: live file (mod_time = old_t), matches snapshot -> unchanged.
        # Decision rule 4: deletion wins -> X on peer_b.
        stdout, _, rc = run_sync(["--dry-run", str(peer_a), str(peer_b)])
        check(rc == 0, f"010.6 dry run failed: rc={rc}")
        check(
            "X file.txt" in stdout,
            f"010.6: absent entry with non-NULL deleted_time must vote deleted and displace file on surviving peer; stdout={stdout!r}",
        )


# ---------------------------------------------------------------------------
# 010.7 -- absent + NULL deleted_time -> absent-unconfirmed (not a deletion vote)
# ---------------------------------------------------------------------------

def test_010_7_absent_unconfirmed_not_deletion() -> None:
    """Absent file with NULL deleted_time is absent-unconfirmed, not a deletion.

    When last_seen does not exceed the file's max mod_time (rule 4b), the
    absent-unconfirmed peer does not vote deletion -- the file is re-copied.
    A future mod_time guarantees last_seen < max_mod_time.
    """
    with tempfile.TemporaryDirectory() as tmp:
        peer_a = Path(tmp) / "peer_a"
        peer_b = Path(tmp) / "peer_b"
        peer_a.mkdir()
        peer_b.mkdir()

        # Future mod_time ensures rule 4b: last_seen (sync time ~now) <= max_mod_time
        # (far future) -> not a deletion -> re-copy.
        content = b"future file"
        future_t = time.time() + 86400  # 1 day ahead

        for peer in (peer_a, peer_b):
            (peer / "file.txt").write_bytes(content)
            set_mtime(peer / "file.txt", future_t)

        stdout, _, rc = run_sync(["+" + str(peer_a), str(peer_b)])
        check(rc == 0, f"010.7 setup run failed: rc={rc}")

        # Delete file from peer_a; leave its snapshot row with deleted_time = NULL.
        (peer_a / "file.txt").unlink()

        # peer_a: absent, last_seen ~now, deleted_time NULL -> absent-unconfirmed.
        # Rule 4b: last_seen (~now) <= max_mod_time (now + 86400) -> not deletion.
        # -> re-copy to peer_a, no X line.
        stdout, _, rc = run_sync(["--dry-run", str(peer_a), str(peer_b)])
        check(rc == 0, f"010.7 dry run failed: rc={rc}")
        check(
            "X file.txt" not in stdout,
            f"010.7: absent-unconfirmed peer must not vote deletion (no X line); stdout={stdout!r}",
        )
        check(
            "C file.txt" in stdout,
            f"010.7: absent-unconfirmed peer should trigger re-copy (C line); stdout={stdout!r}",
        )


# ---------------------------------------------------------------------------
# 010.8 -- absent + no snapshot row -> no opinion, does not remove file from others
# ---------------------------------------------------------------------------

def test_010_8_no_opinion_does_not_remove() -> None:
    """A peer with no snapshot row and no live file has no opinion.

    The no-opinion peer alone must not cause the file to be removed from peers
    that have it; the file is propagated to the no-opinion peer instead.
    """
    with tempfile.TemporaryDirectory() as tmp:
        peer_a = Path(tmp) / "peer_a"
        peer_b = Path(tmp) / "peer_b"
        peer_a.mkdir()
        peer_b.mkdir()

        # Seed a base file to give both peers snapshot history.
        base_t = time.time() - 300
        for peer in (peer_a, peer_b):
            (peer / "base.txt").write_bytes(b"base")
            set_mtime(peer / "base.txt", base_t)

        stdout, _, rc = run_sync(["+" + str(peer_a), str(peer_b)])
        check(rc == 0, f"010.8 setup run failed: rc={rc}")

        # Add a file on peer_b only -- no snapshot row on either peer.
        new_t = time.time() - 100
        (peer_b / "solo_file.txt").write_bytes(b"only on b")
        set_mtime(peer_b / "solo_file.txt", new_t)

        # peer_a: absent, no snapshot row for solo_file.txt -> no opinion.
        # peer_b: live file, no snapshot row -> new.
        # Decision: new wins; peer_a's no-opinion must not block or delete it.
        stdout, _, rc = run_sync(["--dry-run", str(peer_a), str(peer_b)])
        check(rc == 0, f"010.8 dry run failed: rc={rc}")
        check(
            "X solo_file.txt" not in stdout,
            f"010.8: no-opinion peer must not cause deletion of file on other peer; stdout={stdout!r}",
        )
        check(
            "C solo_file.txt" in stdout,
            f"010.8: file on peer with no snapshot row elsewhere must be propagated; stdout={stdout!r}",
        )


# ---------------------------------------------------------------------------
# Runner -- collect all failures; exit 1 only when something fails.
# ---------------------------------------------------------------------------

TESTS = [
    test_010_1_unchanged_not_recopied,
    test_010_1_tolerance_within_5s,
    test_010_2_byte_size_differs_is_modified,
    test_010_3_mod_time_differs_gt5s_is_modified,
    test_010_4_resurrection_live_over_tombstone,
    test_010_5_new_file_propagated,
    test_010_6_deleted_entry_displaces_existing,
    test_010_7_absent_unconfirmed_not_deletion,
    test_010_8_no_opinion_does_not_remove,
]

for _test in TESTS:
    try:
        _test()
        print(f"PASS: {_test.__name__}")
    except subprocess.TimeoutExpired:
        failures.append(f"{_test.__name__}: subprocess timed out")
        print(f"FAIL: {_test.__name__} (timeout)")
    except Exception as _exc:
        failures.append(f"{_test.__name__}: {_exc}")
        print(f"FAIL: {_test.__name__} ({_exc})")

if failures:
    print(f"\n{len(failures)} failure(s):")
    for _f in failures:
        print(f"  - {_f}")
    sys.exit(1)

print("\nAll checks passed.")
sys.exit(0)
