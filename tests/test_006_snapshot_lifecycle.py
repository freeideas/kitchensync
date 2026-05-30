#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# ///

"""End-to-end test for reqs/006_snapshot-lifecycle.md."""

from __future__ import annotations

import os
import random
import shutil
import sqlite3
import subprocess
import sys
import tempfile
from datetime import datetime, timezone
from pathlib import Path
from typing import Iterable


if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
if hasattr(sys.stderr, "reconfigure"):
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")


WORKSPACE_ROOT = Path(r"C:\\Users\\human\\Desktop\\prjx\\kitchensync")
PROJECT_DIR = Path(r"C:\\Users\\human\\Desktop\\prjx\\kitchensync\\proj")
RELEASED_EXE = WORKSPACE_ROOT / "released" / ("kitchensync.exe" if os.name == "nt" else "kitchensync")

SNAPSHOT_ID_CHARS = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"


def _run_kitchensync(
    args: Iterable[str],
    *,
    cwd: Path,
    timeout_seconds: float = 30.0,
) -> subprocess.CompletedProcess[str] | None:
    command = [str(RELEASED_EXE), *args]
    try:
        return subprocess.run(
            command,
            cwd=str(cwd),
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout_seconds,
        )
    except subprocess.TimeoutExpired:
        return None
    except FileNotFoundError:
        return subprocess.CompletedProcess(
            args=command,
            returncode=127,
            stdout="",
            stderr="released executable not found",
        )


def _fail_if(failures: list[str], condition: bool, req_id: str, message: str) -> None:
    if not condition:
        failures.append(f"{req_id}: {message}")


def _snapshot_db(peer: Path) -> Path:
    return peer / ".kitchensync" / "snapshot.db"


def _snapshot_swap_root(peer: Path) -> Path:
    return peer / ".kitchensync" / "SWAP" / "snapshot.db"


def _seed_peer(peer: Path, files: dict[str, str] | None = None) -> None:
    peer.mkdir(parents=True, exist_ok=True)
    files = files or {}
    for relative_name, content in files.items():
        target = peer / relative_name
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(content, encoding="utf-8")


def _snapshot_basenames(snapshot_path: Path) -> set[str]:
    if not snapshot_path.is_file():
        return set()
    conn: sqlite3.Connection | None = None
    try:
        conn = sqlite3.connect(str(snapshot_path))
        rows = conn.execute("SELECT basename FROM snapshot;").fetchall()
        return {str(row[0]) for row in rows if row and row[0] is not None}
    except Exception:
        return set()
    finally:
        if conn is not None:
            conn.close()


def _snapshot_mtime(snapshot_path: Path) -> float | None:
    try:
        return snapshot_path.stat().st_mtime
    except OSError:
        return None


def _new_id() -> str:
    return "".join(random.choice(SNAPSHOT_ID_CHARS) for _ in range(11))


def _inject_marker(snapshot_path: Path, marker_name: str) -> None:
    conn: sqlite3.Connection | None = None
    timestamp = datetime.now(tz=timezone.utc).strftime("%Y-%m-%d_%H-%M-%S_%fZ")
    try:
        conn = sqlite3.connect(str(snapshot_path))
        row = conn.execute("SELECT parent_id FROM snapshot LIMIT 1;").fetchone()
        parent_id = row[0] if row and row[0] is not None else _new_id()
        node_id = _new_id()
        conn.execute(
            """
            INSERT INTO snapshot (id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            """,
            (node_id, str(parent_id), marker_name, timestamp, 0, timestamp, None),
        )
        conn.commit()
    finally:
        if conn is not None:
            conn.close()


def _copy_snapshot_with_marker(source: Path, destination: Path, marker: str | None) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(str(source), str(destination))
    if marker:
        _inject_marker(destination, marker)


