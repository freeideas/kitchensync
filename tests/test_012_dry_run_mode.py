#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# ///

"""End-to-end verification for reqs/012_dry-run-mode.md."""

from __future__ import annotations

import os
import re
import shutil
import subprocess
import sys
import tempfile
import sqlite3
from datetime import datetime, timezone
from pathlib import Path
from typing import Callable

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

WORKSPACE_ROOT = Path(__file__).resolve().parent.parent
PROJECT_DIR = WORKSPACE_ROOT / "proj"
RELEASED_EXE = WORKSPACE_ROOT / "released" / ("kitchensync.exe" if os.name == "nt" else "kitchensync")
SNAPSHOT_NAME = "snapshot.db"
COPY_SLOT_RE = re.compile(r"copy-slots\s+active=(\d+)/(\d+)")


def _run_case(label: str, failures: list[str], fn: Callable[[], None]) -> None:
    try:
        fn()
    except AssertionError as exc:
        failures.append(f"{label}: {exc}")
    except Exception as exc:  # noqa: BLE001
        failures.append(f"{label}: unexpected exception: {exc!r}")


def _fail(failures: list[str], condition: bool, message: str) -> None:
    if not condition:
        failures.append(message)


def _run_kitchensync(
    args: list[str],
    *,
    cwd: Path,
    timeout_seconds: int = 45,
    env: dict[str, str] | None = None,
) -> subprocess.CompletedProcess[str] | None:
    command = [str(RELEASED_EXE), *args]
    launch_env = os.environ.copy()
    if env:
        launch_env.update(env)
    try:
        return subprocess.run(
            command,
            cwd=str(cwd),
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            env=launch_env,
            timeout=timeout_seconds,
            check=False,
        )
    except subprocess.TimeoutExpired:
        return subprocess.CompletedProcess(
            args=command,
            returncode=124,
            stdout="",
            stderr="kitchensync invocation timed out",
        )
    except (FileNotFoundError, OSError) as exc:
        return subprocess.CompletedProcess(
            args=command,
            returncode=127,
            stdout="",
            stderr=f"failed to launch released executable: {exc}",
        )


def _file_url(path: Path) -> str:
    return path.resolve().as_uri()


def _write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def _read_bytes(path: Path) -> bytes:
    return path.read_bytes()


def _set_mtime(path: Path, when: datetime) -> None:
    if when.tzinfo is None:
        when = when.replace(tzinfo=timezone.utc)
    timestamp = when.timestamp()
    try:
        os.utime(path, (timestamp, timestamp), follow_symlinks=False)
    except NotImplementedError:
        os.utime(path, (timestamp, timestamp))


def _snapshot_path(peer: Path) -> Path:
    return peer / ".kitchensync" / SNAPSHOT_NAME


def _local_temp_snapshot_paths(temp_root: Path) -> set[Path]:
    if not temp_root.is_dir():
        return set()
    try:
        return {
            path
            for path in temp_root.rglob("snapshot.db")
            if path.is_file()
        }
    except OSError:
        return set()


def _snapshot_row_count(path: Path) -> int | None:
    conn: sqlite3.Connection | None = None
    try:
        conn = sqlite3.connect(str(path))
        marker = conn.execute(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='snapshot' LIMIT 1;"
        ).fetchone()
        if marker is None:
            return None
        row = conn.execute("SELECT COUNT(*) FROM snapshot;").fetchone()
        if row is None:
            return None
        return int(row[0]) if row[0] is not None else None
    except Exception:
        return None
    finally:
        if conn is not None:
            conn.close()


def _temp_env(temp_root: Path) -> dict[str, str]:
    value = str(temp_root)
    return {"TMP": value, "TEMP": value, "TMPDIR": value}


def _snapshot_mtime(peer: Path) -> float | None:
    path = _snapshot_path(peer)
    try:
        return path.stat().st_mtime
    except OSError:
        return None


def _copy_file(src: Path, dst: Path) -> None:
    dst.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(str(src), str(dst))


