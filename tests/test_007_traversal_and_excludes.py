#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# ///

"""End-to-end test for reqs/007_traversal-and-excludes.md."""

from __future__ import annotations

import os
import sqlite3
import subprocess
import sys
import tempfile
from pathlib import Path


sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

WORKSPACE_ROOT = Path(r"C:\\Users\\human\\Desktop\\prjx\\kitchensync")
PROJECT_DIR = Path(r"C:\\Users\\human\\Desktop\\prjx\\kitchensync\\proj")
WINDOWS_EXE_PATH = WORKSPACE_ROOT / "released" / "kitchensync.exe"
POSIX_EXE_PATH = WORKSPACE_ROOT / "released" / "kitchensync"
RELEASED_EXE_PATH = WINDOWS_EXE_PATH if os.name == "nt" else POSIX_EXE_PATH

SNAPSHOT_COLUMNS = [
    "id",
    "parent_id",
    "basename",
    "mod_time",
    "byte_size",
    "last_seen",
    "deleted_time",
]


def _run_kitchensync(
    args: list[str],
    *,
    cwd: Path,
    timeout_seconds: float = 30.0,
) -> subprocess.CompletedProcess[str] | None:
    command = [str(RELEASED_EXE_PATH), *args]
    try:
        return subprocess.run(
            command,
            cwd=str(cwd),
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout_seconds,
            check=False,
        )
    except subprocess.TimeoutExpired:
        return subprocess.CompletedProcess(
            args=command,
            returncode=124,
            stdout="",
            stderr=f"command timed out after {timeout_seconds:.1f}s",
        )
    except FileNotFoundError as exc:
        return subprocess.CompletedProcess(
            args=command,
            returncode=127,
            stdout="",
            stderr=f"failed to launch kitchensync: {exc}",
        )


def _run_and_check(
    failures: list[str],
    req_id: str,
    args: list[str],
    cwd: Path,
    *,
    expected_exit: int = 0,
    timeout_seconds: float = 30.0,
    required_output: tuple[str, ...] = (),
) -> subprocess.CompletedProcess[str] | None:
    result = _run_kitchensync(args, cwd=cwd, timeout_seconds=timeout_seconds)
    if result is None:
        failures.append(f"{req_id}: command failed to run")
        return None

    if result.returncode != expected_exit:
        failures.append(
            f"{req_id}: expected exit {expected_exit}, got {result.returncode}. "
            f"args={args!r} stdout={result.stdout!r} stderr={result.stderr!r}"
        )

    output = f"{result.stdout}\n{result.stderr}".lower()
    for needle in required_output:
        if needle.lower() not in output:
            failures.append(
                f"{req_id}: expected output to contain {needle!r}. "
                f"args={args!r} stdout={result.stdout!r} stderr={result.stderr!r}"
            )
    return result


