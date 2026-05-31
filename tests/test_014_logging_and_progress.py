#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# ///

"""End-to-end verification for reqs/014_logging-and-progress.md."""

from __future__ import annotations

import os
import re
import subprocess
import sys
import tempfile
import time
from pathlib import Path


sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")


WORKSPACE_ROOT = Path(__file__).resolve().parents[1]
PROJECT_DIR = WORKSPACE_ROOT / "proj"
HELP_SPEC_PATH = WORKSPACE_ROOT / "specs" / "help.md"
RELEASED_EXE = (
    WORKSPACE_ROOT / "released" / "kitchensync.exe"
    if os.name == "nt"
    else WORKSPACE_ROOT / "released" / "kitchensync"
)

VERBOSE_PHASES = {
    "read_source",
    "write_swap_new",
    "move_existing_to_swap_old",
    "rename_final",
    "set_mod_time",
    "archive_old",
    "cleanup",
}


def _normalize_output(value: str) -> str:
    return value.replace("\r\n", "\n").replace("\r", "\n")


def _run_kitchensync(
    args: list[str],
    *,
    cwd: Path,
    timeout_seconds: float = 30.0,
) -> subprocess.CompletedProcess[str] | None:
    cmd = [str(RELEASED_EXE), *args]
    try:
        return subprocess.run(
            cmd,
            cwd=str(cwd),
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout_seconds,
        )
    except subprocess.TimeoutExpired:
        return None
    except FileNotFoundError as exc:
        return subprocess.CompletedProcess(args=cmd, returncode=127, stdout="", stderr=f"{exc}")


def _run_kitchensync_with_timing(
    args: list[str],
    *,
    cwd: Path,
    timeout_seconds: float = 30.0,
) -> tuple[subprocess.CompletedProcess[str] | None, float]:
    start = time.perf_counter()
    result = _run_kitchensync(args, cwd=cwd, timeout_seconds=timeout_seconds)
    elapsed = max(0.0, time.perf_counter() - start)
    return result, elapsed


def _load_expected_help() -> str:
    raw = HELP_SPEC_PATH.read_text(encoding="utf-8", errors="replace")
    raw = raw.lstrip("\ufeff")
    match = re.search(r"(?ms)^# Help Screen.*?```(.*?)```", raw)
    if not match:
        raise AssertionError("failed to parse help block from specs/help.md")
    return _normalize_output(match.group(1)).strip("\n")


def _file_url(path: Path) -> str:
    return path.resolve().as_uri()


def _write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def _run_case(name: str, failures: list[str], fn) -> None:
    try:
        fn()
    except AssertionError as exc:
        failures.append(f"{name}: {exc}")
    except Exception as exc:  # noqa: BLE001
        failures.append(f"{name}: unexpected exception {exc!r}")


def _assert_exit_code(
    failures: list[str],
    req_id: str,
    result: subprocess.CompletedProcess[str] | None,
    command: list[str],
    expected: int,
) -> None:
    if result is None:
        failures.append(f"{req_id}: command timed out for {command!r}")
        return
    if result.returncode != expected:
        failures.append(
            f"{req_id}: expected exit code {expected}, got {result.returncode}; command={command!r}; "
            f"stdout={result.stdout!r}; stderr={result.stderr!r}"
        )


def _assert_stderr_empty(failures: list[str], req_id: str, result: subprocess.CompletedProcess[str]) -> None:
    if result.stderr:
        failures.append(f"{req_id}: expected empty stderr, got {result.stderr!r}")


def _assert_in_output(failures: list[str], req_id: str, output: str, token: str, message: str) -> None:
    if token not in output:
        failures.append(f"{req_id}: {message}; token={token!r}; output={output!r}")


