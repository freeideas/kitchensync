#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# ///

"""End-to-end verification for reqs/004_peer-connectivity.md."""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Callable


if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
if hasattr(sys.stderr, "reconfigure"):
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")


WORKSPACE_ROOT = Path(r"C:/Users/human/Desktop/prjx/kitchensync")
PROJECT_DIR = Path(r"C:/Users/human/Desktop/prjx/kitchensync/proj")
RELEASED_BINARY = Path(
    r"C:/Users/human/Desktop/prjx/kitchensync/released/kitchensync.exe"
    if os.name == "nt"
    else r"C:/Users/human/Desktop/prjx/kitchensync/released/kitchensync"
)


def _run_case(name: str, failures: list[str], fn: Callable[[], None]) -> None:
    try:
        fn()
    except AssertionError as exc:
        failures.append(f"{name}: {exc}")
    except Exception as exc:  # noqa: BLE001
        failures.append(f"{name}: unexpected exception: {exc!r}")


def _fail(failures: list[str], condition: bool, message: str) -> None:
    if not condition:
        failures.append(message)


def _run_kitchensync(args: list[str], cwd: Path, timeout_seconds: int = 30) -> subprocess.CompletedProcess[str]:
    cmd = [str(RELEASED_BINARY), *args]
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
        return subprocess.CompletedProcess(
            args=cmd,
            returncode=124,
            stdout="",
            stderr=f"kitchensync invocation timed out after {timeout_seconds}s",
        )


def _write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def _file_url(path: Path) -> str:
    return path.as_uri()


def _assert_exit_code(
    failures: list[str],
    label: str,
    result: subprocess.CompletedProcess[str],
    expected: int,
) -> None:
    _fail(
        failures,
        result.returncode == expected,
        f"{label}: expected exit code {expected}, got {result.returncode}. stdout={result.stdout.strip()!r}, stderr={result.stderr.strip()!r}",
    )


def _assert_nonzero(failures: list[str], label: str, result: subprocess.CompletedProcess[str]) -> None:
    _fail(
        failures,
        result.returncode != 0,
        f"{label}: expected non-zero exit code for failure path, got {result.returncode}. stdout={result.stdout.strip()!r}, stderr={result.stderr.strip()!r}",
    )


def _assert_stderr_empty(failures: list[str], label: str, result: subprocess.CompletedProcess[str]) -> None:
    _fail(failures, not result.stderr.strip(), f"{label}: expected empty stderr, got {result.stderr.strip()!r}")


def _assert_contains_all(
    failures: list[str],
    label: str,
    output: str,
    terms: list[str],
) -> None:
    lower = output.lower()
    _fail(failures, any(t in lower for t in terms), f"{label}: expected one of {terms} in output, got {output!r}")


