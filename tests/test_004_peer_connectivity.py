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


RELEASED_BINARY = Path(__file__).resolve().parent.parent / "released" / (
    "kitchensync.exe" if os.name == "nt" else "kitchensync"
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


def _run_kitchensync(
    args: list[str],
    cwd: Path,
    timeout_seconds: int = 30,
) -> subprocess.CompletedProcess[str]:
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
            stderr="kitchensync invocation timed out",
        )


def _write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def _file_url(path: Path) -> str:
    return path.resolve().as_uri()


def _assert_exit_code(
    failures: list[str],
    label: str,
    result: subprocess.CompletedProcess[str],
    expected: int,
) -> None:
    _fail(
        failures,
        result.returncode == expected,
        f"{label}: expected exit code {expected}, got {result.returncode}. stdout={result.stdout.strip()!r}, stderr={result.stderr.strip()[:512]!r}",
    )


def _assert_nonzero(failures: list[str], label: str, result: subprocess.CompletedProcess[str]) -> None:
    _fail(
        failures,
        result.returncode != 0,
        f"{label}: expected non-zero exit code for failure path, got {result.returncode}. stdout={result.stdout.strip()!r}, stderr={result.stderr.strip()[:512]!r}",
    )


def _assert_contains_all(
    failures: list[str],
    label: str,
    text: str,
    terms: list[str],
) -> None:
    lower = text.lower()
    _fail(
        failures,
        all(term.lower() in lower for term in terms),
        f"{label}: expected all of {terms} in output, got {text!r}",
    )


def _assert_not_exists(failures: list[str], label: str, path: Path) -> None:
    _fail(failures, not path.exists(), f"{label}: expected path to remain absent: {path}")


