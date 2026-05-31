#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# ///

"""End-to-end verification for reqs/010_file-transfer-safety.md."""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import tempfile
from datetime import datetime, timezone
from pathlib import Path
from typing import Callable
from urllib.parse import quote

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

WORKSPACE_ROOT = Path(r"C:\\Users\\human\\Desktop\\prjx\\kitchensync")
PROJECT_DIR = Path(r"C:\\Users\\human\\Desktop\\prjx\\kitchensync\\proj")
WINDOWS_EXE_PATH = Path(r"C:\\Users\\human\\Desktop\\prjx\\kitchensync\\released\\kitchensync.exe")
POSIX_EXE_PATH = Path(r"C:\\Users\\human\\Desktop\\prjx\\kitchensync\\released\\kitchensync")
RELEASED_EXE_PATH = WINDOWS_EXE_PATH if os.name == "nt" else POSIX_EXE_PATH



def _fail(failures: list[str], condition: bool, message: str) -> None:
    if not condition:
        failures.append(message)



def _run_kitchensync(
    args: list[str],
    *,
    cwd: Path,
    timeout_seconds: float = 30.0,
) -> subprocess.CompletedProcess[str] | None:
    cmd = [str(RELEASED_EXE_PATH), *args]
    try:
        return subprocess.run(
            cmd,
            cwd=str(cwd),
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout_seconds,
            check=False,
        )
    except subprocess.TimeoutExpired:
        return None
    except (FileNotFoundError, OSError) as exc:
        return subprocess.CompletedProcess(
            args=cmd,
            returncode=127,
            stdout="",
            stderr=f"failed to launch released executable: {exc}",
        )



def _write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def _replace_with_text_file(path: Path, text: str) -> None:
    if path.is_dir():
        shutil.rmtree(path)
    elif path.exists():
        path.unlink()
    _write_text(path, text)



def _read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")



def _file_url(path: Path) -> str:
    return path.resolve().as_uri()



def _set_mtime(path: Path, when: datetime) -> None:
    if when.tzinfo is None:
        when = when.replace(tzinfo=timezone.utc)
    timestamp = when.timestamp()
    os.utime(path, (timestamp, timestamp), follow_symlinks=True)



def _assert_exit_code_zero(
    failures: list[str],
    req: str,
    result: subprocess.CompletedProcess[str] | None,
    command: list[str],
) -> None:
    _fail(
        failures,
        result is not None,
        f"{req}: kitchensync call timed out for {command!r}",
    )
    if result is None:
        return
    _fail(
        failures,
        result.returncode == 0,
        f"{req}: command {command!r} expected exit 0, got {result.returncode}. stdout={result.stdout!r} stderr={result.stderr!r}",
    )



def _assert_stderr_empty(
    failures: list[str],
    req: str,
    result: subprocess.CompletedProcess[str],
    command: list[str],
) -> None:
    _fail(
        failures,
        not result.stderr.strip(),
        f"{req}: expected empty stderr for {command!r}, got {result.stderr!r}",
    )



def _assert_output_contains_any(
    failures: list[str],
    req: str,
    output: str,
    tokens: list[str],
) -> None:
    lower = output.lower()
    _fail(
        failures,
        any(token in lower for token in tokens),
        f"{req}: expected one of {tokens} in output, got {output!r}",
    )



def _require_path_absent(failures: list[str], req: str, path: Path, detail: str) -> None:
    _fail(failures, not path.exists(), f"{req}: {detail}: {path}")



def _swap_dir(peer_root: Path, filename: str) -> Path:
    return peer_root / ".kitchensync" / "SWAP" / quote(filename, safe="")



def _backup_entries(peer_root: Path, filename: str) -> list[Path]:
    bak_root = peer_root / ".kitchensync" / "BAK"
    if not bak_root.exists():
        return []
    return [p for p in bak_root.rglob("*") if p.is_file() and p.name == filename]



def _assert_mtime_matches(
    failures: list[str],
    req: str,
    path: Path,
    expected: datetime,
    tolerance_seconds: float = 2.0,
) -> None:
    actual = datetime.fromtimestamp(path.stat().st_mtime, tz=timezone.utc)
    diff = abs((actual - expected).total_seconds())
    _fail(
        failures,
        diff <= tolerance_seconds,
        f"{req}: expected mtime near {expected.isoformat()}, got {actual.isoformat()}",
    )



def _run_case(label: str, failures: list[str], fn: Callable[[], None]) -> None:
    try:
        fn()
    except AssertionError as exc:
        failures.append(f"{label}: {exc}")
    except Exception as exc:  # noqa: BLE001
        failures.append(f"{label}: unexpected exception {exc!r}")



