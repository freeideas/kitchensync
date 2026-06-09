# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""End-to-end test for ./reqs/018_snapshot-maintenance.md

Covers requirements 018.1 through 018.6.  The test seeds a local peer's
snapshot.db with orphaned rows representing each case defined in the
requirement, runs a real sync, and inspects the database that the sync
writes back to the peer.
"""

import sqlite3
import subprocess
import sys
import tempfile
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

WORKSPACE_ROOT = Path("/home/ace/Desktop/prjx/kitchensync")
EXE = WORKSPACE_ROOT / "released" / "kitchensync.exe"

# Use an explicit --keep-del-days value so the test does not depend on the
# default.  90 days gives plenty of room between OLD_TS and RECENT_TS.
KEEP_DEL_DAYS = 90

# Today is 2026-06-08.
# OLD_TS   = 2025-01-01 -> ~523 days ago (clearly > 90 days -> eligible for removal)
# RECENT_TS = 2026-06-07 -> 1 day ago (clearly < 90 days -> must be kept)
OLD_TS = "2025-01-01_00-00-00_000000Z"
RECENT_TS = "2026-06-07_00-00-00_000000Z"

# Arbitrary 11-char base62 IDs for the three orphaned test rows.  These do not
# correspond to any file on any peer so traversal never visits them; only the
# opportunistic cleanup logic sees them.
_FAKE_PARENT = "00000000000"
_ID_OLD_TOMBSTONE = "00000000001"    # 018.1: old deleted_time -> must be removed
_ID_RECENT_TOMBSTONE = "00000000002" # 018.2: recent deleted_time -> must be kept
_ID_STALE_UNVISITED = "00000000003"  # 018.3: old last_seen, NULL deleted_time -> must be removed


def _create_snapshot(db_path: Path) -> None:
    """Create snapshot.db at db_path with three orphaned test rows."""
    db_path.parent.mkdir(parents=True, exist_ok=True)
    con = sqlite3.connect(str(db_path))
    con.executescript(
        """
        CREATE TABLE snapshot (
            id TEXT PRIMARY KEY,
            parent_id TEXT NOT NULL,
            basename TEXT NOT NULL,
            mod_time TEXT NOT NULL,
            byte_size INTEGER NOT NULL,
            last_seen TEXT,
            deleted_time TEXT
        );
        CREATE INDEX idx_parent_id    ON snapshot(parent_id);
        CREATE INDEX idx_last_seen    ON snapshot(last_seen);
        CREATE INDEX idx_deleted_time ON snapshot(deleted_time);
        """
    )
    con.executemany(
        "INSERT INTO snapshot VALUES (?,?,?,?,?,?,?)",
        [
            # 018.1: tombstone whose deleted_time predates keep-del-days cutoff
            (_ID_OLD_TOMBSTONE, _FAKE_PARENT, "old_deleted.txt",
             OLD_TS, 100, OLD_TS, OLD_TS),
            # 018.2: tombstone whose deleted_time is within keep-del-days
            (_ID_RECENT_TOMBSTONE, _FAKE_PARENT, "recent_deleted.txt",
             OLD_TS, 100, RECENT_TS, RECENT_TS),
            # 018.3: live-looking row for a path absent from all peers, last_seen old
            (_ID_STALE_UNVISITED, _FAKE_PARENT, "stale_unvisited.txt",
             OLD_TS, 100, OLD_TS, None),
        ],
    )
    con.commit()
    con.close()


def _has_row(db_path: Path, row_id: str) -> bool:
    con = sqlite3.connect(str(db_path))
    cur = con.execute("SELECT 1 FROM snapshot WHERE id=?", (row_id,))
    found = cur.fetchone() is not None
    con.close()
    return found


failures: list[str] = []


def _check(cond: bool, label: str) -> None:
    status = "PASS" if cond else "FAIL"
    print(f"{status}: {label}")
    if not cond:
        failures.append(label)


def _run_sync(*peers: Path, extra: list[str] | None = None) -> subprocess.CompletedProcess:
    cmd = [str(EXE), *(str(p) for p in peers)]
    if extra:
        cmd.extend(extra)
    return subprocess.run(
        cmd, capture_output=True, text=True,
        encoding="utf-8", errors="replace", timeout=120,
    )


def test_snapshot_maintenance() -> None:
    with tempfile.TemporaryDirectory() as _td:
        tmp = Path(_td)
        peer_a = tmp / "peer_a"
        peer_b = tmp / "peer_b"
        peer_a.mkdir()
        peer_b.mkdir()

        # A real file on both peers gives the sync walk something concrete to
        # process; identical content avoids triggering any copy.
        payload = "kitchensync 018 snapshot maintenance test\n"
        (peer_a / "data.txt").write_text(payload, encoding="utf-8")
        (peer_b / "data.txt").write_text(payload, encoding="utf-8")

        # Seed peer_a with orphaned rows covering all testable cases.
        # peer_b has no .kitchensync/ directory -> no snapshot.db ->
        # auto-subordinate on startup (per sync.md step 6).
        _create_snapshot(peer_a / ".kitchensync" / "snapshot.db")

        result = _run_sync(
            peer_a, peer_b,
            extra=["--keep-del-days", str(KEEP_DEL_DAYS)],
        )
        out, err = result.stdout, result.stderr

        # 018.6: sync exits 0 even when eligible orphaned rows exist.
        _check(
            result.returncode == 0,
            f"018.6: sync exits 0 with eligible rows present "
            f"(got {result.returncode}; stdout={out!r}; stderr={err!r})",
        )

        snap = peer_a / ".kitchensync" / "snapshot.db"
        if not snap.exists():
            _check(False, "snapshot.db missing after sync -- cannot verify 018.1/018.2/018.3")
            return

        # 018.1: tombstone row whose deleted_time is older than keep-del-days must be removed.
        _check(
            not _has_row(snap, _ID_OLD_TOMBSTONE),
            "018.1: old tombstone row (deleted_time older than keep-del-days) removed after sync",
        )

        # 018.2: tombstone row whose deleted_time is within keep-del-days must be kept.
        _check(
            _has_row(snap, _ID_RECENT_TOMBSTONE),
            "018.2: recent tombstone row (deleted_time within keep-del-days) kept after sync",
        )

        # 018.3: row with deleted_time NULL and last_seen older than keep-del-days,
        # for a path absent from all peers (not visited by traversal), must be removed.
        _check(
            not _has_row(snap, _ID_STALE_UNVISITED),
            "018.3: stale unvisited row (last_seen > keep-del-days, deleted_time NULL) removed after sync",
        )

        # not reasonably testable: 018.4 (maintenance timing vs first directory scan is internal)
        # not reasonably testable: 018.5 (maintenance timing vs first eligible copy is internal)


test_snapshot_maintenance()

if failures:
    print(f"\n{len(failures)} failure(s).")
    sys.exit(1)
else:
    print("\nAll checks passed.")
    sys.exit(0)
