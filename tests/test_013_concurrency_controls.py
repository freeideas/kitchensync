#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# ///

"""End-to-end verification for reqs/013_concurrency-controls.md."""

from __future__ import annotations

import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path


sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

WORKSPACE_ROOT = Path(r"C:\\Users\\human\\Desktop\\prjx\\kitchensync")
PROJECT_DIR = Path(r"C:\\Users\\human\\Desktop\\prjx\\kitchensync\\proj")
WINDOWS_EXE_PATH = Path(r"C:\\Users\\human\\Desktop\\prjx\\kitchensync\\released\\kitchensync.exe")
POSIX_EXE_PATH = Path(r"C:\\Users\\human\\Desktop\\prjx\\kitchensync\\released\\kitchensync")
RELEASED_EXE_PATH = WINDOWS_EXE_PATH if os.name == "nt" else POSIX_EXE_PATH

_SLOT_EVENT_RE = re.compile(r"copy-slots active=(\d+)/(\d+)")


def _add_failure(failures: list[str], condition: bool, message: str) -> None:
    if not condition:
        failures.append(message)


def _run_kitchensync(
    args: list[str],
    *,
    cwd: Path,
    timeout_seconds: float = 60.0,
) -> subprocess.CompletedProcess[str]:
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
            stderr="command timed out",
        )
    except FileNotFoundError as exc:
        return subprocess.CompletedProcess(
            args=command,
            returncode=127,
            stdout="",
            stderr=f"failed to launch released executable: {exc}",
        )