def check_root_missing_is_unreachable_and_not_created(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_012_root_missing_") as raw_root:
        root = Path(raw_root)
        canon = root / "canon"
        missing_root = root / "missing-root" / "nested"

        _write_text(canon / "seed.txt", "seed")

        result = _run_kitchensync(
            ["--dry-run", f"+{_file_url(canon)}", str(missing_root)],
            cwd=root,
        )
        _fail(
            failures,
            result is not None,
            "012.1/012.2: dry-run invocation timed out or failed to start",
        )
        if result is None:
            return

        _fail(
            failures,
            result.returncode != 0,
            f"012.1/012.2: expected non-zero exit because a missing root peer is unreachable, got {result.returncode}. stdout={result.stdout!r} stderr={result.stderr!r}",
        )
        _fail(
            failures,
            not missing_root.exists(),
            "012.2: dry-run created missing peer root path",
        )
        _fail(
            failures,
            not missing_root.parent.exists(),
            "012.2: dry-run created missing peer root parent path",
        )


def check_missing_peer_is_skipped_and_other_peer_still_syncs_without_mutation(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_012_peer_skip_") as raw_root:
        root = Path(raw_root)
        canon = root / "canon"
        reachable = root / "reachable"
        missing_root = root / "missing" / "peer"

        _write_text(canon / "seed.txt", "from-canon")
        _write_text(reachable / "keep.txt", "kept")
        _set_mtime(reachable / "keep.txt", datetime(2026, 1, 1, 12, 0, 0, tzinfo=timezone.utc))

        result = _run_kitchensync(
            ["--dry-run", f"+{_file_url(canon)}", str(reachable), str(missing_root)],
            cwd=root,
        )
        _fail(
            failures,
            result is not None,
            "012.1/012.8: dry-run invocation timed out or failed to start",
        )
        if result is None:
            return

        _fail(
            failures,
            result.returncode == 0,
            f"012.1/012.8: dry-run should still complete when at least one reachable peer remains. got {result.returncode}. stdout={result.stdout!r} stderr={result.stderr!r}",
        )
        _fail(
            failures,
            not missing_root.exists(),
            "012.1/012.2: dry-run created an unreachable missing root or parent",
        )
        _fail(
            failures,
            "unreachable" in (result.stdout + result.stderr).lower(),
            "012.1: dry-run did not report the peer as unreachable",
        )
        _fail(
            failures,
            not (reachable / "seed.txt").exists(),
            "012.16: destination file was written in dry-run to a reachable peer",
        )
        _fail(
            failures,
            (reachable / "keep.txt").read_text(encoding="utf-8") == "kept",
            "012.17/012.18/012.19: existing destination data changed during dry-run",
        )


def check_no_peer_mutation_or_cleanup_on_file_peers(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_012_no_mutation_") as raw_root:
        root = Path(raw_root)
        canon = root / "canon"
        peer_a = root / "peer_a"
        peer_b = root / "peer_b"

        _write_text(canon / "copy_me.txt", "payload")
        _write_text(canon / "nested" / "inner.txt", "nested payload")

        # Destination baseline that should never change in dry-run.
        _write_text(peer_a / "legacy.txt", "must remain")
        _set_mtime(peer_a / "legacy.txt", datetime(2025, 12, 1, 9, 0, 0, tzinfo=timezone.utc))
        _write_text(peer_a / "keep" / "left.txt", "existing")
        _set_mtime(peer_a / "keep" / "left.txt", datetime(2026, 2, 1, 10, 0, 0, tzinfo=timezone.utc))

        # Pre-existing staging/metadata entries that should remain untouched in dry-run.
        _write_text(peer_a / ".kitchensync" / "BAK" / "2026-01-01_00-00-00_000000Z" / "legacy.txt", "bak-blocker")
        _write_text(peer_b / ".kitchensync" / "TMP" / "2026-01-01_00-00-00_000000Z" / "tmp-blocker", "tmp-blocker")
        _write_text(peer_a / ".kitchensync" / "SWAP" / "legacy.txt" / "old", "legacy-swap-old")
        _write_text(peer_a / ".kitchensync" / "SWAP" / "legacy.txt" / "new", "legacy-swap-new")

        before_children_a = sorted(p.name for p in (peer_a / ".kitchensync").iterdir()) if (peer_a / ".kitchensync").is_dir() else []
        before_children_b = (
            sorted(p.name for p in (peer_b / ".kitchensync").iterdir()) if (peer_b / ".kitchensync").is_dir() else []
        )
        before_keep = _read_bytes(peer_a / "keep" / "left.txt")
        before_keep_mtime = (peer_a / "keep" / "left.txt").stat().st_mtime
        before_legacy = _read_bytes(peer_a / "legacy.txt")
        before_legacy_mtime = (peer_a / "legacy.txt").stat().st_mtime
        before_swap_old = _read_bytes(peer_a / ".kitchensync" / "SWAP" / "legacy.txt" / "old")
        before_swap_new = _read_bytes(peer_a / ".kitchensync" / "SWAP" / "legacy.txt" / "new")
        before_bak = _read_bytes(peer_a / ".kitchensync" / "BAK" / "2026-01-01_00-00-00_000000Z" / "legacy.txt")
        before_tmp = _read_bytes(peer_b / ".kitchensync" / "TMP" / "2026-01-01_00-00-00_000000Z" / "tmp-blocker")
        before_meta_bak_files = {
            p.relative_to(peer_a / ".kitchensync" / "BAK").as_posix()
            for p in (peer_a / ".kitchensync" / "BAK").rglob("*")
            if p.is_file()
        }
        before_meta_tmp_files = {
            p.relative_to(peer_b / ".kitchensync" / "TMP").as_posix()
            for p in (peer_b / ".kitchensync" / "TMP").rglob("*")
            if p.is_file()
        }
        before_meta_swap_files = {
            p.relative_to(peer_a / ".kitchensync" / "SWAP").as_posix()
            for p in (peer_a / ".kitchensync" / "SWAP").rglob("*")
            if p.is_file()
        }

        result = _run_kitchensync(
            [
                "--dry-run",
                "--verbosity",
                "trace",
                "--max-copies",
                "1",
                f"+{_file_url(canon)}",
                str(peer_a),
                str(peer_b),
            ],
            cwd=root,
        )
        _fail(
            failures,
            result is not None,
            "012.3/012.4/012.15/012.20/012.21/012.22: dry-run invocation timed out or failed to start",
        )
        if result is None:
            return

        _fail(
            failures,
            result.returncode == 0,
            f"012.3/012.14/012.17/012.18/012.19/012.7/012.10/012.11: dry-run expected exit 0, got {result.returncode}. stdout={result.stdout!r} stderr={result.stderr!r}",
        )
        combined = (result.stdout + "\n" + result.stderr).lower()
        _fail(
            failures,
            "dry run" in result.stdout.lower(),
            "012.23: stdout did not contain required phrase 'dry run'",
        )

        # No destination write/displace/delete operations expected on file peers.
        _fail(
            failures,
            not (peer_a / "copy_me.txt").exists(),
            "012.16: destination file was written during dry-run",
        )
        _fail(
            failures,
            not (peer_b / "copy_me.txt").exists(),
            "012.16: destination file was written during dry-run",
        )
        _fail(
            failures,
            not (peer_a / "nested").exists(),
            "012.14: dry-run created destination directory while syncing",
        )
        _fail(
            failures,
            not (peer_b / "nested").exists(),
            "012.14: dry-run created destination directory while syncing",
        )
        _fail(
            failures,
            not (peer_a / ".kitchensync" / "SWAP" / "copy_me.txt").exists(),
            "012.15: dry-run created user-file SWAP directory",
        )
        _fail(
            failures,
            not (peer_a / ".kitchensync" / "TMP").exists(),
            "012.15: dry-run created user-file TMP metadata under destination",
        )

        _fail(
            failures,
            (peer_a / "keep" / "left.txt").read_text(encoding="utf-8") == "existing",
            "012.17/012.18/012.19: existing destination data was modified or displaced in dry-run",
        )
        _fail(
            failures,
            (peer_a / "keep" / "left.txt").stat().st_mtime == before_keep_mtime,
            "012.19: destination file mod_time changed in dry-run",
        )
        _fail(
            failures,
            (peer_a / "legacy.txt").read_bytes() == before_legacy,
            "012.17/012.18/012.19/012.20: existing destination file was changed in dry-run",
        )
        _fail(
            failures,
            (peer_a / "legacy.txt").stat().st_mtime == before_legacy_mtime,
            "012.19: destination legacy file mod_time changed in dry-run",
        )

        after_children_a = sorted(p.name for p in (peer_a / ".kitchensync").iterdir()) if (peer_a / ".kitchensync").is_dir() else []
        after_children_b = (
            sorted(p.name for p in (peer_b / ".kitchensync").iterdir()) if (peer_b / ".kitchensync").is_dir() else []
        )
        _fail(
            failures,
            after_children_a == before_children_a,
            "012.20/012.21/012.22: dry-run modified destination metadata top-level layout",
        )
        _fail(
            failures,
            after_children_b == before_children_b,
            "012.15/012.20/012.21/012.22: dry-run created forbidden destination metadata",
        )
        _fail(
            failures,
            _read_bytes(peer_a / ".kitchensync" / "SWAP" / "legacy.txt" / "old") == before_swap_old,
            "012.20/012.21/012.22: dry-run altered existing user SWAP path",
        )
        _fail(
            failures,
            _read_bytes(peer_a / ".kitchensync" / "SWAP" / "legacy.txt" / "new") == before_swap_new,
            "012.20/012.21/012.22: dry-run altered existing user SWAP path",
        )
        _fail(
            failures,
            _read_bytes(peer_a / ".kitchensync" / "BAK" / "2026-01-01_00-00-00_000000Z" / "legacy.txt") == before_bak,
            "012.17/012.21/012.22: dry-run altered existing BAK staging",
        )
        _fail(
            failures,
            _read_bytes(peer_b / ".kitchensync" / "TMP" / "2026-01-01_00-00-00_000000Z" / "tmp-blocker") == before_tmp,
            "012.22: dry-run altered existing TMP staging",
        )
        after_meta_bak_files = {
            p.relative_to(peer_a / ".kitchensync" / "BAK").as_posix()
            for p in (peer_a / ".kitchensync" / "BAK").rglob("*")
            if p.is_file()
        }
        after_meta_tmp_files = {
            p.relative_to(peer_b / ".kitchensync" / "TMP").as_posix()
            for p in (peer_b / ".kitchensync" / "TMP").rglob("*")
            if p.is_file()
        }
        after_meta_swap_files = {
            p.relative_to(peer_a / ".kitchensync" / "SWAP").as_posix()
            for p in (peer_a / ".kitchensync" / "SWAP").rglob("*")
            if p.is_file()
        }
        _fail(
            failures,
            before_meta_bak_files == after_meta_bak_files,
            "012.15/012.21/012.22: BAK metadata changed during dry-run",
        )
        _fail(
            failures,
            before_meta_tmp_files == after_meta_tmp_files,
            "012.15/012.22: TMP metadata changed during dry-run",
        )
        _fail(
            failures,
            before_meta_swap_files == after_meta_swap_files,
            "012.20/012.21: SWAP metadata changed during dry-run",
        )


def check_copy_slots_are_limited_during_dry_run(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_012_copyslots_") as raw_root:
        root = Path(raw_root)
        canon = root / "canon"
        peer = root / "peer"

        _write_text(canon / "a.txt", "one")
        _write_text(canon / "b.txt", "two")
        _write_text(canon / "c.txt", "three")
        peer.mkdir(parents=True, exist_ok=True)

        result = _run_kitchensync(
            [
                "--dry-run",
                "--verbosity",
                "trace",
                "--max-copies",
                "1",
                f"+{_file_url(canon)}",
                str(peer),
            ],
            cwd=root,
        )
        _fail(
            failures,
            result is not None,
            "012.11/012.12/012.13/012.10: dry-run invocation timed out or failed to start",
        )
        if result is None:
            return
        _fail(
            failures,
            result.returncode == 0,
            f"012.11/012.10/012.13: dry-run expected exit 0, got {result.returncode}. stdout={result.stdout!r} stderr={result.stderr!r}",
        )

        combined = result.stdout + "\n" + result.stderr
        matches = [tuple(map(int, match)) for match in COPY_SLOT_RE.findall(combined)]
        _fail(
            failures,
            matches,
            "012.11: trace output did not include copy-slot events, so max-copy enforcement could not be observed",
        )
        _fail(
            failures,
            any(active > 0 for active, _ in matches),
            "012.10: no active copy-slot events observed; expected dry-run transfer queuing",
        )
        _fail(
            failures,
            all(active <= 1 for active, _ in matches),
            "012.11: active dry-run copy slots exceeded configured --max-copies=1",
        )

        _fail(
            failures,
            all(maximum == 1 for _, maximum in matches),
            "012.11: copy slot denominator in trace output should reflect --max-copies=1",
        )
        _fail(
            failures,
            not (peer / "a.txt").exists(),
            "012.16: destination file written despite dry-run",
        )
        _fail(
            failures,
            not (peer / "b.txt").exists(),
            "012.16: destination file written despite dry-run",
        )
        _fail(
            failures,
            "dry run" in result.stdout.lower(),
            "012.23: stdout must include phrase 'dry run'",
        )


def check_snapshot_not_uploaded_and_recovery_markers_preserved(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_012_snapshot_") as raw_root:
        root = Path(raw_root)
        canon = root / "canon"
        peer = root / "peer"

        _write_text(canon / "seed.txt", "seed")
        normal = _run_kitchensync([f"+{_file_url(canon)}", str(peer)], cwd=root)
        _fail(
            failures,
            normal is not None,
            "012.7: initial normal run timed out or failed to start",
        )
        if normal is None:
            return
        _fail(
            failures,
            normal.returncode == 0,
            f"012.7: initial normal run expected exit 0, got {normal.returncode}. stdout={normal.stdout!r} stderr={normal.stderr!r}",
        )
        _fail(
            failures,
            (peer / "seed.txt").is_file(),
            "012.14/012.16: normal baseline run did not create destination file",
        )

        snapshot = _snapshot_path(peer)
        before_mtime = _snapshot_mtime(peer)
        _fail(failures, before_mtime is not None, f"012.22: peer live snapshot missing at {snapshot}")
        if before_mtime is None:
            return

        swap_dir = peer / ".kitchensync" / "SWAP" / SNAPSHOT_NAME
        swap_dir.mkdir(parents=True, exist_ok=True)
        _copy_file(snapshot, swap_dir / "old")
        _copy_file(snapshot, swap_dir / "new")
        before_old = _read_bytes(swap_dir / "old")
        before_new = _read_bytes(swap_dir / "new")

        _write_text(canon / "later.txt", "later")

        with tempfile.TemporaryDirectory(prefix="ks_012_tmp_snapshot_") as raw_temp_root:
            local_snapshot_root = Path(raw_temp_root)
            local_before = _local_temp_snapshot_paths(local_snapshot_root)
            local_new_count = 0
            local_snapshot_bytes: bytes | None = None
            dry = _run_kitchensync(
                [
                    "--dry-run",
                    "--retries-copy",
                    "2",
                    f"+{_file_url(canon)}",
                    str(peer),
                ],
                cwd=root,
                env=_temp_env(local_snapshot_root),
            )
            _fail(
                failures,
                dry is not None,
                "012.3/012.4/012.7: dry-run with existing snapshot timed out or failed to start",
            )
            if dry is None:
                return
            _fail(
                failures,
                dry.returncode == 0,
                f"012.3/012.4/012.7: dry-run expected exit 0, got {dry.returncode}. stdout={dry.stdout!r} stderr={dry.stderr!r}",
            )

            after_mtime = _snapshot_mtime(peer)
            local_after = _local_temp_snapshot_paths(local_snapshot_root)
            local_new = sorted(local_after - local_before, key=lambda path: path.as_posix())
            local_new_count = len(local_new)
        _fail(
            failures,
            local_new_count == 2,
            f"012.3/012.4: expected one local temp snapshot per reachable peer, got {local_new_count}",
        )
        _fail(
            failures,
            after_mtime == before_mtime,
            "012.7: live peer snapshot mtime changed during dry-run, indicating upload",
        )
        _fail(
            failures,
            _read_bytes(swap_dir / "old") == before_old,
            "012.20/012.3: SWAP/snapshot.db/old was modified during dry-run startup",
        )
        _fail(
            failures,
            _read_bytes(swap_dir / "new") == before_new,
            "012.3: SWAP/snapshot.db/new was modified during dry-run startup",
        )
        _fail(
            failures,
            not (peer / "later.txt").exists(),
            "012.16: destination wrote copied file during dry-run",
        )


def check_existing_snapshot_downloaded_as_is(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_012_existing_snapshot_") as raw_root:
        root = Path(raw_root)
        canon = root / "canon"
        peer = root / "peer"

        _write_text(canon / "seed.txt", "seed")
        normal = _run_kitchensync([f"+{_file_url(canon)}", str(peer)], cwd=root)
        _fail(
            failures,
            normal is not None,
            "012.4: baseline non-dry run timed out or failed to start",
        )
        if normal is None:
            return
        _fail(
            failures,
            normal.returncode == 0,
            f"012.4: baseline non-dry run expected exit 0, got {normal.returncode}. stdout={normal.stdout!r} stderr={normal.stderr!r}",
        )
        _fail(
            failures,
            (peer / "seed.txt").is_file(),
            "012.4: baseline sync did not reach peer",
        )

        live_snapshot = _snapshot_path(peer)
        live_snapshot_rows = _snapshot_row_count(live_snapshot)
        _fail(
            failures,
            live_snapshot_rows is not None,
            f"012.4: expected readable peer snapshot at {live_snapshot}",
        )
        if live_snapshot_rows is None:
            return

        with tempfile.TemporaryDirectory(prefix="ks_012_tmp_snapshot_") as raw_temp_root:
            local_snapshot_root = Path(raw_temp_root)
            local_before = _local_temp_snapshot_paths(local_snapshot_root)
            dry = _run_kitchensync(
                ["--dry-run", f"+{_file_url(canon)}", str(peer)],
                cwd=root,
                env=_temp_env(local_snapshot_root),
            )
            _fail(
                failures,
                dry is not None,
                "012.4: dry-run with existing snapshot timed out or failed to start",
            )
            if dry is None:
                return
            _fail(
                failures,
                dry.returncode == 0,
                f"012.4: dry-run expected exit 0, got {dry.returncode}. stdout={dry.stdout!r} stderr={dry.stderr!r}",
            )

            local_after = _local_temp_snapshot_paths(local_snapshot_root)
            downloaded = sorted(local_after - local_before, key=lambda path: path.as_posix())
            downloaded_row_counts = [_snapshot_row_count(path) for path in downloaded]

        _fail(
            failures,
            downloaded,
            "012.4: dry-run did not create a local temporary snapshot for a peer with an existing snapshot",
        )
        _fail(
            failures,
            any(row_count == live_snapshot_rows for row_count in downloaded_row_counts),
            "012.4: reachable peer snapshot did not include a local temporary snapshot matching live snapshot row count",
        )


def check_missing_snapshot_generates_no_live_snapshot_in_dry_run(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_012_missing_snapshot_") as raw_root:
        root = Path(raw_root)
        canon = root / "canon"
        peer = root / "peer"

        _write_text(canon / "seed.txt", "seed")
        peer.mkdir(parents=True, exist_ok=True)

        with tempfile.TemporaryDirectory(prefix="ks_012_tmp_snapshot_") as raw_temp_root:
            local_snapshot_root = Path(raw_temp_root)
            local_before = _local_temp_snapshot_paths(local_snapshot_root)
            local_new_count = 0
            local_row_counts: list[int | None] = []
            result = _run_kitchensync(
                ["--dry-run", f"+{_file_url(canon)}", str(peer)],
                cwd=root,
                env=_temp_env(local_snapshot_root),
            )
            _fail(
                failures,
                result is not None,
                "012.5: dry-run invocation timed out or failed to start",
            )
            if result is None:
                return
            _fail(
                failures,
                result.returncode == 0,
                f"012.5: dry-run expected exit 0, got {result.returncode}. stdout={result.stdout!r} stderr={result.stderr!r}",
            )

            local_after = _local_temp_snapshot_paths(local_snapshot_root)
            local_new = sorted(local_after - local_before, key=lambda path: path.as_posix())
            local_new_count = len(local_new)
            local_row_counts = [_snapshot_row_count(path) for path in local_new]

        _fail(
            failures,
            local_new_count == 2,
            f"012.5: expected one local temp snapshot per reachable peer with missing live snapshot, got {local_new_count}",
        )
        if local_new_count == 2:
            _fail(
                failures,
                all(row_count is not None for row_count in local_row_counts),
                "012.5: a local temp snapshot for a missing peer was not a readable snapshot database",
            )
        _fail(
            failures,
            not _snapshot_path(peer).is_file(),
            "012.5: missing live peer snapshot should create local temp only, not live snapshot.db",
        )


def main() -> int:
    failures: list[str] = []

    _fail(
        failures,
        WORKSPACE_ROOT.is_dir(),
        f"precondition: workspace root missing at {WORKSPACE_ROOT}",
    )
    _fail(
        failures,
        PROJECT_DIR.is_dir(),
        f"precondition: project directory missing at {PROJECT_DIR}",
    )
    _fail(
        failures,
        RELEASED_EXE.is_file(),
        f"precondition: released executable missing at {RELEASED_EXE}",
    )

    _run_case(
        "012.1/012.2",
        failures,
        lambda: check_root_missing_is_unreachable_and_not_created(failures),
    )
    _run_case(
        "012.1/012.8",
        failures,
        lambda: check_missing_peer_is_skipped_and_other_peer_still_syncs_without_mutation(failures),
    )
    _run_case(
        "012.14-012.22",
        failures,
        lambda: check_no_peer_mutation_or_cleanup_on_file_peers(failures),
    )
    _run_case(
        "012.10/012.11/012.23",
        failures,
        lambda: check_copy_slots_are_limited_during_dry_run(failures),
    )
    _run_case(
        "012.3/012.7",
        failures,
        lambda: check_snapshot_not_uploaded_and_recovery_markers_preserved(failures),
    )
    _run_case(
        "012.5",
        failures,
        lambda: check_missing_snapshot_generates_no_live_snapshot_in_dry_run(failures),
    )
    _run_case(
        "012.4",
        failures,
        lambda: check_existing_snapshot_downloaded_as_is(failures),
    )

    # 012.6 -- local temporary snapshots are updated during traversal.
    # not reasonably testable from this CLI-only surface without per-operation instrumentation.
    # 012.8 -- connect-to-peer ordering is observable only through SFTP-capable scenarios in this CLI setup.
    # 012.9 -- directory-listing behavior during combined-tree walk.
    # not reasonably testable from process output alone in this environment.
    # 012.12 -- reading source file contents for queued copy work.
    # cannot be observed directly with file peers without brittle instrumentation.
    # 012.13 -- retry behavior on failed dry-run copy tries.
    # stable deterministic failure injection for copy retries on local peers is not available here.

    if failures:
        print("FAIL: test_012_dry_run_mode.py")
        for index, failure in enumerate(failures, start=1):
            print(f"  {index:02d}. {failure}")
        return 1

    print("PASS: test_012_dry_run_mode.py")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