def _prepare_initial_pair(failures: list[str], req_id: str, root: Path) -> tuple[Path, Path] | None:
    canon = root / "canon"
    peer = root / "peer"
    _seed_peer(canon, {"seed.txt": "seed"})
    _seed_peer(peer)

    result = _run_kitchensync([f"+{canon}", str(peer)], cwd=root)
    _fail_if(
        failures,
        result is not None,
        req_id,
        "command timed out",
    )
    if result is None:
        return None

    _fail_if(
        failures,
        result.returncode == 0,
        req_id,
        f"expected exit code 0, got {result.returncode}; stdout={result.stdout!r}; stderr={result.stderr!r}",
    )
    if result.returncode != 0:
        return None

    _fail_if(
        failures,
        _snapshot_db(peer).is_file(),
        req_id,
        f"missing live snapshot file at {_snapshot_db(peer)}",
    )
    if not _snapshot_db(peer).is_file():
        return None

    return canon, peer


def check_recovery_with_old_and_snapshot(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_006_recov_old_snapshot_") as raw_root:
        root = Path(raw_root)
        setup = _prepare_initial_pair(failures, "006.5", root)
        if setup is None:
            return
        canon, peer = setup

        baseline = _snapshot_db(peer)
        swap = _snapshot_swap_root(peer)
        _copy_snapshot_with_marker(baseline, swap / "old", "006.5_old_marker")
        _copy_snapshot_with_marker(baseline, swap / "new", "006.5_new_marker")

        (canon / "added.txt").write_text("peer-recovery", encoding="utf-8")
        result = _run_kitchensync([f"+{canon}", str(peer)], cwd=root)

        _fail_if(
            failures,
            result is not None,
            "006.5/006.6/006.1/006.2",
            "normal run timed out",
        )
        if result is None:
            return
        _fail_if(
            failures,
            result.returncode == 0,
            "006.5/006.6/006.1/006.2",
            f"normal run expected exit 0, got {result.returncode}; stdout={result.stdout!r}; stderr={result.stderr!r}",
        )
        if result.returncode != 0:
            return

        _fail_if(failures, not (swap / "new").exists(), "006.5", "SWAP/snapshot.db/new was not deleted")
        _fail_if(failures, not (swap / "old").exists(), "006.6", "SWAP/snapshot.db/old was not deleted")
        _fail_if(
            failures,
            _snapshot_db(peer).is_file(),
            "006.1/006.2",
            f"live snapshot is missing at {_snapshot_db(peer)} after startup",
        )
        _fail_if(
            failures,
            (peer / "added.txt").is_file(),
            "006.1",
            "canonical delta file was not applied after peer SWAP recovery",
        )
        _fail_if(
            failures,
            "006.5_old_marker" not in _snapshot_basenames(_snapshot_db(peer)),
            "006.2",
            "stale SWAP/old marker was unexpectedly promoted into live snapshot",
        )
        _fail_if(
            failures,
            "006.5_new_marker" not in _snapshot_basenames(_snapshot_db(peer)),
            "006.2",
            "stale SWAP/new marker was unexpectedly promoted into live snapshot",
        )


def check_recovery_old_new_to_snapshot(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_006_recov_old_new_") as raw_root:
        root = Path(raw_root)
        setup = _prepare_initial_pair(failures, "006.7/006.8", root)
        if setup is None:
            return
        canon, peer = setup

        baseline = _snapshot_db(peer)
        swap = _snapshot_swap_root(peer)
        live_snapshot = _snapshot_db(peer)
        swap.parent.mkdir(parents=True, exist_ok=True)
        _copy_snapshot_with_marker(live_snapshot, swap / "old", "006.7_old_marker")
        _copy_snapshot_with_marker(live_snapshot, swap / "new", "006.7_new_marker")
        live_snapshot.unlink()

        (canon / "added.txt").write_text("old-new-recovery", encoding="utf-8")
        result = _run_kitchensync([f"+{canon}", str(peer)], cwd=root)

        _fail_if(failures, result is not None, "006.7/006.8", "normal run timed out")
        if result is None:
            return
        _fail_if(
            failures,
            result.returncode == 0,
            "006.7/006.8",
            f"normal run expected exit 0, got {result.returncode}; stdout={result.stdout!r}; stderr={result.stderr!r}",
        )
        if result.returncode != 0:
            return

        _fail_if(
            failures,
            _snapshot_db(peer).is_file(),
            "006.7",
            "live snapshot was not restored from SWAP state",
        )
        _fail_if(failures, not (swap / "new").exists(), "006.7", "SWAP/snapshot.db/new was not removed after restore")
        _fail_if(failures, not (swap / "old").exists(), "006.8", "SWAP/snapshot.db/old was not removed after restore")
        snapshot_names = _snapshot_basenames(_snapshot_db(peer))
        _fail_if(
            failures,
            "006.7_new_marker" in snapshot_names,
            "006.7",
            "restored live snapshot did not retain the SWAP/new marker as expected",
        )
        _fail_if(
            failures,
            (peer / "added.txt").is_file(),
            "006.7",
            "peer did not receive canonical updates after SWAP recovery",
        )


def check_recovery_old_only(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_006_recov_old_only_") as raw_root:
        root = Path(raw_root)
        setup = _prepare_initial_pair(failures, "006.9", root)
        if setup is None:
            return
        canon, peer = setup

        baseline = _snapshot_db(peer)
        swap = _snapshot_swap_root(peer)
        live_snapshot = _snapshot_db(peer)
        _copy_snapshot_with_marker(live_snapshot, swap / "old", "006.9_old_marker")
        live_snapshot.unlink()

        (canon / "added.txt").write_text("old-only", encoding="utf-8")
        result = _run_kitchensync([f"+{canon}", str(peer)], cwd=root)

        _fail_if(failures, result is not None, "006.9", "normal run timed out")
        if result is None:
            return
        _fail_if(
            failures,
            result.returncode == 0,
            "006.9",
            f"normal run expected exit 0, got {result.returncode}; stdout={result.stdout!r}; stderr={result.stderr!r}",
        )
        if result.returncode != 0:
            return

        _fail_if(failures, not (swap / "old").exists(), "006.9", "SWAP/snapshot.db/old was not renamed away")
        _fail_if(
            failures,
            _snapshot_db(peer).is_file(),
            "006.9",
            "live snapshot missing after rename from SWAP/old",
        )
        snapshot_names = _snapshot_basenames(_snapshot_db(peer))
        _fail_if(
            failures,
            "006.9_old_marker" in snapshot_names,
            "006.9",
            "SWAP/old marker was not present in restored live snapshot",
        )


def check_recovery_new_with_snapshot(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_006_recov_new_with_snapshot_") as raw_root:
        root = Path(raw_root)
        setup = _prepare_initial_pair(failures, "006.10/006.11", root)
        if setup is None:
            return
        canon, peer = setup

        baseline = _snapshot_db(peer)
        swap = _snapshot_swap_root(peer)
        _copy_snapshot_with_marker(baseline, swap / "new", "006.10_new_marker")
        live_before = _snapshot_basenames(baseline)

        (canon / "added.txt").write_text("new-present", encoding="utf-8")
        result = _run_kitchensync([f"+{canon}", str(peer)], cwd=root)

        _fail_if(failures, result is not None, "006.10/006.11", "normal run timed out")
        if result is None:
            return
        _fail_if(
            failures,
            result.returncode == 0,
            "006.10/006.11",
            f"normal run expected exit 0, got {result.returncode}; stdout={result.stdout!r}; stderr={result.stderr!r}",
        )
        if result.returncode != 0:
            return

        _fail_if(
            failures,
            not (swap / "new").exists(),
            "006.10",
            "SWAP/snapshot.db/new was not deleted while live snapshot was present",
        )
        _fail_if(
            failures,
            "006.10_new_marker" not in _snapshot_basenames(_snapshot_db(peer)),
            "006.11",
            "SWAP/new marker unexpectedly survived as the live snapshot",
        )
        _fail_if(
            failures,
            _snapshot_basenames(_snapshot_db(peer)) >= live_before,
            "006.10/006.11",
            "live snapshot was not retained when SWAP/new should be dropped",
        )
        _fail_if(
            failures,
            (peer / "added.txt").is_file(),
            "006.10",
            "peer did not receive canonical update while SWAP recovery skipped new",
        )


def check_dry_run_skips_snapshot_recovery(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_006_dryrun_skip_") as raw_root:
        root = Path(raw_root)
        setup = _prepare_initial_pair(failures, "006.3", root)
        if setup is None:
            return
        canon, peer = setup

        baseline = _snapshot_db(peer)
        baseline_before = _snapshot_basenames(baseline)
        swap = _snapshot_swap_root(peer)
        _copy_snapshot_with_marker(baseline, swap / "old", "006.3_old_marker")
        _copy_snapshot_with_marker(baseline, swap / "new", "006.3_new_marker")

        result = _run_kitchensync(["--dry-run", f"+{canon}", str(peer)], cwd=root)
        _fail_if(failures, result is not None, "006.3/006.4", "dry-run timed out")
        if result is None:
            return
        _fail_if(
            failures,
            result.returncode == 0,
            "006.3/006.4",
            f"dry-run expected exit 0, got {result.returncode}; stdout={result.stdout!r}; stderr={result.stderr!r}",
        )
        if result.returncode != 0:
            return

        _fail_if(failures, (swap / "old").is_file(), "006.3", "SWAP/snapshot.db/old was unexpectedly deleted in --dry-run")
        _fail_if(failures, (swap / "new").is_file(), "006.3", "SWAP/snapshot.db/new was unexpectedly deleted in --dry-run")
        _fail_if(
            failures,
            _snapshot_basenames(_snapshot_db(peer)) == baseline_before,
            "006.4",
            "live snapshot basenames changed during --dry-run, indicating recovery-like local replacement",
        )


def check_missing_live_snapshot_creates_local_empty(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_006_missing_snapshot_") as raw_root:
        root = Path(raw_root)
        canon = root / "canon"
        peer = root / "peer"

        _seed_peer(canon, {"seed.txt": "seed"})
        _seed_peer(peer)
        result = _run_kitchensync([f"+{canon}", str(peer)], cwd=root)

        _fail_if(
            failures,
            result is not None,
            "006.13",
            "sync timed out",
        )
        if result is None:
            return
        _fail_if(
            failures,
            result.returncode == 0,
            "006.13",
            f"initial sync expected exit 0, got {result.returncode}; stdout={result.stdout!r}; stderr={result.stderr!r}",
        )
        _fail_if(
            failures,
            _snapshot_db(peer).is_file(),
            "006.13",
            f"live snapshot was not created for peer with no preexisting snapshot at {_snapshot_db(peer)}",
        )


def check_dry_run_does_not_upload_snapshots(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_006_dryrun_no_upload_") as raw_root:
        root = Path(raw_root)
        setup = _prepare_initial_pair(failures, "006.21/006.22/006.35", root)
        if setup is None:
            return
        canon, peer = setup

        snapshot = _snapshot_db(peer)
        swap_root = _snapshot_swap_root(peer)
        sidecars_before = sorted(
            path.name for path in peer.joinpath(".kitchensync").iterdir() if path.name.startswith("snapshot.db") and path.name != "snapshot.db"
        )
        snapshot_mtime_before = _snapshot_mtime(snapshot)
        _fail_if(
            failures,
            snapshot_mtime_before is not None,
            "006.21/006.22/006.35",
            f"live snapshot missing before dry-run at {snapshot}",
        )
        if snapshot_mtime_before is None:
            return

        (canon / "added.txt").write_text("upload-gate", encoding="utf-8")

        dry_run = _run_kitchensync(["--dry-run", f"+{canon}", str(peer)], cwd=root)
        _fail_if(
            failures,
            dry_run is not None,
            "006.22",
            "dry-run timed out",
        )
        if dry_run is None:
            return
        _fail_if(
            failures,
            dry_run.returncode == 0,
            "006.22",
            f"dry-run expected exit 0, got {dry_run.returncode}; stdout={dry_run.stdout!r}; stderr={dry_run.stderr!r}",
        )
        _fail_if(
            failures,
            _snapshot_mtime(snapshot) == snapshot_mtime_before,
            "006.22",
            "peer live snapshot mtime changed during --dry-run, indicating upload of local snapshot state",
        )
        _fail_if(
            failures,
            not (peer / "added.txt").exists(),
            "006.22",
            "target peer unexpectedly had new copied file after dry-run",
        )

        normal = _run_kitchensync([f"+{canon}", str(peer)], cwd=root)
        _fail_if(
            failures,
            normal is not None,
            "006.21",
            "normal run timed out",
        )
        if normal is None:
            return
        _fail_if(
            failures,
            normal.returncode == 0,
            "006.21",
            f"normal run expected exit 0, got {normal.returncode}; stdout={normal.stdout!r}; stderr={normal.stderr!r}",
        )
        if normal.returncode != 0:
            return
        _fail_if(
            failures,
            (peer / "added.txt").is_file(),
            "006.21",
            "target peer did not receive canonical update in normal run",
        )
        _fail_if(
            failures,
            (_snapshot_mtime(snapshot) or 0.0) > snapshot_mtime_before,
            "006.21/006.26",
            "peer live snapshot was not updated in normal run",
        )
        _fail_if(
            failures,
            sidecars_before == sorted(
                path.name
                for path in peer.joinpath(".kitchensync").iterdir()
                if path.name.startswith("snapshot.db") and path.name != "snapshot.db"
            ),
            "006.35",
            "snapshot sidecar artifacts were introduced in .kitchensync after normal run",
        )
        _fail_if(
            failures,
            not (swap_root / "new").exists(),
            "006.21/006.25",
            "SWAP/snapshot.db/new remained after normal run",
        )
        _fail_if(
            failures,
            not (swap_root / "old").exists(),
            "006.21/006.24",
            "SWAP/snapshot.db/old remained after normal run",
        )


def main() -> int:
    failures: list[str] = []

    _fail_if(
        failures,
        RELEASED_EXE.is_file(),
        "precondition",
        f"released executable missing at {RELEASED_EXE}",
    )
    _fail_if(
        failures,
        WORKSPACE_ROOT.is_dir(),
        "precondition",
        f"workspace root missing at {WORKSPACE_ROOT}",
    )
    _fail_if(
        failures,
        PROJECT_DIR.is_dir(),
        "precondition",
        f"project directory missing at {PROJECT_DIR}",
    )

    if failures:
        print(f"FAIL: test_006_snapshot_lifecycle.py ({len(failures)} precondition failure(s))")
        for failure in failures:
            print(f"  - {failure}")
        return 1

    # 006.1, 006.2, 006.5, 006.6
    check_recovery_with_old_and_snapshot(failures)
    # 006.7, 006.8, 006.9
    check_recovery_old_new_to_snapshot(failures)
    # 006.9
    check_recovery_old_only(failures)
    # 006.10, 006.11
    check_recovery_new_with_snapshot(failures)
    # 006.3, 006.4
    check_dry_run_skips_snapshot_recovery(failures)
    # 006.13
    check_missing_live_snapshot_creates_local_empty(failures)
    # 006.21, 006.22, 006.23, 006.24, 006.25, 006.26, 006.35
    check_dry_run_does_not_upload_snapshots(failures)

    # 006.12: not reasonably testable from released CLI output; local temp snapshot path is runtime-internal.
    # 006.14: not reasonably testable without injecting controlled snapshot download/recovery transport faults.
    # 006.15: not reasonably testable from process exit/code/file state alone for a peer-specific exclusion edge case.
    # 006.16: not reasonably testable without stable control over independent peer reachability outcomes.
    # 006.17: not reasonably testable without forcing canon-only exclusion independent of filesystem preconditions.
    # 006.18: not reasonably testable; requires per-peer local temporary DB instrumentation.
    # 006.19: not reasonably testable; requires per-peer local snapshot write tracing.
    # 006.20: not reasonably testable; requires synchronization timestamp ordering from scheduler internals.
    # 006.27: not reasonably testable on local filesystem behavior without custom transport implementation.
    # 006.28: not reasonably testable without forcing upload failure before SWAP/old exists.
    # 006.29: not reasonably testable without stable fault injection before SWAP/old creation.
    # 006.30: not reasonably testable without forcing upload failure after SWAP/old creation.
    # 006.31: not reasonably testable without controlling and preserving partially uploaded SWAP state.
    # 006.32, 006.33, 006.34: not reasonably testable without reliable overlapped-run coordination guarantees.

    if failures:
        print("FAIL: test_006_snapshot_lifecycle.py")
        for failure in failures:
            print(f"  - {failure}")
        return 1

    print("PASS: test_006_snapshot_lifecycle.py")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