def check_replacement_uses_swap_and_archives_old(failures: list[str]) -> None:
    ctx = tempfile.TemporaryDirectory(prefix="ks_010_replacement_")
    try:
        root = Path(ctx.name)
        canon = root / "canon"
        sink = root / "sink"
        source = canon / "payload.txt"
        destination = sink / "payload.txt"

        _write_text(source, "first-version")
        _set_mtime(source, datetime(2025, 11, 1, 10, 15, 0, tzinfo=timezone.utc))

        initial = _run_kitchensync(
            [f"+{_file_url(canon)}", _file_url(sink)],
            cwd=root,
        )
        _assert_exit_code_zero(failures, "010.1/010.2/010.3/010.4/010.6/010.7/010.10", initial, [f"+{_file_url(canon)}", _file_url(sink)])
        if initial is None:
            return
        _assert_stderr_empty(failures, "010.1/010.2/010.3/010.4/010.6/010.7/010.10", initial, [f"+{_file_url(canon)}", _file_url(sink)])
        _fail(
            failures,
            destination.is_file() and _read_text(destination) == "first-version",
            "010.1/010.2/010.3/010.4/010.6/010.7/010.10: initial payload did not copy from canon",
        )

        _write_text(source, "second-version")
        winning_time = datetime(2026, 1, 2, 12, 0, 0, tzinfo=timezone.utc)
        _set_mtime(source, winning_time)

        replacement = _run_kitchensync(
            [f"+{_file_url(canon)}", _file_url(sink)],
            cwd=root,
        )
        _assert_exit_code_zero(
            failures,
            "010.1/010.2/010.3/010.4/010.6/010.7/010.10",
            replacement,
            [f"+{_file_url(canon)}", _file_url(sink)],
        )
        if replacement is None:
            return
        _assert_stderr_empty(
            failures,
            "010.1/010.2/010.3/010.4/010.6/010.7/010.10",
            replacement,
            [f"+{_file_url(canon)}", _file_url(sink)],
        )

        _fail(
            failures,
            destination.read_text(encoding="utf-8") == "second-version",
            "010.1/010.2/010.4/010.10: destination was not replaced with new source content",
        )
        _assert_mtime_matches(
            failures,
            "010.4/010.5",
            destination,
            winning_time,
            tolerance_seconds=2.0,
        )

        backups = _backup_entries(sink, destination.name)
        _fail(
            failures,
            len(backups) >= 1,
            "010.6: replacement did not archive prior destination content to BAK",
        )
        if backups:
            old_content_found = any(_read_text(entry) == "first-version" for entry in backups)
            _fail(
                failures,
                old_content_found,
                "010.6: BAK entry did not contain displaced source content",
            )

        swap_base = _swap_dir(sink, destination.name)
        _require_path_absent(
            failures,
            "010.7",
            swap_base,
            "swap staging directory for replaced file was not cleaned after success",
        )
    finally:
        ctx.cleanup()



def check_transfer_failure_before_old_keeps_destination(failures: list[str]) -> None:
    ctx = tempfile.TemporaryDirectory(prefix="ks_010_staging_fail_")
    try:
        root = Path(ctx.name)
        canon = root / "canon"
        sink = root / "sink"
        source = canon / "payload.txt"
        destination = sink / "payload.txt"

        _write_text(source, "first-version")
        _set_mtime(source, datetime(2025, 11, 1, 10, 15, 0, tzinfo=timezone.utc))

        initial = _run_kitchensync(
            [f"+{_file_url(canon)}", _file_url(sink)],
            cwd=root,
        )
        _assert_exit_code_zero(failures, "010.11/010.12/010.13/010.14/010.15/010.16/010.21", initial, [f"+{_file_url(canon)}", _file_url(sink)])
        if initial is None:
            return

        swap_block = _swap_dir(sink, destination.name)
        _replace_with_text_file(swap_block, "BLOCKED SWAP")
        _write_text(source, "second-version")
        _set_mtime(source, datetime(2026, 1, 2, 12, 30, 0, tzinfo=timezone.utc))

        failure = _run_kitchensync(
            ["--retries-copy", "1", f"+{_file_url(canon)}", _file_url(sink)],
            cwd=root,
        )
        _fail(
            failures,
            failure is not None,
            "010.11/010.12/010.13/010.14/010.15/010.16/010.21: sync timed out during transfer failure case",
        )
        if failure is None:
            return
        _assert_output_contains_any(
            failures,
            "010.15",
            failure.stdout + "\n" + failure.stderr,
            ["transfer failed", "write_swap_new", "move_existing_to_swap_old", "cleanup", "archive_old"],
        )
        _fail(
            failures,
            destination.read_text(encoding="utf-8") == "first-version",
            "010.11/010.13: destination changed despite staging failure before old destination could be moved",
        )
        _fail(
            failures,
            swap_block.is_file(),
            "010.12/010.14: staging did not preserve transfer block file as expected",
        )
        _fail(
            failures,
            _read_text(swap_block) == "BLOCKED SWAP",
            "010.12/010.14: transfer block file content changed during transfer failure scenario",
        )
        _fail(
            failures,
            not (swap_block / "new").exists(),
            "010.14: transfer staging files were not cleaned for failed pre-old stage",
        )
    finally:
        ctx.cleanup()