def _bootstrap_sync_pair_with_plus(root: Path) -> tuple[Path, Path, subprocess.CompletedProcess[str]]:
    source = root / "source"
    destination = root / "destination"
    _write_text(source / "seed.txt", "seed v1")

    result = _run_kitchensync(
        ["--verbosity", "error", f"+{_file_url(source)}", _file_url(destination)],
        cwd=root,
    )
    if result is None:
        raise AssertionError("bootstrap kitchensync timed out")
    if result.returncode != 0:
        raise AssertionError(
            f"bootstrap command failed; returncode={result.returncode}; stdout={result.stdout!r}; stderr={result.stderr!r}"
        )
    return source, destination, result


def _parse_copy_slot_events(output: str) -> list[tuple[int, int]]:
    pattern = re.compile(r"copy-slots active=(\d+)/(\d+)")
    matches: list[tuple[int, int]] = []
    for line in output.splitlines():
        m = pattern.search(line)
        if m:
            matches.append((int(m.group(1)), int(m.group(2))))
    return matches


def _find_transfer_line(output: str) -> str | None:
    lowered = output.lower()
    for line in lowered.splitlines():
        if "transfer failed" in line:
            return line
    return None


def _extract_transfer_fields(output: str) -> tuple[str | None, str | None, str | None, str | None]:
    line = _find_transfer_line(_normalize_output(output))
    if not line:
        return None, None, None, None

    m = re.search(
        r"transfer failed for (.+?) to (\S+):\s*([a-z_]+):\s*(.+)",
        line,
        flags=re.IGNORECASE,
    )
    if not m:
        return None, None, None, None
    return m.group(1), m.group(2), m.group(3), m.group(4)


def _assert_transfer_diagnostic(
    failures: list[str],
    req_id: str,
    result: subprocess.CompletedProcess[str] | None,
    *,
    expected_relative_path: str,
    expected_peer: str,
    expected_phase: str,
) -> str:
    if result is None:
        failures.append(f"{req_id}: command timed out")
        return ""

    output = _normalize_output(result.stdout)
    raw_line = _find_transfer_line(output)
    if raw_line is None:
        failures.append(f"{req_id}: no transfer failure diagnostic found in output {output!r}")
        return output

    relative_path, peer, phase, category = _extract_transfer_fields(output)
    if relative_path is None:
        if expected_relative_path not in raw_line:
            failures.append(
                f"{req_id}: malformed transfer failure line; raw={raw_line!r}; expected path token {expected_relative_path!r}"
            )
    else:
        if expected_relative_path not in relative_path and expected_relative_path not in raw_line:
            failures.append(
                f"{req_id}: transfer diagnostic path did not include expected relative path {expected_relative_path!r}; raw={raw_line!r}"
            )

    if expected_peer and expected_peer not in raw_line:
        if expected_peer not in peer if peer else True:
            failures.append(
                f"{req_id}: transfer diagnostic did not include destination peer token {expected_peer!r}; raw={raw_line!r}"
            )

    if expected_phase not in (phase or ""):
        failures.append(
            f"{req_id}: expected transfer phase {expected_phase!r}, got {phase!r}; line={raw_line!r}"
        )

    if (category or "").strip() == "":
        failures.append(f"{req_id}: transfer diagnostic lacked transport category; line={raw_line!r}")

    if phase and phase not in VERBOSE_PHASES:
        failures.append(f"{req_id}: phase {phase!r} is outside the specified set {sorted(VERBOSE_PHASES)}")

    return output