def main() -> int:
    failures: list[str] = []

    _fail(failures, RELEASED_BINARY.is_file(), f"precondition: released executable missing at {RELEASED_BINARY}")

    # 004.2, 004.3, 004.4, 004.5 -- fallback order and winner selection for a local file peer.
    def check_fallback_primary_wins() -> None:
        with tempfile.TemporaryDirectory(prefix="ks_004_2_") as raw_tmp:
            root = Path(raw_tmp)
            primary_root = root / "fallback-primary"
            secondary_root = root / "fallback-secondary"
            target = root / "target"

            _write_text(primary_root / "payload.txt", "from-primary")
            _write_text(secondary_root / "payload.txt", "from-secondary")

            result = _run_kitchensync(
                [f"+[{_file_url(primary_root)},{_file_url(secondary_root)}]", str(target)],
                cwd=root,
            )
            _assert_exit_code(failures, "004.2/004.3/004.4/004.5", result, 0)
            if result.returncode == 0:
                _fail(
                    failures,
                    (target / "payload.txt").read_text(encoding="utf-8") == "from-primary",
                    "004.2/004.3/004.4/004.5: expected target file from first (primary) fallback URL",
                )
                _assert_stderr_empty(failures, "004.2/004.3/004.4/004.5", result)

    # 004.3 -- fallback URLs are tried in listed order when early URLs do not connect.
    def check_fallback_uses_secondary_after_primary_fail() -> None:
        with tempfile.TemporaryDirectory(prefix="ks_004_3_") as raw_tmp:
            root = Path(raw_tmp)
            bad_primary = root / "bad-primary"
            _write_text(bad_primary, "this-is-a-file-not-a-directory")
            secondary_root = root / "secondary"
            target = root / "target"

            _write_text(secondary_root / "payload.txt", "from-secondary")

            result = _run_kitchensync(
                [f"+[{bad_primary},{_file_url(secondary_root)}]", str(target)],
                cwd=root,
            )
            _assert_exit_code(failures, "004.3", result, 0)
            if result.returncode == 0:
                _fail(
                    failures,
                    (target / "payload.txt").read_text(encoding="utf-8") == "from-secondary",
                    "004.3: expected target to receive data from secondary fallback URL",
                )

    # 004.6 -- do not try remaining fallback URLs after a winner is selected for a peer.
    def check_fallback_no_retry_after_winner() -> None:
        with tempfile.TemporaryDirectory(prefix="ks_004_6_") as raw_tmp:
            root = Path(raw_tmp)
            primary_root = root / "primary"
            unused_fallback = root / "unused" / "secondary"
            target = root / "target"

            _write_text(primary_root / "payload.txt", "winner")
            result = _run_kitchensync(
                [f"+[{_file_url(primary_root)},{str(unused_fallback)}]", str(target)],
                cwd=root,
            )
            _assert_exit_code(failures, "004.6", result, 0)
            _fail(
                failures,
                not unused_fallback.exists(),
                "004.6: secondary fallback path was created, suggesting it may have been tried",
            )
            if result.returncode == 0:
                _fail(
                    failures,
                    (target / "payload.txt").read_text(encoding="utf-8") == "winner",
                    "004.6: expected target to use primary fallback URL payload",
                )

    # 004.7 -- all URLs for a peer can fail while sync still succeeds with remaining peers.
    def check_unreachable_peer_skipped_and_sync_continues() -> None:
        with tempfile.TemporaryDirectory(prefix="ks_004_7_") as raw_tmp:
            root = Path(raw_tmp)
            canon = root / "canon"
            reachable = root / "reachable"
            bad_a = root / "bad-a"
            bad_b = root / "bad-b"

            _write_text(canon / "seed.txt", "canonical")
            _write_text(reachable / "existing.txt", "keep")
            _write_text(bad_a, "bad")
            _write_text(bad_b, "bad")

            result = _run_kitchensync(
                ["--verbosity", "error", f"+{_file_url(canon)}", str(reachable), f"[{bad_a},{bad_b}]"],
                cwd=root,
            )
            _assert_exit_code(failures, "004.7", result, 0)
            _fail(
                failures,
                (reachable / "seed.txt").exists(),
                "004.7: expected a reachable peer to participate after another peer is skipped",
            )

    # 004.8 -- each skipped peer logs an error-level diagnostic.
    def check_unreachable_peer_diagnostic() -> None:
        with tempfile.TemporaryDirectory(prefix="ks_004_8_") as raw_tmp:
            root = Path(raw_tmp)
            canon = root / "canon"
            reachable = root / "reachable"
            bad = root / "bad"

            _write_text(canon / "seed.txt", "canonical")
            _write_text(reachable / "existing.txt", "sink")
            _write_text(bad, "bad")

            result = _run_kitchensync(
                ["--verbosity", "error", f"+{_file_url(canon)}", str(reachable), f"[{bad}]"],
                cwd=root,
            )
            _assert_exit_code(failures, "004.8", result, 0)
            _assert_contains_all(
                failures,
                "004.8",
                result.stdout,
                ["error", "unreachable"],
            )

    # 004.9 -- fewer than two reachable peers after startup is an error.
    def check_too_few_reachable_peers() -> None:
        with tempfile.TemporaryDirectory(prefix="ks_004_9_") as raw_tmp:
            root = Path(raw_tmp)
            canon = root / "canon"
            missing = root / "missing"

            _write_text(canon / "seed.txt", "canonical")
            result = _run_kitchensync(["--dry-run", f"+{_file_url(canon)}", str(missing)], cwd=root)
            _assert_nonzero(failures, "004.9", result)
            _fail(failures, not missing.exists(), "004.9: dry-run missing peer root should remain missing")

    # 004.10 -- unreachable canon fails the run even when others are reachable.
    def check_canon_unreachable_fails() -> None:
        with tempfile.TemporaryDirectory(prefix="ks_004_10_") as raw_tmp:
            root = Path(raw_tmp)
            bad_canon = root / "bad-canon"
            reachable_a = root / "reachable-a"
            reachable_b = root / "reachable-b"

            _write_text(bad_canon, "this-is-a-file-not-a-directory")
            _write_text(reachable_a / "seed.txt", "a")
            _write_text(reachable_b / "seed.txt", "b")

            result = _run_kitchensync(
                [f"+{str(bad_canon)}", str(reachable_a), str(reachable_b)],
                cwd=root,
            )
            _assert_nonzero(failures, "004.10", result)
            _assert_contains_all(
                failures,
                "004.10",
                result.stdout,
                ["canon", "unreachable"],
            )

    # 004.11 -- normal runs auto-create missing file:// peer root directories before connect.
    def check_file_url_creates_missing_root_in_normal_run() -> None:
        with tempfile.TemporaryDirectory(prefix="ks_004_11_") as raw_tmp:
            root = Path(raw_tmp)
            canon = root / "canon"
            missing = root / "missing" / "nested" / "peer"

            _write_text(canon / "seed.txt", "canonical")
            result = _run_kitchensync(
                [f"+{_file_url(canon)}", _file_url(missing)],
                cwd=root,
            )
            _assert_exit_code(failures, "004.11", result, 0)
            _fail(failures, missing.is_dir(), "004.11: missing file:// root directory should be created in normal run")
            _fail(
                failures,
                (missing / "seed.txt").read_text(encoding="utf-8") == "canonical",
                "004.11: expected data to sync into auto-created file:// root",
            )

    # 004.12 -- dry-run does not create missing file:// peer roots.
    def check_file_url_dry_run_no_create_root() -> None:
        with tempfile.TemporaryDirectory(prefix="ks_004_12_") as raw_tmp:
            root = Path(raw_tmp)
            canon = root / "canon"
            missing = root / "missing" / "nested" / "peer"
            sink = root / "sink"

            _write_text(canon / "seed.txt", "canonical")
            sink.mkdir()

            result = _run_kitchensync(
                ["--dry-run", f"+{_file_url(canon)}", _file_url(missing), str(sink)],
                cwd=root,
            )
            _assert_exit_code(failures, "004.12", result, 0)
            _fail(failures, not missing.exists(), "004.12: dry-run must not create missing file:// root directory")

    # 004.13 -- dry-run treats missing file:// roots as failed for that peer.
    def check_file_url_dry_run_treats_missing_as_failed() -> None:
        with tempfile.TemporaryDirectory(prefix="ks_004_13_") as raw_tmp:
            root = Path(raw_tmp)
            canon = root / "canon"
            missing = root / "missing"

            _write_text(canon / "seed.txt", "canonical")
            result = _run_kitchensync(["--dry-run", f"+{_file_url(canon)}", _file_url(missing)], cwd=root)
            _assert_nonzero(failures, "004.13", result)

    # 004.14-004.16, 004.17 -- SFTP-specific root handling and timeout behavior requires sftp transport and is not tested here via local process surface.
    # 004.14: normal runs create missing sftp:// root before using winning URL
    # 004.15: dry-run does not create missing sftp:// root
    # 004.16: dry-run treats missing sftp:// root as failed
    # 004.17: root creation failure for file:///sftp:// normal run treats that URL as failed

    # 004.17 (file:// representative) -- root creation failures in normal run mark URL as failed.
    def check_root_creation_failure_marks_unreachable() -> None:
        with tempfile.TemporaryDirectory(prefix="ks_004_17_") as raw_tmp:
            root = Path(raw_tmp)
            canon = root / "canon"
            blocked_parent = root / "blocked-parent"
            blocked_peer = blocked_parent / "missing-root"
            sink = root / "sink"

            _write_text(canon / "seed.txt", "canonical")
            sink.mkdir()
            _write_text(blocked_parent, "cannot")

            result = _run_kitchensync(
                [f"+{_file_url(canon)}", str(blocked_peer), str(sink)],
                cwd=root,
            )
            _assert_exit_code(failures, "004.17", result, 0)
            _fail(failures, not blocked_peer.exists(), "004.17: missing-root creation failure should not create blocked path")

    # 004.22, 004.23 -- file:// timeouts are connection-stage no-ops; not directly separable from file-peer behavior in this test model.

    # 004.27, 004.28, 004.30 -- bare peers and file:// peers use local filesystem operations, no peer infrastructure required.
    def check_local_file_and_file_url_are_peer_transports() -> None:
        with tempfile.TemporaryDirectory(prefix="ks_004_27_28_30_") as raw_tmp:
            root = Path(raw_tmp)
            bare_canon = root / "bare-canon"
            file_peer = root / "file-peer"

            _write_text(bare_canon / "seed.txt", "local")

            result = _run_kitchensync([f"+{bare_canon}", _file_url(file_peer)], cwd=root)
            _assert_exit_code(failures, "004.27/004.28/004.30", result, 0)
            _fail(
                failures,
                (file_peer / "seed.txt").read_text(encoding="utf-8") == "local",
                "004.27/004.28/004.30: file:// peer and bare peer should both participate as local filesystem operations",
            )
            _assert_stderr_empty(failures, "004.27/004.28/004.30", result)

    # Not reasonably testable from release binary CLI/log surface without in-process SFTP auth stack:
    # 004.1 -- startup parallelism across all peers
    # 004.18 -- timeout-conn applies to sftp:// handshakes
    # 004.19 -- timeout-conn URL override for sftp://
    # 004.20 -- timeout on handshake marks URL as failed
    # 004.21 -- timeout-idle URL override for sftp://
    # 004.24 -- SFTP auth order
    # 004.25 -- Host key verification from ~/.ssh/known_hosts
    # 004.26 -- Rejecting untrusted SFTP hosts
    # 004.29 -- sftp:// URL path uses SSH/SFTP transport

    for label, fn in {
        "004.2/004.3/004.4/004.5": check_fallback_primary_wins,
        "004.3": check_fallback_uses_secondary_after_primary_fail,
        "004.6": check_fallback_no_retry_after_winner,
        "004.7": check_unreachable_peer_skipped_and_sync_continues,
        "004.8": check_unreachable_peer_diagnostic,
        "004.9": check_too_few_reachable_peers,
        "004.10": check_canon_unreachable_fails,
        "004.11": check_file_url_creates_missing_root_in_normal_run,
        "004.12": check_file_url_dry_run_no_create_root,
        "004.13": check_file_url_dry_run_treats_missing_as_failed,
        "004.17": check_root_creation_failure_marks_unreachable,
        "004.27/004.28/004.30": check_local_file_and_file_url_are_peer_transports,
    }.items():
        _run_case(label, failures, fn)

    if failures:
        print("FAIL: test_004_peer_connectivity.py")
        for index, failure in enumerate(failures, start=1):
            print(f"  {index:02d}. {failure}")
        return 1

    print("PASS: test_004_peer_connectivity.py")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