def check_archive_failure_after_replace_leaves_swap_old(failures: list[str]) -> None:
    ctx = tempfile.TemporaryDirectory(prefix="ks_010_archive_fail_")
    try:
        root = Path(ctx.name)
        canon = root / "canon"
        sink = root / "sink"
        source = canon / "payload.txt"
        destination = sink / "payload.txt"

        _write_text(source, "first-version")
        _set_mtime(source, datetime(2025, 11, 1, 10, 15, 0, tzinfo=timezone.utc))

        initial = _run_kitchensync(
            [f"+{_file_url(canon)}", _file_url(sink)],
            cwd=root,
        )
        _assert_exit_code_zero(
            failures,
            "010.17/010.18/010.19/010.20",
            initial,
            [f"+{_file_url(canon)}", _file_url(sink)],
        )
        if initial is None:
            return

        bak_block = sink / ".kitchensync" / "BAK"
        _write_text(bak_block, "BLOCKED BAK")

        _write_text(source, "second-version")
        winning_time = datetime(2026, 1, 2, 13, 0, 0, tzinfo=timezone.utc)
        _set_mtime(source, winning_time)

        blocked = _run_kitchensync(
            ["--retries-copy", "1", f"+{_file_url(canon)}", _file_url(sink)],
            cwd=root,
        )
        _fail(
            failures,
            blocked is not None,
            "010.17/010.18/010.19/010.20: sync timed out while forcing archive failure",
        )
        if blocked is None:
            return

        _assert_output_contains_any(
            failures,
            "010.18/010.20",
            blocked.stdout + "\n" + blocked.stderr,
            ["archive_old", "archive", "transfer failed", "error"],
        )

        _fail(
            failures,
            destination.read_text(encoding="utf-8") == "second-version",
            "010.17/010.19: destination was not updated after archive-stage failure",
        )
        _assert_mtime_matches(
            failures,
            "010.17/010.19",
            destination,
            winning_time,
            tolerance_seconds=2.0,
        )

        swap_old = _swap_dir(sink, destination.name) / "old"
        _fail(
            failures,
            swap_old.is_file(),
            "010.19: SWAP old path was not left for recovery after archive failure",
        )
        if swap_old.is_file():
            _fail(
                failures,
                _read_text(swap_old) == "first-version",
                "010.19: SWAP old file did not preserve displaced destination content",
            )

        _fail(
            failures,
            not _backup_entries(sink, destination.name),
            "010.19: BAK entries should be absent when archive_old fails",
        )
        _fail(
            failures,
            bak_block.is_file(),
            "010.19/010.20: BAK blocker file was removed when archive step failed",
        )
    finally:
        ctx.cleanup()


# not reasonably testable from this CLI-only release-surface test: 010.5 -- requires measuring internal re-read of source mod_time during active transfer window
# not reasonably testable from this CLI-only release-surface test: 010.8 -- requires allocator/streaming telemetry to prove bounded buffering
# not reasonably testable from this CLI-only release-surface test: 010.9 -- requires timing telemetry to show streaming begins before full file buffering completes
# not reasonably testable from this CLI-only release-surface test: 010.22 -- requires forcing set_mod_time to fail after a successful copy in a portable way
# not reasonably testable from this CLI-only release-surface test: 010.23 -- requires deterministic, portable log and failure injection for set_mod_time failure case
# not reasonably testable from this CLI-only release-surface test: 010.10 -- requires an in-process SFTP transport to compare file:// behavior against non-file transfer


def main() -> int:
    failures: list[str] = []
    _fail(failures, RELEASED_EXE_PATH.is_file(), f"precondition: released executable missing at {RELEASED_EXE_PATH}")

    _run_case(
        "010.1/010.2/010.3/010.4/010.6/010.7/010.10",
        failures,
        lambda: check_replacement_uses_swap_and_archives_old(failures),
    )

    _run_case(
        "010.11/010.12/010.13/010.14/010.15/010.16/010.21",
        failures,
        lambda: check_transfer_failure_before_old_keeps_destination(failures),
    )

    _run_case(
        "010.17/010.18/010.19/010.20",
        failures,
        lambda: check_archive_failure_after_replace_leaves_swap_old(failures),
    )

    if failures:
        print("FAIL: test_010_file_transfer_safety.py")
        for index, item in enumerate(failures, start=1):
            print(f"  {index:02d}. {item}")
        return 1

    print("PASS: test_010_file_transfer_safety.py")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