def check_014_1_stdout_only_and_exit_and_no_stderr(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_014_stdout_") as tmpdir:
        root = Path(tmpdir)
        source, destination, _bootstrap = _bootstrap_sync_pair_with_plus(root)

        result = _run_kitchensync(
            ["--dry-run", "--verbosity", "info", f"+{_file_url(source)}", _file_url(destination)],
            cwd=root,
        )
        _assert_exit_code(failures, "014.1", result, ["--dry-run", "--verbosity", "info", f"+{_file_url(source)}", _file_url(destination)], 0)
        if result is None:
            return
        _assert_stderr_empty(failures, "014.1", result)


def check_014_2_validation_and_help_errors(failures: list[str]) -> None:
    expected_help = _load_expected_help()

    invalid = _run_kitchensync(["--max-copies", "0"], cwd=WORKSPACE_ROOT)
    if invalid is None:
        failures.append("014.2/014.3/014.4: command timed out")
        return

    _assert_exit_code(failures, "014.4", invalid, ["--max-copies", "0"], 1)
    _assert_stderr_empty(failures, "014.4", invalid)

    normalized = _normalize_output(invalid.stdout)
    if not normalized.strip():
        failures.append("014.3: expected output for non-help validation error, got empty stdout")
        return

    if not normalized.rstrip("\n").endswith(expected_help):
        failures.append(
            "014.3: expected help text after validation error; stdout does not end with help block"
        )
    if normalized == expected_help:
        failures.append("014.3: expected an error message before help output")


def check_014_5_no_canon_for_first_sync(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_014_firstsync_") as tmpdir:
        root = Path(tmpdir)
        source = root / "source"
        destination = root / "destination"
        source.mkdir(parents=True, exist_ok=True)
        destination.mkdir(parents=True, exist_ok=True)

        result = _run_kitchensync(
            ["--dry-run", str(source), str(destination)],
            cwd=root,
        )
        if result is None:
            failures.append("014.5/014.6: command timed out")
            return

        _assert_exit_code(failures, "014.6", result, ["--dry-run", str(source), str(destination)], 1)
        _assert_stderr_empty(failures, "014.5", result)
        _assert_in_output(
            failures,
            "014.5",
            _normalize_output(result.stdout),
            "First sync? Mark the authoritative peer with a leading +",
            "missing canon-peer first-sync message",
        )


def check_014_7_no_contributing_peer_reachable(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_014_subordinate_") as tmpdir:
        root = Path(tmpdir)
        source, destination, _ = _bootstrap_sync_pair_with_plus(root)

        no_contrib = _run_kitchensync(
            ["--verbosity", "error", f"-{_file_url(source)}", f"-{_file_url(destination)}"],
            cwd=root,
        )
        if no_contrib is None:
            failures.append("014.7/014.8: command timed out")
            return
        _assert_exit_code(failures, "014.8", no_contrib, ["--verbosity", "error", f"-{_file_url(source)}", f"-{_file_url(destination)}"], 1)
        _assert_stderr_empty(failures, "014.7", no_contrib)
        _assert_in_output(
            failures,
            "014.7",
            _normalize_output(no_contrib.stdout),
            "No contributing peer reachable - cannot make sync decisions",
            "expected no-contributing-peer message",
        )


def check_014_9_and_014_10_reachability_errors(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_014_reachable_") as tmpdir:
        root = Path(tmpdir)
        source, destination, _ = _bootstrap_sync_pair_with_plus(root)
        missing = root / "missing-canon"

        canon_missing = _run_kitchensync(
            ["--dry-run", "--verbosity", "error", f"+{_file_url(missing)}", _file_url(destination)],
            cwd=root,
        )
        if canon_missing is None:
            failures.append("014.9: command timed out")
        else:
            _assert_exit_code(
                failures,
                "014.9",
                canon_missing,
                ["--dry-run", "--verbosity", "error", f"+{_file_url(missing)}", _file_url(destination)],
                1,
            )
            if canon_missing is not None:
                _assert_stderr_empty(failures, "014.9", canon_missing)
                _assert_in_output(failures, "014.9", _normalize_output(canon_missing.stdout), "unreachable", "missing canon peer did not surface unreachable condition")
                if not _normalize_output(canon_missing.stdout).strip():
                    failures.append("014.13: unreachable peer diagnostic did not appear at error verbosity")

        not_enough = _run_kitchensync(
            ["--dry-run", "--verbosity", "error", _file_url(source), str(missing)],
            cwd=root,
        )
        if not_enough is None:
            failures.append("014.10: command timed out")
            return
        _assert_exit_code(
            failures,
            "014.10",
            not_enough,
            ["--dry-run", "--verbosity", "error", _file_url(source), str(missing)],
            1,
        )
        _assert_stderr_empty(failures, "014.10", not_enough)


def check_014_11_and_014_12_and_014_13_completion_and_errors(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_014_success_") as tmpdir:
        root = Path(tmpdir)
        source, destination, _ = _bootstrap_sync_pair_with_plus(root)
        _write_text(source / "seed.txt", "seed v2")

        result = _run_kitchensync(
            ["--verbosity", "info", f"+{_file_url(source)}", _file_url(destination)],
            cwd=root,
        )
        if result is None:
            failures.append("014.11/014.12/014.13: command timed out")
            return

        _assert_exit_code(failures, "014.12", result, ["--verbosity", "info", f"+{_file_url(source)}", _file_url(destination)], 0)
        _assert_stderr_empty(failures, "014.11", result)
        stdout = _normalize_output(result.stdout).lower()
        if not any(token in stdout for token in ("complete", "completed", "success", "finished")):
            failures.append("014.11: expected completion message on stdout")


def check_014_23_24_25_26_27_transfer_failure_fields(failures: list[str]) -> None:
    # This case intentionally exercises a deterministic local staging failure and validates the transfer failure surface
    if os.name == "nt":
        return

    with tempfile.TemporaryDirectory(prefix="ks_014_transfer_") as tmpdir:
        root = Path(tmpdir)
        source, destination, _ = _bootstrap_sync_pair_with_plus(root)
        nested = destination / ".kitchensync"
        _write_text(source / "nested" / "changed.txt", "updated")

        destination.chmod(0o555)
        try:
            result = _run_kitchensync(
                ["--verbosity", "info", "--retries-copy", "1", f"+{_file_url(source)}", _file_url(destination)],
                cwd=root,
                timeout_seconds=30.0,
            )
        finally:
            destination.chmod(0o755)

        if result is None:
            failures.append("014.15: command timed out")
            return

        out = _normalize_output(result.stdout)
        _assert_stderr_empty(failures, "014.23/14.24/14.25/14.26/14.27", result)
        path_line, peer_line, phase_line, category_line = _extract_transfer_fields(out)
        if not path_line:
            failures.append(
                f"014.23/014.24/014.25/014.26/014.27: could not parse transfer failure diagnostic from output {out!r}"
            )
            return

        line_lower = _find_transfer_line(out) or ""
        if "nested/changed.txt" not in line_lower:
            failures.append("014.23: transfer diagnostic missing relative path")

        # destination peer is printed as file URL in this test
        expected_peer = _file_url(destination)
        if expected_peer.lower() not in line_lower:
            failures.append(f"014.24: transfer diagnostic missing destination peer url {expected_peer!r}")

        if "write_swap_new" not in line_lower:
            failures.append("014.25: expected failure phase information to include failed phase")

        if not category_line:
            failures.append("014.26: expected transport error category in transfer diagnostic")
        if phase_line and phase_line not in VERBOSE_PHASES:
            failures.append(f"014.27: phase {phase_line!r} not in the allowed transfer phase set")

        if "write_swap_new" not in line_lower:
            failures.append("014.15: expected transfer failure before SWAP old exists (write_swap_new)")


def check_014_17_archive_old_failure(failures: list[str]) -> None:
    if os.name == "nt":
        return

    with tempfile.TemporaryDirectory(prefix="ks_014_archive_") as tmpdir:
        root = Path(tmpdir)
        source, destination, _ = _bootstrap_sync_pair_with_plus(root)
        _write_text(source / "seed.txt", "seed v2")

        # Block archive destination with a file in place of BAK/ to force archive_old failure during replacement.
        bak_path = destination / ".kitchensync" / "BAK"
        bak_path.write_text("blocked", encoding="utf-8")

        result = _run_kitchensync(
            ["--verbosity", "info", "--retries-copy", "1", f"+{_file_url(source)}", _file_url(destination)],
            cwd=root,
        )
        if result is None:
            failures.append("014.17: command timed out")
            return

        output = _normalize_output(result.stdout).lower()
        _assert_stderr_empty(failures, "014.17", result)
        if "archive_old" not in output:
            failures.append("014.17: expected archive_old diagnostic on archive failure")
        if "error" not in output:
            failures.append("014.17: expected error-level text for archive_old failure")


def check_014_18_displacement_failure(failures: list[str]) -> None:
    if os.name == "nt":
        return

    with tempfile.TemporaryDirectory(prefix="ks_014_displace_") as tmpdir:
        root = Path(tmpdir)
        source, destination, _ = _bootstrap_sync_pair_with_plus(root)

        # Add a file only on destination so it should be displaced to BAK during normal sync.
        _write_text(destination / "extra.txt", "local-only")

        # Make the expected BAK target unusable to force a displacement error.
        bak_dir = destination / ".kitchensync"
        bak_dir.mkdir(exist_ok=True)
        bak_block = bak_dir / "BAK"
        bak_block.write_text("blocked", encoding="utf-8")

        result = _run_kitchensync(
            ["--verbosity", "info", f"+{_file_url(source)}", _file_url(destination)],
            cwd=root,
        )
        if result is None:
            failures.append("014.18: command timed out")
            return

        output = _normalize_output(result.stdout).lower()
        _assert_stderr_empty(failures, "014.18", result)
        if "error" not in output:
            failures.append("014.18: expected an error-level diagnostic when displacement cannot complete")
        if "displace" not in output:
            failures.append("014.18: expected displacement diagnostic text")


def check_014_28_29_30_31_info_debug_error_verbosity(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_014_verbosity_") as tmpdir:
        root = Path(tmpdir)
        source = root / "source"
        destination = root / "destination"
        _write_text(source / "seed.txt", "v1")
        destination.mkdir(parents=True, exist_ok=True)

        result_default = _run_kitchensync(
            ["--dry-run", f"+{_file_url(source)}", _file_url(destination)],
            cwd=root,
        )
        result_error = _run_kitchensync(
            ["--dry-run", "--verbosity", "error", f"+{_file_url(source)}", _file_url(destination)],
            cwd=root,
        )
        result_info = _run_kitchensync(
            ["--dry-run", "--verbosity", "info", f"+{_file_url(source)}", _file_url(destination)],
            cwd=root,
        )
        result_debug = _run_kitchensync(
            ["--dry-run", "--verbosity", "debug", f"+{_file_url(source)}", _file_url(destination)],
            cwd=root,
        )

        if result_default is None or result_error is None or result_info is None or result_debug is None:
            failures.append("014.28/014.29/014.30/014.31: command timed out")
            return

        out_default = _normalize_output(result_default.stdout)
        out_error = _normalize_output(result_error.stdout)
        out_info = _normalize_output(result_info.stdout)
        out_debug = _normalize_output(result_debug.stdout)

        if out_default != out_info:
            failures.append("014.28: default verbosity output did not match --verbosity info output")

        _assert_exit_code(failures, "014.28", result_default, ["--dry-run", f"+{_file_url(source)}", _file_url(destination)], 0)
        _assert_exit_code(failures, "014.29", result_error, ["--dry-run", "--verbosity", "error", f"+{_file_url(source)}", _file_url(destination)], 0)
        _assert_exit_code(failures, "014.30", result_info, ["--dry-run", "--verbosity", "info", f"+{_file_url(source)}", _file_url(destination)], 0)
        _assert_exit_code(failures, "014.31", result_debug, ["--dry-run", "--verbosity", "debug", f"+{_file_url(source)}", _file_url(destination)], 0)

        _assert_stderr_empty(failures, "014.28", result_default)
        _assert_stderr_empty(failures, "014.29", result_error)
        _assert_stderr_empty(failures, "014.30", result_info)
        _assert_stderr_empty(failures, "014.31", result_debug)

        if "Scanning:" not in out_default:
            failures.append("014.28: default verbosity did not show progress output")
        if "Scanning:" in out_error:
            failures.append("014.29: error verbosity should omit info-level progress output")
        if out_info == out_debug:
            # observably identical, per spec
            pass
        else:
            failures.append("014.31: debug output not observationally identical to info output")


def check_014_32_33_34_35_36_copy_slot_trace(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_014_trace_") as tmpdir:
        root = Path(tmpdir)
        source = root / "source"
        destination = root / "destination"
        _write_text(source / "seed.txt", "seed v1")
        _write_text(source / "nested" / "changed.txt", "payload")

        # baseline to create snapshots and ensure active copy slots are exercised on change.
        _bootstrap_sync_pair_with_plus(root)
        _write_text(source / "new.txt", "new payload")

        result, duration = _run_kitchensync_with_timing(
            ["--max-copies", "2", "--verbosity", "trace", f"+{_file_url(source)}", _file_url(destination)],
            cwd=root,
            timeout_seconds=40.0,
        )
        if result is None:
            failures.append("014.32/033/034/035/036: command timed out")
            return

        out = _normalize_output(result.stdout)
        _assert_exit_code(failures, "014.32", result, ["--max-copies", "2", "--verbosity", "trace", f"+{_file_url(source)}", _file_url(destination)], 0)
        _assert_stderr_empty(failures, "014.32", result)

        events = _parse_copy_slot_events(out)
        if not events:
            failures.append("014.32: expected copy-slot events in trace output")
            return

        # 014.35: each event should use required format and 014.36: active count should stay within configured max.
        observed_max = max(maximum for _, maximum in events)
        if observed_max != 2:
            failures.append(f"014.35/036: expected trace max-copies format with 2, got observed max {observed_max}")

        if not any(active == 1 for active, _ in events):
            failures.append("014.33/034: expected slot acquire event")
        if not any(active == 0 for active, _ in events):
            failures.append("014.34: expected slot release behavior")

        if any(active < 0 or max_active < 0 for active, max_active in events):
            failures.append("014.33/34/35/36: invalid copy-slot counters in output")
        if any(active > max_active for active, max_active in events):
            failures.append("014.36: active copy slots exceeded configured max")


def check_014_50_51_52_noninteractive_progress(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_014_progress_") as tmpdir:
        root = Path(tmpdir)
        source = root / "source"
        destination = root / "destination"
        _write_text(source / "scan-root" / "seed.txt", "payload")
        destination.mkdir(parents=True, exist_ok=True)

        result, elapsed = _run_kitchensync_with_timing(
            ["--dry-run", "--verbosity", "info", f"+{_file_url(source)}", _file_url(destination)],
            cwd=root,
        )
        if result is None:
            failures.append("014.50/014.51/014.52: command timed out")
            return

        _assert_exit_code(failures, "014.50/014.51/014.52", result, ["--dry-run", "--verbosity", "info", f"+{_file_url(source)}", _file_url(destination)], 0)
        _assert_stderr_empty(failures, "014.50/014.51/014.52", result)

        raw = result.stdout
        output = _normalize_output(raw)

        if "\x1b[" in raw:
            failures.append("014.50: stdout progress control sequences found in non-interactive run")

        if "Scanning:" not in output:
            failures.append("014.52: non-interactive progress did not include currently scanned directory")

        _ = elapsed


def main() -> int:
    failures: list[str] = []

    if not WORKSPACE_ROOT.exists():
        failures.append(f"precondition: workspace root missing at {WORKSPACE_ROOT}")
    if not PROJECT_DIR.exists():
        failures.append(f"precondition: project directory missing at {PROJECT_DIR}")
    if not RELEASED_EXE.exists():
        failures.append(f"precondition: released executable missing at {RELEASED_EXE}")
    if not HELP_SPEC_PATH.exists():
        failures.append(f"precondition: help spec missing at {HELP_SPEC_PATH}")

    if failures:
        print("FAIL: test_014_logging_and_progress.py")
        for index, item in enumerate(failures, start=1):
            print(f"  {index:02d}. {item}")
        return 1

    _run_case("014.1", failures, lambda: check_014_1_stdout_only_and_exit_and_no_stderr(failures))
    _run_case("014.3/014.4", failures, lambda: check_014_2_validation_and_help_errors(failures))
    _run_case("014.5/014.6", failures, lambda: check_014_5_no_canon_for_first_sync(failures))
    _run_case("014.7/014.8", failures, lambda: check_014_7_no_contributing_peer_reachable(failures))
    _run_case("014.9/014.10", failures, lambda: check_014_9_and_014_10_reachability_errors(failures))
    _run_case("014.11/014.12/014.13", failures, lambda: check_014_11_and_014_12_and_014_13_completion_and_errors(failures))
    _run_case(
        "014.23/014.24/014.25/014.26/014.27/014.15",
        failures,
        lambda: check_014_23_24_25_26_27_transfer_failure_fields(failures),
    )
    _run_case("014.17", failures, lambda: check_014_17_archive_old_failure(failures))
    _run_case("014.18", failures, lambda: check_014_18_displacement_failure(failures))
    _run_case("014.28/014.29/014.30/014.31", failures, lambda: check_014_28_29_30_31_info_debug_error_verbosity(failures))
    _run_case("014.32/014.33/014.34/014.35/014.36", failures, lambda: check_014_32_33_34_35_36_copy_slot_trace(failures))
    _run_case("014.50/014.51/014.52", failures, lambda: check_014_50_51_52_noninteractive_progress(failures))

    # not reasonably testable: 014.14 -- no local, cross-platform deterministic way to exhaust list retries without a purpose-built transport/error fixture.
    # not reasonably testable: 014.16 -- no reliable cross-platform way to force a post-SWAP-old transfer failure without transport-level fault injection.
    # not reasonably testable: 014.19 -- covered by 014.15 as the staging failure path is forced and surfaced as transfer failure.
    # not reasonably testable: 014.20 -- set_mod_time failure injection requires post-copy metadata fault injection unavailable from this CLI surface.
    # not reasonably testable: 014.21 -- snapshot upload failure-before-SWAP-old requires peer-side snapshot write fault injection.
    # not reasonably testable: 014.22 -- snapshot upload failure-after-SWAP-old requires snapshot swap-failure injection at a specific point.
    # not reasonably testable: 014.51 -- output cadence verification for non-interactive runs is timing-sensitive and not robust across CI environments.
    # not reasonably testable: 014.37 -- requires interactive terminal capture of live screen.
    # not reasonably testable: 014.38 -- requires interactive terminal capture with high-resolution timing of refresh.
    # not reasonably testable: 014.39 -- requires interactive terminal capture and internal event coalescing visibility.
    # not reasonably testable: 014.40 -- requires interactive terminal live row layout capture.
    # not reasonably testable: 014.41 -- requires interactive live row rendering capture.
    # not reasonably testable: 014.42 -- requires interactive live row rendering capture.
    # not reasonably testable: 014.43 -- requires interactive live progress bar capture.
    # not reasonably testable: 014.44 -- requires interactive live progress bar capture.
    # not reasonably testable: 014.45 -- requires interactive live progress row lifecycle capture.
    # not reasonably testable: 014.46 -- requires interactive live bottom scanning-row capture.
    # not reasonably testable: 014.47 -- requires interactive live scanning-row root behavior.
    # not reasonably testable: 014.48 -- requires interactive live relative non-root scanning-row behavior.
    # not reasonably testable: 014.49 -- requires interactive live summary row layout capture.
    # not reasonably testable: 014.53 -- requires interactive-to-terminal persistence capture.
    # not reasonably testable: 014.54 -- requires interactive-to-terminal completion visibility capture.

    if failures:
        print("FAIL: test_014_logging_and_progress.py")
        for index, item in enumerate(failures, start=1):
            print(f"  {index:02d}. {item}")
        return 1

    print("PASS: test_014_logging_and_progress.py")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