def main() -> int:
    failures: list[str] = []

    _fail(failures, RELEASED_BINARY.is_file(), f"precondition: released executable missing at {RELEASED_BINARY}")

    # 004.2/004.4/004.5/004.6
    def check_fallback_winner_is_primary_and_used_for_all_operations() -> None:
        with tempfile.TemporaryDirectory(prefix="ks_004_2_") as raw_tmp:
            root = Path(raw_tmp)
            primary_root = root / "primary"
            secondary_root = root / "secondary"
            sink = root / "sink"

            _write_text(primary_root / "seed.txt", "primary-root")
            _write_text(secondary_root / "seed.txt", "secondary-root")
            _write_text(secondary_root / "fallback-only.txt", "secondary-only")

            result = _run_kitchensync(
                [f"+[{_file_url(primary_root)},{_file_url(secondary_root)}]", str(sink)],
                cwd=root,
            )
            _assert_exit_code(failures, "004.2/004.4/004.5/004.6", result, 0)
            if result.returncode == 0:
                _fail(
                    failures,
                    (sink / "seed.txt").read_text(encoding="utf-8") == "primary-root",
                    "004.2/004.4/004.5/004.6: expected winning primary URL payload",
                )
                _fail(
                    failures,
                    not (sink / "fallback-only.txt").exists(),
                    "004.2/004.4/004.5/004.6: secondary fallback URL should not have been used for copy operations",
                )

    def check_fallback_uses_secondary_only_after_primary_fails() -> None:
        with tempfile.TemporaryDirectory(prefix="ks_004_3_") as raw_tmp:
            root = Path(raw_tmp)
            bad_primary = root / "bad-primary"
            secondary = root / "secondary"
            sink = root / "sink"

            _write_text(bad_primary, "not-a-directory")
            _write_text(secondary / "seed.txt", "secondary-root")

            result = _run_kitchensync(
                [f"+[{_file_url(bad_primary)},{_file_url(secondary)}]", str(sink)],
                cwd=root,
            )
            _assert_exit_code(failures, "004.3", result, 0)
            if result.returncode == 0:
                _fail(
                    failures,
                    (sink / "seed.txt").read_text(encoding="utf-8") == "secondary-root",
                    "004.3: expected secondary fallback URL payload when primary URL does not connect",
                )

    # 004.3
    def check_fallback_urls_respected_in_list_order() -> None:
        with tempfile.TemporaryDirectory(prefix="ks_004_3_order_") as raw_tmp:
            root = Path(raw_tmp)
            bad_primary = root / "bad-primary"
            first_fallback = root / "fallback-first"
            second_fallback = root / "fallback-second"
            sink = root / "sink"

            _write_text(bad_primary, "not-a-directory")
            _write_text(first_fallback / "seed.txt", "first-fallback-root")
            _write_text(first_fallback / "first-only.txt", "first-only")
            _write_text(second_fallback / "seed.txt", "second-fallback-root")
            _write_text(second_fallback / "second-only.txt", "second-only")

            result = _run_kitchensync(
                [
                    f"+[{_file_url(bad_primary)},{_file_url(first_fallback)},{_file_url(second_fallback)}]",
                    str(sink),
                ],
                cwd=root,
            )
            _assert_exit_code(failures, "004.3", result, 0)
            if result.returncode == 0:
                _fail(
                    failures,
                    (sink / "seed.txt").read_text(encoding="utf-8") == "first-fallback-root",
                    "004.3: expected first fallback URL payload when primary URL does not connect",
                )
                _fail(
                    failures,
                    not (sink / "second-only.txt").exists(),
                    "004.3: later fallback URL should not have been used when an earlier fallback URL connects",
                )

    def check_unreachable_peer_skipped_and_sync_continues() -> None:
        with tempfile.TemporaryDirectory(prefix="ks_004_7_") as raw_tmp:
            root = Path(raw_tmp)
            canon = root / "canon"
            sink = root / "sink"
            bad_a = root / "bad-a"
            bad_b = root / "bad-b"

            _write_text(canon / "seed.txt", "canonical")
            _write_text(bad_a, "bad")
            _write_text(bad_b, "bad")

            result = _run_kitchensync(
                ["--verbosity", "error", f"+{_file_url(canon)}", str(sink), f"[{_file_url(bad_a)},{_file_url(bad_b)}]"],
                cwd=root,
            )
            _assert_exit_code(failures, "004.7", result, 0)
            if result.returncode == 0:
                _fail(
                    failures,
                    (sink / "seed.txt").read_text(encoding="utf-8") == "canonical",
                    "004.7: remaining reachable peer should still receive canonical data",
                )

    def check_unreachable_peer_logs_diagnostic() -> None:
        with tempfile.TemporaryDirectory(prefix="ks_004_8_") as raw_tmp:
            root = Path(raw_tmp)
            canon = root / "canon"
            sink = root / "sink"
            bad = root / "bad-peer"

            _write_text(canon / "seed.txt", "canonical")
            _write_text(bad, "bad")

            result = _run_kitchensync(
                ["--verbosity", "error", f"+{_file_url(canon)}", str(sink), f"[{_file_url(bad)}]"],
                cwd=root,
            )
            _assert_exit_code(failures, "004.8", result, 0)
            _assert_contains_all(
                failures,
                "004.8",
                result.stdout + result.stderr,
                ["unreachable"],
            )

    # 004.9
    def check_too_few_reachable_peers_fails() -> None:
        with tempfile.TemporaryDirectory(prefix="ks_004_9_") as raw_tmp:
            root = Path(raw_tmp)
            canon = root / "canon"
            missing = root / "missing"
            _write_text(canon / "seed.txt", "canonical")

            result = _run_kitchensync(["--dry-run", f"+{_file_url(canon)}", _file_url(missing)], cwd=root)
            _assert_nonzero(failures, "004.9", result)

    # 004.10
    def check_canon_unreachable_causes_error() -> None:
        with tempfile.TemporaryDirectory(prefix="ks_004_10_") as raw_tmp:
            root = Path(raw_tmp)
            bad_canon = root / "bad-canon"
            reachable_a = root / "reachable-a"
            reachable_b = root / "reachable-b"
            sink = root / "sink"

            _write_text(bad_canon, "not-a-directory")
            _write_text(reachable_a / "seed.txt", "a")
            _write_text(reachable_b / "seed.txt", "b")
            _write_text(sink / "seed.txt", "sink")

            result = _run_kitchensync(
                [f"+{_file_url(bad_canon)}", _file_url(reachable_a), _file_url(reachable_b), str(sink)],
                cwd=root,
            )
            _assert_nonzero(failures, "004.10", result)
            _assert_contains_all(
                failures,
                "004.10",
                result.stdout + result.stderr,
                ["unreachable"],
            )

    # 004.11
    def check_file_url_missing_root_created_in_normal_run() -> None:
        with tempfile.TemporaryDirectory(prefix="ks_004_11_") as raw_tmp:
            root = Path(raw_tmp)
            canon = root / "canon"
            missing = root / "missing" / "nested" / "peer"

            _write_text(canon / "seed.txt", "canonical")
            result = _run_kitchensync([f"+{_file_url(canon)}", _file_url(missing)], cwd=root)
            _assert_exit_code(failures, "004.11", result, 0)
            _fail(failures, missing.is_dir(), "004.11: expected missing file:// root to be created in normal run")
            _fail(
                failures,
                (missing / "seed.txt").read_text(encoding="utf-8") == "canonical",
                "004.11: expected normal file:// root to receive canonical data",
            )

    # 004.12
    def check_file_url_missing_root_not_created_in_dry_run() -> None:
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
            _assert_not_exists(failures, "004.12", missing)

    # 004.13
    def check_file_url_missing_root_treated_failed_in_dry_run() -> None:
        with tempfile.TemporaryDirectory(prefix="ks_004_13_") as raw_tmp:
            root = Path(raw_tmp)
            canon = root / "canon"
            missing = root / "missing"
            _write_text(canon / "seed.txt", "canonical")

            result = _run_kitchensync(["--dry-run", f"+{_file_url(canon)}", _file_url(missing)], cwd=root)
            _assert_nonzero(failures, "004.13", result)
            _assert_contains_all(
                failures,
                "004.13",
                result.stdout + result.stderr,
                ["unreachable"],
            )

    # 004.17
    def check_root_creation_failure_marks_url_failed() -> None:
        with tempfile.TemporaryDirectory(prefix="ks_004_17_") as raw_tmp:
            root = Path(raw_tmp)
            canon = root / "canon"
            blocked_parent = root / "blocked-parent"
            missing_root = blocked_parent / "nested" / "peer"
            sink = root / "sink"

            _write_text(canon / "seed.txt", "canonical")
            _write_text(blocked_parent, "blocked")
            sink.mkdir()

            result = _run_kitchensync(
                [f"+{_file_url(canon)}", _file_url(missing_root), str(sink)],
                cwd=root,
            )
            _assert_exit_code(failures, "004.17", result, 0)
            _assert_not_exists(failures, "004.17", missing_root)
            _fail(
                failures,
                (sink / "seed.txt").read_text(encoding="utf-8") == "canonical",
                "004.17: remaining peers should still participate when root creation fails",
            )

    # 004.27/004.28/004.30
    def check_bare_and_file_url_peers_are_local_fs_only() -> None:
        with tempfile.TemporaryDirectory(prefix="ks_004_27_") as raw_tmp:
            root = Path(raw_tmp)
            bare_canon = root / "bare-canon"
            file_peer = root / "file-peer"

            _write_text(bare_canon / "seed.txt", "local")

            result = _run_kitchensync([f"+{bare_canon}", _file_url(file_peer)], cwd=root)
            _assert_exit_code(failures, "004.27/004.28/004.30", result, 0)
            _fail(
                failures,
                (file_peer / "seed.txt").read_text(encoding="utf-8") == "local",
                "004.27/004.28/004.30: bare and file:// peers should both operate as local filesystem peers",
            )

    # Not reasonably testable from this root-level black-box surface:
    # 004.1 -- startup connection attempts across peers run concurrently (timing-dependent and not externally observable without hooks)
    # 004.14 -- normal runs create missing sftp:// peer root before using winning URL (requires SFTP server)
    # 004.15 -- dry-run does not create missing sftp:// peer root (requires SFTP server)
    # 004.16 -- dry-run treats missing sftp:// root as failed (requires SFTP server)
    # 004.18 -- --timeout-conn bounds sftp:// handshake (requires SFTP handshake timing assertions)
    # 004.19 -- timeout-conn override via URL query (requires SFTP URL query parsing in transport test harness)
    # 004.20 -- handshake timeout marks URL as failed (requires controllable slow SFTP server)
    # 004.21 -- timeout-idle override via URL query (requires SFTP keep-alive telemetry)
    # 004.22 -- file:// ignores timeout-conn during connect (timing path is not surfaced)
    # 004.23 -- file:// ignores timeout-idle during connect (timing path is not surfaced)
    # 004.24 -- sftp auth order (requires in-process SSH auth surface)
    # 004.25 -- known_hosts verification (requires SFTP infra)
    # 004.26 -- reject untrusted SFTP hosts (requires SFTP infra)
    # 004.29 -- sftp:// peers use SSH/SFTP operations (requires protocol-specific transport assertions)

    cases = {
        "004.2/004.4/004.5/004.6": check_fallback_winner_is_primary_and_used_for_all_operations,
        "004.3": check_fallback_urls_respected_in_list_order,
        "004.3-secondary": check_fallback_uses_secondary_only_after_primary_fails,
        "004.7": check_unreachable_peer_skipped_and_sync_continues,
        "004.8": check_unreachable_peer_logs_diagnostic,
        "004.9": check_too_few_reachable_peers_fails,
        "004.10": check_canon_unreachable_causes_error,
        "004.11": check_file_url_missing_root_created_in_normal_run,
        "004.12": check_file_url_missing_root_not_created_in_dry_run,
        "004.13": check_file_url_missing_root_treated_failed_in_dry_run,
        "004.17": check_root_creation_failure_marks_url_failed,
        "004.27/004.28/004.30": check_bare_and_file_url_peers_are_local_fs_only,
    }

    for label, fn in cases.items():
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
