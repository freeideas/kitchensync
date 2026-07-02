# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

from __future__ import annotations

import os
import shutil
import sqlite3
import subprocess
import sys
import tempfile
from pathlib import Path


sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")


WORKSPACE_ROOT = Path("/home/ace/Desktop/prjx/kitchensync")
RELEASED_EXE = WORKSPACE_ROOT / "released" / "kitchensync.exe"


class CheckSet:
    def __init__(self) -> None:
        self.failures: list[str] = []

    def check(self, condition: bool, message: str) -> None:
        if not condition:
            self.failures.append(message)

    def equal(self, actual: object, expected: object, message: str) -> None:
        if actual != expected:
            self.failures.append(f"{message}: expected {expected!r}, got {actual!r}")


def run_kitchensync(args: list[str], cwd: Path, timeout: float = 30.0) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(RELEASED_EXE), *args],
        cwd=str(cwd),
        text=True,
        encoding="utf-8",
        errors="replace",
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=timeout,
        shell=False,
        check=False,
    )


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")


def make_empty_snapshot(path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.exists():
        path.unlink()
    with sqlite3.connect(str(path)) as db:
        mode = db.execute("PRAGMA journal_mode=DELETE").fetchone()[0]
        if str(mode).lower() == "wal":
            raise RuntimeError(f"could not create rollback-journal snapshot at {path}")
        db.executescript(
            """
            CREATE TABLE snapshot (
                id TEXT PRIMARY KEY,
                parent_id TEXT,
                basename TEXT NOT NULL,
                mod_time TEXT NOT NULL,
                byte_size INTEGER NOT NULL,
                last_seen TEXT,
                deleted_time TEXT
            );
            CREATE INDEX idx_snapshot_parent_id ON snapshot(parent_id);
            CREATE INDEX idx_snapshot_last_seen ON snapshot(last_seen);
            CREATE INDEX idx_snapshot_deleted_time ON snapshot(deleted_time);
            """
        )


def assert_snapshot_database(checks: CheckSet, path: Path, label: str) -> None:
    checks.check(path.is_file(), f"{label}: .kitchensync/snapshot.db should exist")
    if not path.is_file():
        return
    try:
        with sqlite3.connect(str(path)) as db:
            integrity = db.execute("PRAGMA integrity_check").fetchone()[0]
            mode = str(db.execute("PRAGMA journal_mode").fetchone()[0]).lower()
        checks.equal(integrity, "ok", f"{label}: snapshot.db should be a usable SQLite database")
        checks.check(mode != "wal", f"{label}: snapshot.db should not use WAL journal mode")
    except sqlite3.Error as exc:
        checks.failures.append(f"{label}: snapshot.db should open with sqlite3: {exc}")


def assert_no_peer_sidecars(checks: CheckSet, snapshot: Path, label: str) -> None:
    for suffix in ("-wal", "-shm", "-journal"):
        checks.check(
            not snapshot.with_name(snapshot.name + suffix).exists(),
            f"{label}: peer snapshot sidecar {snapshot.name + suffix} should not exist",
        )


def assert_no_snapshot_swap(checks: CheckSet, peer: Path, label: str) -> None:
    swap = peer / ".kitchensync" / "SWAP" / "snapshot.db"
    checks.check(not (swap / "old").exists(), f"{label}: SWAP snapshot old should be removed")
    checks.check(not (swap / "new").exists(), f"{label}: SWAP snapshot new should be removed")


def local_first_sync_checks(checks: CheckSet, root: Path) -> None:
    peer_a = root / "local-first" / "peer-a"
    peer_b = root / "local-first" / "peer-b"
    write_text(peer_a / "alpha.txt", "alpha\n")

    result = run_kitchensync([f"+{peer_a}", str(peer_b)], root)
    checks.equal(result.returncode, 0, "006 local first sync should exit 0")
    checks.equal(result.stderr, "", "006 local first sync should keep stderr empty")
    checks.check((peer_b / "alpha.txt").read_text(encoding="utf-8") == "alpha\n", "006 setup copy should complete")

    for label, peer in (("peer-a", peer_a), ("peer-b", peer_b)):
        snapshot = peer / ".kitchensync" / "snapshot.db"
        assert_snapshot_database(checks, snapshot, label)
        assert_no_peer_sidecars(checks, snapshot, label)
        assert_no_snapshot_swap(checks, peer, label)


def snapshot_replacement_checks(checks: CheckSet, root: Path) -> None:
    peer_a = root / "replace" / "peer-a"
    peer_b = root / "replace" / "peer-b"
    write_text(peer_a / "before.txt", "before\n")

    first = run_kitchensync([f"+{peer_a}", str(peer_b)], root)
    checks.equal(first.returncode, 0, "006 replacement setup should exit 0")

    write_text(peer_a / "after.txt", "after\n")
    second = run_kitchensync([str(peer_a), str(peer_b)], root)
    checks.equal(second.returncode, 0, "006 replacement run should exit 0")
    checks.equal(second.stderr, "", "006 replacement run should keep stderr empty")
    checks.check((peer_b / "after.txt").is_file(), "006 replacement run should copy new user data before finishing")

    for label, peer in (("replace peer-a", peer_a), ("replace peer-b", peer_b)):
        snapshot = peer / ".kitchensync" / "snapshot.db"
        assert_snapshot_database(checks, snapshot, label)
        assert_no_peer_sidecars(checks, snapshot, label)
        assert_no_snapshot_swap(checks, peer, label)


def snapshot_swap_recovery_checks(checks: CheckSet, root: Path) -> None:
    cases = [
        ("old-live-new", True, True, True),
        ("old-new-no-live", False, True, True),
        ("old-only", False, True, False),
        ("new-live", True, False, True),
        ("new-only", False, False, True),
    ]
    for name, live_exists, old_exists, new_exists in cases:
        case_root = root / "recovery" / name
        peer_a = case_root / "peer-a"
        peer_b = case_root / "peer-b"
        peer_a.mkdir(parents=True, exist_ok=True)
        peer_b.mkdir(parents=True, exist_ok=True)
        write_text(peer_a / "source.txt", f"{name}\n")

        live = peer_a / ".kitchensync" / "snapshot.db"
        swap = peer_a / ".kitchensync" / "SWAP" / "snapshot.db"
        if live_exists:
            make_empty_snapshot(live)
        if old_exists:
            make_empty_snapshot(swap / "old")
        if new_exists:
            make_empty_snapshot(swap / "new")

        result = run_kitchensync([f"+{peer_a}", str(peer_b)], root)
        checks.equal(result.returncode, 0, f"006 recovery {name} should exit 0")
        checks.equal(result.stderr, "", f"006 recovery {name} should keep stderr empty")
        assert_snapshot_database(checks, live, f"recovery {name}")
        assert_no_snapshot_swap(checks, peer_a, f"recovery {name}")


def main() -> int:
    checks = CheckSet()
    checks.check(RELEASED_EXE.is_file(), "released executable should exist")

    with tempfile.TemporaryDirectory(prefix="kitchensync-006-") as tmp:
        root = Path(tmp)
        try:
            local_first_sync_checks(checks, root)
            snapshot_replacement_checks(checks, root)
            snapshot_swap_recovery_checks(checks, root)
        except subprocess.TimeoutExpired as exc:
            checks.failures.append(f"KitchenSync subprocess timed out: {exc}")
        except OSError as exc:
            checks.failures.append(f"filesystem or process error: {exc}")

    # not reasonably testable: 006.10 local temporary {tmp}/{uuid}/snapshot.db path is internal and removed or unreported.
    # not reasonably testable: 006.12 requires forcing peer snapshot SWAP recovery failure through a transport fault.
    # not reasonably testable: 006.13 requires forcing snapshot download failure other than not found through a transport fault.
    # not reasonably testable: 006.14 local temporary snapshot reads/writes are internal and not exposed after process exit.
    # not reasonably testable: 006.15 requires observing transient ordering between copy queue completion and snapshot upload.
    # not reasonably testable: 006.18 requires observing the transient close point of SWAP snapshot new before rename.
    # not reasonably testable: 006.22 requires a named end-to-end transport fixture that rejects rename over existing destination.
    # not reasonably testable: 006.23 requires forcing upload failure after SWAP old exists.
    # not reasonably testable: 006.24 requires observing internal SQLite transaction state immediately before upload.
    # not reasonably testable: 006.25 requires observing internal SQLite statement, cursor, or reader lifetime.
    # not reasonably testable: 006.26 requires observing internal SQLite connection lifetime immediately before upload.
    # not reasonably testable: 006.27 requires observing the upload source as a closed file rather than a live SQLite connection.
    # not reasonably testable: 006.29 requires controlled overlapping normal runs and deterministic upload completion ordering.

    if checks.failures:
        print("FAIL")
        for failure in checks.failures:
            print(f"- {failure}")
        return 1

    print("PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