def _write_text(path: Path, value: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(value, encoding="utf-8")


def _assert_exists(failures: list[str], req_id: str, path: Path, reason: str) -> None:
    if not path.exists():
        failures.append(f"{req_id}: expected path to exist: {path}. {reason}")


def _assert_not_exists(failures: list[str], req_id: str, path: Path, reason: str) -> None:
    if path.exists():
        failures.append(f"{req_id}: expected path to be absent: {path}. {reason}")


def _assert_text_equals(
    failures: list[str],
    req_id: str,
    path: Path,
    expected: str,
    reason: str,
) -> None:
    if not path.exists():
        failures.append(f"{req_id}: missing path {path} when checking contents. {reason}")
        return
    actual = path.read_text(encoding="utf-8")
    if actual != expected:
        failures.append(
            f"{req_id}: unexpected content at {path}. expected {expected!r}, got {actual!r}. {reason}"
        )


def _snapshot_rows_by_path(peer_root: Path) -> dict[str, dict[str, object]]:
    snapshot_db = peer_root / ".kitchensync" / "snapshot.db"
    if not snapshot_db.is_file():
        return {}

    conn: sqlite3.Connection | None = None
    try:
        conn = sqlite3.connect(str(snapshot_db))
        conn.row_factory = sqlite3.Row
        rows = [
            dict(row)
            for row in conn.execute(
                "SELECT id,parent_id,basename,mod_time,byte_size,last_seen,deleted_time FROM snapshot;"
            ).fetchall()
        ]
    except Exception:
        return {}
    finally:
        if conn is not None:
            conn.close()

    by_id: dict[str, dict[str, object]] = {
        str(row["id"]): row
        for row in rows
        if row.get("id") is not None
    }

    if not by_id:
        return {}

    parent_markers: set[str] = set()
    for row in rows:
        parent = row.get("parent_id")
        if parent is not None:
            parent_text = str(parent)
            if parent_text not in by_id:
                parent_markers.add(parent_text)

    sentinel = next(iter(parent_markers)) if len(parent_markers) == 1 else None
    memo: dict[str, str] = {}

    def resolve(row_id: str, visiting: set[str]) -> str | None:
        if row_id in memo:
            return memo[row_id]
        if row_id in visiting:
            return None

        row = by_id.get(row_id)
        if row is None:
            return None

        basename = row.get("basename")
        parent = row.get("parent_id")
        if basename is None:
            return None

        if parent is None:
            return None

        parent_text = str(parent)
        if parent_text == sentinel:
            path = str(basename)
        else:
            visiting.add(row_id)
            parent_path = resolve(parent_text, visiting)
            visiting.remove(row_id)
            if parent_path is None:
                return None
            path = f"{parent_path}/{basename}"

        memo[row_id] = path
        return path

    by_path: dict[str, dict[str, object]] = {}
    for row_id in by_id:
        path = resolve(row_id, set())
        if path is None:
            continue
        by_path[path] = by_id[row_id]

    return by_path


def _snapshot_signature(row: dict[str, object] | None) -> tuple[object, ...] | None:
    if row is None:
        return None
    return tuple(row[column] for column in SNAPSHOT_COLUMNS)


def _snapshot_row_signature(peer_root: Path, rel_path: str) -> tuple[object, ...] | None:
    rows = _snapshot_rows_by_path(peer_root)
    return _snapshot_signature(rows.get(rel_path))


def _set_mode_if_posix(path: Path, mode: int) -> int | None:
    if os.name == "nt":
        return None
    current = path.stat().st_mode & 0o777
    path.chmod(mode)
    return current


def _case_union_and_subordinate(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_007_union_") as raw_root:
        root = Path(raw_root)
        canon = root / "peer_a"
        peer_b = root / "peer_b"
        peer_c = root / "peer_c"

        _write_text(canon / "seed.txt", "seed")
        _write_text(peer_b / "seed.txt", "seed")
        _write_text(peer_c / "seed.txt", "seed")

        bootstrap = _run_and_check(
            failures,
            "007.4/007.6",
            ["--verbosity", "error", f"+{canon}", str(peer_b)],
            cwd=root,
        )
        if bootstrap is None or bootstrap.returncode != 0:
            return

        _write_text(canon / "from-a.txt", "A")
        _write_text(peer_b / "from-b.txt", "B")
        _write_text(peer_c / "only-subordinate.txt", "C")

        sync = _run_and_check(
            failures,
            "007.4/007.6/007.7",
            ["--verbosity", "error", str(canon), str(peer_b), f"-{peer_c}"],
            cwd=root,
        )
        if sync is None or sync.returncode != 0:
            return

        _assert_text_equals(
            failures,
            "007.4/007.6",
            canon / "from-b.txt",
            "B",
            "from-b must propagate from peer_b to canon",
        )
        _assert_text_equals(
            failures,
            "007.4/007.6",
            peer_b / "from-a.txt",
            "A",
            "from-a must propagate from canon to peer_b",
        )
        _assert_not_exists(
            failures,
            "007.7",
            peer_c / "only-subordinate.txt",
            "subordinate entry not in group should be displaced",
        )


def _case_builtin_and_cli_excludes(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_007_excludes_") as raw_root:
        root = Path(raw_root)
        canon = root / "peer_a"
        peer_b = root / "peer_b"

        _write_text(canon / "seed.txt", "seed")
        _write_text(peer_b / "seed.txt", "seed")

        bootstrap = _run_and_check(
            failures,
            "007.9/007.10/007.13/007.14/007.15/007.16/007.17/007.18/007.19",
            ["--verbosity", "error", f"+{canon}", str(peer_b)],
            cwd=root,
        )
        if bootstrap is None or bootstrap.returncode != 0:
            return

        _write_text(canon / "visible.txt", "visible")
        _write_text(canon / "parent" / "keep.txt", "keep")
        _write_text(canon / ".git" / "ignored.txt", "ignore")
        _write_text(canon / ".kitchensync" / "forbidden.txt", "hidden")
        _write_text(canon / "ghost.txt", "v1")
        _write_text(peer_b / "ghost.txt", "v1")

        baseline = _run_and_check(
            failures,
            "007.9/007.10/007.13/007.14/007.15/007.16/007.17/007.18/007.19",
            ["--verbosity", "error", str(canon), str(peer_b)],
            cwd=root,
        )
        if baseline is None or baseline.returncode != 0:
            return

        git_before = (canon / ".git" / "ignored.txt").read_text(encoding="utf-8")
        ks_before = (canon / ".kitchensync" / "forbidden.txt").read_text(encoding="utf-8")
        ghost_before_p1 = _snapshot_row_signature(canon, "ghost.txt")
        ghost_before_p2 = _snapshot_row_signature(peer_b, "ghost.txt")

        _write_text(canon / "parent" / "skip.txt", "skip")
        _write_text(canon / "skip_file.txt", "skip")
        _write_text(canon / "skip_dir" / "nested.txt", "skip-dir")
        _write_text(canon / "absent.txt", "only-a")
        _write_text(canon / "ghost.txt", "v2")

        sync = _run_and_check(
            failures,
            "007.9/007.10/007.13/007.14/007.15/007.16/007.18/007.19",
            [
                "--verbosity",
                "error",
                str(canon),
                str(peer_b),
                "-x",
                "skip_file.txt",
                "-x",
                "skip_dir",
                "-x",
                "parent/skip.txt",
                "-x",
                "ghost.txt",
                "-x",
                "absent.txt",
            ],
            cwd=root,
        )
        if sync is None or sync.returncode != 0:
            return

        _assert_text_equals(
            failures,
            "007.13",
            peer_b / "visible.txt",
            "visible",
            "non-excluded live file should be copied",
        )
        _assert_text_equals(
            failures,
            "007.14",
            peer_b / "parent" / "keep.txt",
            "keep",
            "exclude parent/skip.txt should not block parent/keep.txt",
        )

        _assert_not_exists(
            failures,
            "007.13",
            peer_b / "skip_file.txt",
            "explicit file exclude should skip only the named file",
        )
        _assert_not_exists(
            failures,
            "007.15",
            peer_b / "skip_dir",
            "explicit directory exclude should skip descendants",
        )
        _assert_not_exists(
            failures,
            "007.14/007.15",
            peer_b / "parent" / "skip.txt",
            "explicitly skipped nested file should not be copied",
        )
        _assert_not_exists(
            failures,
            "007.18",
            peer_b / "absent.txt",
            "excluded path absent on target should remain absent",
        )

        _assert_not_exists(
            failures,
            "007.9",
            peer_b / ".git",
            ".git tree is built-in excluded from traversal",
        )
        _assert_not_exists(
            failures,
            "007.10",
            peer_b / ".kitchensync" / "forbidden.txt",
            ".kitchensync forbidden path is built-in excluded from traversal",
        )
        _assert_text_equals(
            failures,
            "007.17",
            canon / ".git" / "ignored.txt",
            git_before,
            "existing .git content should be left untouched",
        )
        _assert_text_equals(
            failures,
            "007.17",
            canon / ".kitchensync" / "forbidden.txt",
            ks_before,
            "existing .kitchensync content should be left untouched",
        )

        _assert_text_equals(
            failures,
            "007.16",
            peer_b / "ghost.txt",
            "v1",
            "existing excluded path should be left untouched, so copy must not occur",
        )

        ghost_after_p1 = _snapshot_row_signature(canon, "ghost.txt")
        ghost_after_p2 = _snapshot_row_signature(peer_b, "ghost.txt")

        if ghost_before_p1 != ghost_after_p1:
            failures.append(
                "007.19: snapshot row for canon ghost path changed while it was excluded"
            )
        if ghost_before_p2 != ghost_after_p2:
            failures.append(
                "007.19: snapshot row for peer-b ghost path changed while it was excluded"
            )


def _case_symlink_and_special_excludes(failures: list[str]) -> None:
    if os.name == "nt":
        # not reasonably testable on this platform without elevated privileges: 007.11, 007.12
        return

    with tempfile.TemporaryDirectory(prefix="ks_007_links_") as raw_root:
        root = Path(raw_root)
        canon = root / "peer_a"
        peer_b = root / "peer_b"

        _write_text(canon / "seed.txt", "seed")
        _write_text(peer_b / "seed.txt", "seed")

        bootstrap = _run_and_check(
            failures,
            "007.11/007.12",
            ["--verbosity", "error", f"+{canon}", str(peer_b)],
            cwd=root,
        )
        if bootstrap is None or bootstrap.returncode != 0:
            return

        _write_text(canon / "target.txt", "target")
        _write_text(canon / "real-dir" / "payload.txt", "payload")
        os.symlink("target.txt", canon / "linked-file")
        os.symlink("real-dir", canon / "linked-dir")
        os.mkfifo(canon / "special-fifo")

        sync = _run_and_check(
            failures,
            "007.11/007.12",
            ["--verbosity", "error", str(canon), str(peer_b)],
            cwd=root,
        )
        if sync is None or sync.returncode != 0:
            return

        _assert_not_exists(
            failures,
            "007.11",
            peer_b / "linked-file",
            "symbolic link to file must be excluded",
        )
        _assert_not_exists(
            failures,
            "007.11",
            peer_b / "linked-dir",
            "symbolic link directory must be excluded",
        )
        _assert_not_exists(
            failures,
            "007.12",
            peer_b / "special-fifo",
            "special file types should be excluded",
        )
        _assert_exists(
            failures,
            "007.11",
            canon / "linked-file",
            "excluded special entries should remain on source peer",
        )
        _assert_exists(
            failures,
            "007.12",
            canon / "special-fifo",
            "special file should remain on source peer",
        )


def _case_listing_failure_non_canon_peer(failures: list[str]) -> None:
    if os.name == "nt":
        # not reasonably testable on this platform without cross-platform directory mode control: 007.20/007.22/007.23/007.24/007.31
        return

    with tempfile.TemporaryDirectory(prefix="ks_007_fail_non_canon_") as raw_root:
        root = Path(raw_root)
        canon = root / "peer_a"
        failing = root / "peer_b"
        normal = root / "peer_c"

        _write_text(canon / "seed.txt", "seed")
        _write_text(failing / "seed.txt", "seed")
        _write_text(normal / "seed.txt", "seed")

        bootstrap = _run_and_check(
            failures,
            "007.22",
            ["--verbosity", "error", f"+{canon}", str(failing), str(normal)],
            cwd=root,
        )
        if bootstrap is None or bootstrap.returncode != 0:
            return

        block = "blocked"
        _write_text(canon / block / "from-a.txt", "from-a")
        _write_text(failing / block / "keep.txt", "keep")
        _write_text(normal / block / "from-c.txt", "from-c")

        before_signature = _snapshot_row_signature(failing, f"{block}/keep.txt")

        original_mode = _set_mode_if_posix(failing / block, 0)
        if original_mode is None:
            failures.append("007.20/007.22/007.23: failed to alter directory mode for controlled failure")
            return

        sync = _run_and_check(
            failures,
            "007.20/007.21/007.22/007.23/007.24",
            [
                "--verbosity",
                "error",
                "--retries-list",
                "2",
                str(canon),
                str(failing),
                str(normal),
            ],
            cwd=root,
            required_output=("listing failed for",),
        )
        _set_mode_if_posix(failing / block, original_mode)

        if sync is None or sync.returncode != 0:
            return

        _assert_exists(
            failures,
            "007.22",
            normal / block / "from-a.txt",
            "active contributing peer listing should still sync subtree for other peers",
        )
        _assert_not_exists(
            failures,
            "007.24",
            failing / block / "from-a.txt",
            "failed peer should be excluded from the failed subtree",
        )

        after_signature = _snapshot_row_signature(failing, f"{block}/keep.txt")
        if before_signature != after_signature:
            failures.append(
                "007.23: snapshot row changed for failing listing peer under failed subtree"
            )

        retry = _run_and_check(
            failures,
            "007.31",
            [
                "--verbosity",
                "error",
                "--retries-list",
                "2",
                str(canon),
                str(failing),
                str(normal),
            ],
            cwd=root,
        )
        if retry is None or retry.returncode != 0:
            return

        _assert_exists(
            failures,
            "007.31",
            failing / block / "from-a.txt",
            "peer should participate in later runs once listing succeeds",
        )


def _case_listing_failure_canon_peer(failures: list[str]) -> None:
    if os.name == "nt":
        # not reasonably testable on this platform without cross-platform directory mode control: 007.25/007.26/007.27/007.31
        return

    with tempfile.TemporaryDirectory(prefix="ks_007_fail_canon_") as raw_root:
        root = Path(raw_root)
        canon = root / "peer_a"
        peer_b = root / "peer_b"

        _write_text(canon / "seed.txt", "seed")
        _write_text(peer_b / "seed.txt", "seed")

        bootstrap = _run_and_check(
            failures,
            "007.25",
            ["--verbosity", "error", f"+{canon}", str(peer_b)],
            cwd=root,
        )
        if bootstrap is None or bootstrap.returncode != 0:
            return

        block = "blocked"
        _write_text(canon / block / "from-a.txt", "from-a")
        _write_text(peer_b / block / "existing.txt", "existing")

        existing_signature = _snapshot_row_signature(peer_b, f"{block}/existing.txt")
        original_mode = _set_mode_if_posix(canon / block, 0)
        if original_mode is None:
            failures.append("007.25/007.26/007.27: failed to alter directory mode for controlled canon failure")
            return

        sync = _run_and_check(
            failures,
            "007.25/007.26/007.27/007.31",
            [
                "--verbosity",
                "error",
                "--retries-list",
                "2",
                f"+{canon}",
                str(peer_b),
            ],
            cwd=root,
            required_output=("listing failed for",),
        )
        _set_mode_if_posix(canon / block, original_mode)

        if sync is None or sync.returncode != 0:
            return

        _assert_not_exists(
            failures,
            "007.25",
            peer_b / block / "from-a.txt",
            "canon subtree listing failure should skip all decisions in that subtree",
        )
        _assert_text_equals(
            failures,
            "007.26",
            peer_b / block / "existing.txt",
            "existing",
            "canon subtree listing failure should not modify existing peer files",
        )

        after_signature = _snapshot_row_signature(peer_b, f"{block}/existing.txt")
        if existing_signature != after_signature:
            failures.append(
                "007.27: snapshot rows changed in canon failed subtree while canon listing failed"
            )

        recovery = _run_and_check(
            failures,
            "007.31",
            [
                "--verbosity",
                "error",
                "--retries-list",
                "2",
                f"+{canon}",
                str(peer_b),
            ],
            cwd=root,
        )
        if recovery is None or recovery.returncode != 0:
            return

        _assert_exists(
            failures,
            "007.31",
            peer_b / block / "from-a.txt",
            "peer should participate in later runs when canon listing succeeds",
        )


def _case_all_contributing_fail(failures: list[str]) -> None:
    if os.name == "nt":
        # not reasonably testable on this platform without controlled directory mode failure: 007.28/007.29/007.30
        return

    with tempfile.TemporaryDirectory(prefix="ks_007_all_fail_") as raw_root:
        root = Path(raw_root)
        peer_a = root / "peer_a"
        peer_b = root / "peer_b"
        peer_c = root / "peer_c"

        _write_text(peer_a / "seed.txt", "seed")
        _write_text(peer_b / "seed.txt", "seed")
        _write_text(peer_c / "seed.txt", "seed")

        bootstrap = _run_and_check(
            failures,
            "007.28",
            ["--verbosity", "error", f"+{peer_a}", str(peer_b), str(peer_c)],
            cwd=root,
        )
        if bootstrap is None or bootstrap.returncode != 0:
            return

        block = "blocked"
        _write_text(peer_a / block / "a-only.txt", "a-only")
        _write_text(peer_b / block / "b-only.txt", "b-only")
        _write_text(peer_c / block / "c-only.txt", "c-only")

        mode_a = _set_mode_if_posix(peer_a / block, 0)
        mode_b = _set_mode_if_posix(peer_b / block, 0)
        if mode_a is None or mode_b is None:
            failures.append("007.28/007.29/007.30: failed to alter directory mode for all-contributing failure")
            return

        sync = _run_and_check(
            failures,
            "007.28/007.29/007.30",
            [
                "--verbosity",
                "error",
                "--retries-list",
                "2",
                str(peer_a),
                str(peer_b),
                f"-{peer_c}",
            ],
            cwd=root,
            required_output=("listing failed for",),
        )
        _set_mode_if_posix(peer_a / block, mode_a)
        _set_mode_if_posix(peer_b / block, mode_b)

        if sync is None or sync.returncode != 0:
            return

        _assert_not_exists(
            failures,
            "007.29",
            peer_b / block / "a-only.txt",
            "all-contributing failure should not process entries in peer-b subtree",
        )
        _assert_not_exists(
            failures,
            "007.29",
            peer_a / block / "b-only.txt",
            "all-contributing failure should not process entries in peer-a subtree",
        )
        _assert_exists(failures, "007.30", peer_c / block / "c-only.txt", "all-contributing failure should not displace subordinate")


def _case_unreachable_peer(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_007_unreachable_") as raw_root:
        root = Path(raw_root)
        peer_a = root / "peer_a"
        peer_b = root / "peer_b"

        _write_text(peer_a / "seed.txt", "seed")
        _write_text(peer_b / "seed.txt", "seed")

        bootstrap = _run_and_check(
            failures,
            "007.32",
            ["--verbosity", "error", f"+{peer_a}", str(peer_b)],
            cwd=root,
        )
        if bootstrap is None or bootstrap.returncode != 0:
            return

        _write_text(peer_a / "changed.txt", "changed")

        unreachable_file = root / "unreachable-peer"
        unreachable_file.write_text("not-a-directory", encoding="utf-8")

        unreachable = _run_and_check(
            failures,
            "007.32/007.33/007.34",
            ["--verbosity", "error", f"+{peer_a}", str(peer_b), str(unreachable_file)],
            cwd=root,
            required_output=("unreachable",),
        )
        if unreachable is None or unreachable.returncode != 0:
            return

        _assert_exists(
            failures,
            "007.34",
            peer_b / "changed.txt",
            "reachable peers should continue and sync when one peer is unreachable",
        )

        if os.name != "nt":
            # 007.33/007.35/007.36 in this environment
            _write_text(peer_b / "existing.txt", "existing")
            offline = root / "peer_offline"
            _write_text(offline / "seed.txt", "seed")
            _write_text(offline / "solo.txt", "solo")

            bootstrap2 = _run_and_check(
                failures,
                "007.35",
                ["--verbosity", "error", f"+{peer_a}", str(peer_b), str(offline)],
                cwd=root,
            )
            if bootstrap2 is None or bootstrap2.returncode != 0:
                return

            before_signature = _snapshot_row_signature(offline, "solo.txt")
            original_mode = _set_mode_if_posix(offline, 0)
            if original_mode is None:
                failures.append("007.35: failed to simulate unreachable directory peer")
                return

            offline_run = _run_and_check(
                failures,
                "007.33/007.34/007.35",
                [
                    "--verbosity",
                    "error",
                    "--retries-list",
                    "2",
                    f"+{peer_a}",
                    str(peer_b),
                    str(offline),
                ],
                cwd=root,
            )
            _set_mode_if_posix(offline, original_mode)
            if offline_run is None or offline_run.returncode != 0:
                return

            after_signature = _snapshot_row_signature(offline, "solo.txt")
            if before_signature != after_signature:
                failures.append(
                    "007.35: unreachable peer snapshot row changed during reachability failure"
                )

            offline_recover = _run_and_check(
                failures,
                "007.36",
                [
                    "--verbosity",
                    "error",
                    f"+{peer_a}",
                    str(peer_b),
                    str(offline),
                ],
                cwd=root,
            )
            if offline_recover is None or offline_recover.returncode != 0:
                return

            _assert_exists(
                failures,
                "007.36",
                offline / "changed.txt",
                "unreachable peer should participate once it becomes reachable again",
            )
        else:
            # not reasonably testable across this platform without directory permission control: 007.35/007.36
            pass


def main() -> int:
    failures: list[str] = []

    if not RELEASED_EXE_PATH.is_file():
        failures.append(f"precondition: missing executable at {RELEASED_EXE_PATH}")

    if failures:
        for failure in failures:
            print(f"FAIL: {failure}")
        return 1

    test_cases = [
        _case_union_and_subordinate,
        _case_builtin_and_cli_excludes,
        _case_symlink_and_special_excludes,
        _case_listing_failure_non_canon_peer,
        _case_listing_failure_canon_peer,
        _case_all_contributing_fail,
        _case_unreachable_peer,
    ]

    for test_case in test_cases:
        try:
            test_case(failures)
        except Exception as exc:  # noqa: BLE001
            failures.append(f"unexpected exception in {test_case.__name__}: {exc!r}")

    # not reasonably testable:
    # 007.1 (single combined-tree recursive walk)
    # 007.2 (per-level concurrent listing scheduling)
    # 007.3 (decide-before-recurse observable in-file behavior not directly inspectable)
    # 007.5 (snapshot rows never adding names in traversal set; snapshot internals are not directly exposed)
    # 007.8 (case-insensitive tie-breaker ordering)

    if failures:
        for index, failure in enumerate(failures, start=1):
            print(f"FAIL[{index:02d}]: {failure}")
        return 1

    print("PASS: tests/test_007_traversal_and_excludes.py")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