def _seed_file(path: Path, payload: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(payload)


def _payload(size_bytes: int, marker: str) -> bytes:
    marker_bytes = marker.encode("utf-8")
    repeat = max(1, (size_bytes + len(marker_bytes) - 1) // len(marker_bytes))
    data = marker_bytes * repeat
    return data[:size_bytes]


def _read_if_exists(path: Path) -> bytes | None:
    try:
        return path.read_bytes()
    except (FileNotFoundError, OSError):
        return None


def _build_bulk_payload_tree(peer_root: Path, prefix: str, count: int, size_bytes: int) -> list[Path]:
    files: list[Path] = []
    for index in range(1, count + 1):
        rel = Path("bulk") / f"{prefix}_{index:03d}.bin"
        _seed_file(peer_root / rel, _payload(size_bytes, f"{prefix}-payload-{index}"))
        files.append(rel)
    return files


def _copy_slot_events(output: str) -> list[tuple[int, int]]:
    return [(int(active), int(limit)) for active, limit in _SLOT_EVENT_RE.findall(output)]


def _slot_start_count(events: list[tuple[int, int]]) -> int:
    starts = 0
    prior_active = 0
    for active, _ in events:
        if active > 0 and prior_active == 0:
            starts += 1
        prior_active = active
    return starts


def _assert_file_matches(
    failures: list[str],
    req_id: str,
    source_root: Path,
    dest_root: Path,
    relative_paths: list[Path],
) -> None:
    for rel in relative_paths:
        source_path = source_root / rel
        dest_path = dest_root / rel
        source_data = _read_if_exists(source_path)
        dest_data = _read_if_exists(dest_path)

        _add_failure(failures, source_data is not None, f"{req_id}: missing source fixture path {source_path}")
        _add_failure(failures, dest_data is not None, f"{req_id}: expected destination path {dest_path} to exist")
        if source_data is None or dest_data is None:
            continue
        _add_failure(
            failures,
            source_data == dest_data,
            f"{req_id}: destination file {dest_path} did not match source fixture",
        )


def _case_default_and_configured_max_copies(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_013_default_and_configured_") as raw_root:
        root = Path(raw_root)

        # Default max-copies (implicit)
        canon = root / "canon-default"
        sink = root / "sink-default"
        relative_files = _build_bulk_payload_tree(canon, "item", 24, 1_048_576)

        default = _run_kitchensync(
            ["--verbosity", "trace", f"+{canon}", str(sink)],
            cwd=root,
            timeout_seconds=120.0,
        )
        _add_failure(
            failures,
            default.returncode == 0,
            f"013.1/013.9: default max-copies sync exited {default.returncode}. stdout={default.stdout!r} stderr={default.stderr!r}",
        )
        _add_failure(failures, not default.stderr.strip(), f"013.2/014.2: expected empty stderr, got {default.stderr!r}")

        default_events = _copy_slot_events(f"{default.stdout}\n{default.stderr}")
        _add_failure(failures, bool(default_events), "013.1/013.2: expected copy-slot trace events for default run")
        if default_events:
            default_max = max(active for active, _ in default_events)
            _add_failure(failures, default_max <= 10, f"013.1: default max-copies exceeded 10 with active={default_max}")
            _add_failure(failures, default_max >= 2, "013.1: default run showed no concurrent active copy slots")
        _assert_file_matches(failures, "013.1/013.9", canon, sink, relative_files)

        # Configured max-copies
        configured_canon = root / "canon-configured"
        configured_sink = root / "sink-configured"
        configured_files = _build_bulk_payload_tree(configured_canon, "item", 24, 1_048_576)

        configured = _run_kitchensync(
            ["--verbosity", "trace", "--max-copies", "2", f"+{configured_canon}", str(configured_sink)],
            cwd=root,
            timeout_seconds=120.0,
        )
        _add_failure(
            failures,
            configured.returncode == 0,
            f"013.2: configured --max-copies 2 run exited {configured.returncode}. stdout={configured.stdout!r} stderr={configured.stderr!r}",
        )
        _add_failure(failures, not configured.stderr.strip(), f"013.2/014.2: expected empty stderr, got {configured.stderr!r}")

        configured_events = _copy_slot_events(f"{configured.stdout}\n{configured.stderr}")
        _add_failure(failures, bool(configured_events), "013.2: expected copy-slot trace events for configured max run")
        if configured_events:
            configured_max = max(active for active, _ in configured_events)
            _add_failure(failures, configured_max <= 2, f"013.2: --max-copies 2 cap exceeded with active={configured_max}")
            _add_failure(failures, configured_max == 2, "013.2: --max-copies 2 never reached two active slots")
        _assert_file_matches(failures, "013.9", configured_canon, configured_sink, configured_files)


def _case_retries_are_per_copy_and_honor_limits(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_013_retries_") as raw_root:
        root = Path(raw_root)
        canon = root / "canon"
        sink = root / "sink"

        file_a = canon / "alpha.bin"
        file_b = canon / "beta.bin"
        old_payload = _payload(64_000, "payload-v1")
        new_payload = _payload(64_000, "payload-v2")

        _seed_file(file_a, old_payload)
        _seed_file(file_b, old_payload)

        baseline = _run_kitchensync(
            ["--verbosity", "trace", f"+{canon}", str(sink)],
            cwd=root,
            timeout_seconds=60.0,
        )
        _add_failure(
            failures,
            baseline.returncode == 0,
            f"013.11/013.12/013.13/013.15: baseline sync failed. stdout={baseline.stdout!r} stderr={baseline.stderr!r}",
        )
        if baseline.returncode != 0:
            return

        _seed_file(file_a, new_payload)
        _seed_file(file_b, new_payload)

        # Force both transfers in this change set to fail.
        _seed_file(sink / ".kitchensync" / "SWAP" / "alpha.bin", b"blocked")
        _seed_file(sink / ".kitchensync" / "SWAP" / "beta.bin", b"blocked")

        retries_one = _run_kitchensync(
            [
                "--verbosity",
                "trace",
                "--max-copies",
                "1",
                "--retries-copy",
                "1",
                f"+{canon}",
                str(sink),
            ],
            cwd=root,
            timeout_seconds=90.0,
        )
        _add_failure(
            failures,
            retries_one.returncode == 0,
            f"013.12: --retries-copy 1 run exited {retries_one.returncode}. stdout={retries_one.stdout!r} stderr={retries_one.stderr!r}",
        )
        _add_failure(failures, not retries_one.stderr.strip(), f"013.12/014.2: expected empty stderr, got {retries_one.stderr!r}")
        _add_failure(
            failures,
            _slot_start_count(_copy_slot_events(f"{retries_one.stdout}\n{retries_one.stderr}")) == 2,
            "013.12: --retries-copy 1 should schedule one try per queued file in this run",
        )

        _add_failure(failures, _read_if_exists(sink / "alpha.bin") == old_payload, "013.12/013.15: alpha should remain at old payload after exhausting retries")
        _add_failure(failures, _read_if_exists(sink / "beta.bin") == old_payload, "013.12/013.15: beta should remain at old payload after exhausting retries")

        retries_two = _run_kitchensync(
            [
                "--verbosity",
                "trace",
                "--max-copies",
                "1",
                "--retries-copy",
                "2",
                f"+{canon}",
                str(sink),
            ],
            cwd=root,
            timeout_seconds=90.0,
        )
        _add_failure(
            failures,
            retries_two.returncode == 0,
            f"013.11/013.13/013.15: --retries-copy 2 run exited {retries_two.returncode}. stdout={retries_two.stdout!r} stderr={retries_two.stderr!r}",
        )
        _add_failure(failures, not retries_two.stderr.strip(), f"013.13/014.2: expected empty stderr, got {retries_two.stderr!r}")
        _add_failure(
            failures,
            _slot_start_count(_copy_slot_events(f"{retries_two.stdout}\n{retries_two.stderr}")) == 4,
            "013.11/013.13/013.15: with two queued copies and --retries-copy 2, expected 4 total copy-slot start events",
        )

        _add_failure(failures, _read_if_exists(sink / "alpha.bin") == old_payload, "013.15: alpha should remain at old payload after limit exhaustion")
        _add_failure(failures, _read_if_exists(sink / "beta.bin") == old_payload, "013.15: beta should remain at old payload after limit exhaustion")


# not reasonably testable: 013.3 -- requires mixed local and SFTP peers to confirm copy counting is scheme-agnostic.
# not reasonably testable: 013.4 -- requires distinguishing file-copy work from listing/upload/cleanup work using internal accounting details.
# not reasonably testable: 013.5 -- requires proving non-copy concurrent work never expands active file-copy count.
# not reasonably testable: 013.6 -- no explicit per-peer/per-host/per-connection transfer-limit control is exposed in this binary output surface.
# not reasonably testable: 013.7 -- requires internal scheduling/timing between traversal and transfer starts.
# not reasonably testable: 013.8 -- queue insertion timing during traversal is internal and not separately observable.
# not reasonably testable: 013.10 -- inline-displacement and directory-creation timing are not surfaced in release output.
# not reasonably testable: 013.14 -- queue back-rotation on retry is not reported in release output.
# not reasonably testable: 013.16 -- requires mixed-scheme copy execution.


def main() -> int:
    failures: list[str] = []

    _add_failure(failures, RELEASED_EXE_PATH.is_file(), f"precondition: released executable missing at {RELEASED_EXE_PATH}")
    if failures:
        print("FAIL: test_013_concurrency_controls.py")
        for index, item in enumerate(failures, start=1):
            print(f"  {index:02d}. {item}")
        return 1

    _case_default_and_configured_max_copies(failures)
    _case_retries_are_per_copy_and_honor_limits(failures)

    if failures:
        print("FAIL: test_013_concurrency_controls.py")
        for index, item in enumerate(failures, start=1):
            print(f"  {index:02d}. {item}")
        return 1

    print("PASS: test_013_concurrency_controls.py")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
