#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

from __future__ import annotations

import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path


PROJECT_DIR = Path(__file__).resolve().parents[1]
JAVA = PROJECT_DIR / "tools/compiler/jdk/bin/java"
JAR = PROJECT_DIR / "released/kitchensync.jar"

REMOTE_HOST = "ordinarydata.com"
REMOTE_USER = "ace"
REMOTE_BASE = f"/tmp/testks/kitchensync_03_logging_{os.getpid()}"
REMOTE_URL_BASE = f"sftp://{REMOTE_USER}@{REMOTE_HOST}{REMOTE_BASE}"

LOCAL_BASE = Path(tempfile.gettempdir()) / f"kitchensync_03_logging_{os.getpid()}"


class Check:
    def __init__(self) -> None:
        self.failures: list[str] = []

    def true(self, condition: bool, message: str) -> None:
        if not condition:
            self.failures.append(message)

    def equal(self, actual: object, expected: object, message: str) -> None:
        if actual != expected:
            self.failures.append(f"{message}: expected {expected!r}, got {actual!r}")


def run_cli(*args: str, timeout: int = 60) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(JAVA), "-jar", str(JAR), *args],
        cwd=str(PROJECT_DIR),
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=timeout,
        check=False,
    )


def run_ssh(command: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [
            "ssh",
            "-o",
            "BatchMode=yes",
            "-o",
            "StrictHostKeyChecking=accept-new",
            f"{REMOTE_USER}@{REMOTE_HOST}",
            command,
        ],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=30,
        check=False,
    )


def reset_local_base() -> None:
    if LOCAL_BASE.exists():
        shutil.rmtree(LOCAL_BASE)
    LOCAL_BASE.mkdir(parents=True)


def reset_remote_base(check: Check) -> None:
    result = run_ssh(f"rm -rf {REMOTE_BASE!r} && mkdir -p {REMOTE_BASE!r}")
    check.equal(result.returncode, 0, f"remote fixture reset failed; stderr={result.stderr!r}")


def write_file(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8", newline="\n")


def make_copy_tree(name: str) -> tuple[Path, Path, Path]:
    src = LOCAL_BASE / name / "src"
    dst_a = LOCAL_BASE / name / "dst_a"
    dst_b = LOCAL_BASE / name / "dst_b"
    write_file(src / "alpha.txt", "alpha\n")
    write_file(src / "nested" / "beta.txt", "beta\n")
    dst_a.mkdir(parents=True)
    dst_b.mkdir(parents=True)
    return src, dst_a, dst_b


def make_displacement_tree(name: str) -> tuple[Path, Path, Path]:
    src = LOCAL_BASE / name / "src"
    dst_a = LOCAL_BASE / name / "dst_a"
    dst_b = LOCAL_BASE / name / "dst_b"
    src.mkdir(parents=True)
    write_file(dst_a / "remove.txt", "old a\n")
    write_file(dst_b / "remove.txt", "old b\n")
    return src, dst_a, dst_b


def progress_lines(stdout: str, marker: str) -> list[str]:
    prefix = f"{marker} "
    return [line for line in stdout.splitlines() if line.startswith(prefix)]


def pool_lines(stdout: str) -> list[str]:
    return [line for line in stdout.splitlines() if "endpoint=" in line and "connections=" in line]


def check_clean_process(check: Check, result: subprocess.CompletedProcess[str], label: str) -> None:
    check.equal(result.returncode, 0, f"{label} should exit 0; stdout={result.stdout!r}; stderr={result.stderr!r}")
    check.equal(result.stderr, "", f"{label} should keep stderr empty")


def argument_parsing_checks(check: Check) -> None:
    help_result = run_cli("--help")
    check.equal(help_result.stderr, "", f"argument parsing should keep stderr empty; stderr={help_result.stderr!r}")
    invalid_result = run_cli("--mc", "0")
    check.equal(
        invalid_result.stderr,
        "",
        f"argument validation should keep stderr empty; stderr={invalid_result.stderr!r}",
    )


def local_progress_checks(check: Check) -> None:
    src, dst_a, dst_b = make_copy_tree("info_copy")
    info = run_cli("-vl", "info", f"+{src}", f"-{dst_a}", f"-{dst_b}")
    check_clean_process(check, info, "info copy sync")
    check.equal(
        sorted(progress_lines(info.stdout, "C")),
        ["C alpha.txt", "C nested/beta.txt"],
        "info copy should log each copy decision once",
    )

    src, dst_a, dst_b = make_displacement_tree("info_displacement")
    displacement = run_cli("-vl", "info", f"+{src}", f"-{dst_a}", f"-{dst_b}")
    check_clean_process(check, displacement, "info displacement sync")
    check.equal(
        progress_lines(displacement.stdout, "X"),
        ["X remove.txt"],
        "info displacement should log each displacement decision once",
    )

    src, dst_a, dst_b = make_copy_tree("error_copy")
    error = run_cli("-vl", "error", f"+{src}", f"-{dst_a}", f"-{dst_b}")
    check_clean_process(check, error, "error copy sync")
    check.equal(progress_lines(error.stdout, "C"), [], "error verbosity should not emit C progress lines")
    check.equal(progress_lines(error.stdout, "X"), [], "error verbosity should not emit X progress lines")

    src, dst_a, dst_b = make_displacement_tree("error_displacement")
    error_displacement = run_cli("-vl", "error", f"+{src}", f"-{dst_a}", f"-{dst_b}")
    check_clean_process(check, error_displacement, "error displacement sync")
    check.equal(progress_lines(error_displacement.stdout, "C"), [], "error verbosity should not emit C progress lines for displacement sync")
    check.equal(progress_lines(error_displacement.stdout, "X"), [], "error verbosity should not emit X progress lines for displacement sync")

    src, dst_a, dst_b = make_copy_tree("debug_copy")
    debug = run_cli("-vl", "debug", f"+{src}", f"-{dst_a}", f"-{dst_b}")
    check_clean_process(check, debug, "debug copy sync")
    check.equal(sorted(progress_lines(debug.stdout, "C")), ["C alpha.txt", "C nested/beta.txt"], "debug should match info progress output")
    check.equal(pool_lines(debug.stdout), [], "debug should not emit pool acquire/release lines")

    src, dst_a, dst_b = make_displacement_tree("debug_displacement")
    debug_displacement = run_cli("-vl", "debug", f"+{src}", f"-{dst_a}", f"-{dst_b}")
    check_clean_process(check, debug_displacement, "debug displacement sync")
    check.equal(progress_lines(debug_displacement.stdout, "X"), ["X remove.txt"], "debug should include displacement progress output")
    check.equal(pool_lines(debug_displacement.stdout), [], "debug displacement sync should not emit pool acquire/release lines")

    info_src, info_dst_a, info_dst_b = make_copy_tree("info_debug_equivalence_info")
    debug_src, debug_dst_a, debug_dst_b = make_copy_tree("info_debug_equivalence_debug")
    info_equivalent = run_cli("-vl", "info", f"+{info_src}", f"-{info_dst_a}", f"-{info_dst_b}")
    debug_equivalent = run_cli("-vl", "debug", f"+{debug_src}", f"-{debug_dst_a}", f"-{debug_dst_b}")
    check_clean_process(check, info_equivalent, "info equivalence sync")
    check_clean_process(check, debug_equivalent, "debug equivalence sync")
    check.equal(
        sorted(info_equivalent.stdout.splitlines()),
        sorted(debug_equivalent.stdout.splitlines()),
        "debug output should be observationally identical to info output",
    )

    info_src, info_dst_a, info_dst_b = make_displacement_tree("info_debug_displacement_equivalence_info")
    debug_src, debug_dst_a, debug_dst_b = make_displacement_tree("info_debug_displacement_equivalence_debug")
    info_displacement_equivalent = run_cli("-vl", "info", f"+{info_src}", f"-{info_dst_a}", f"-{info_dst_b}")
    debug_displacement_equivalent = run_cli("-vl", "debug", f"+{debug_src}", f"-{debug_dst_a}", f"-{debug_dst_b}")
    check_clean_process(check, info_displacement_equivalent, "info displacement equivalence sync")
    check_clean_process(check, debug_displacement_equivalent, "debug displacement equivalence sync")
    check.equal(
        sorted(info_displacement_equivalent.stdout.splitlines()),
        sorted(debug_displacement_equivalent.stdout.splitlines()),
        "debug displacement output should be observationally identical to info output",
    )

    src, dst_a, dst_b = make_copy_tree("trace_copy")
    trace = run_cli("-vl", "trace", f"+{src}", f"-{dst_a}", f"-{dst_b}")
    check_clean_process(check, trace, "trace local copy sync")
    check.equal(sorted(progress_lines(trace.stdout, "C")), ["C alpha.txt", "C nested/beta.txt"], "trace should include cumulative info progress output")

    src, dst_a, dst_b = make_displacement_tree("trace_displacement")
    trace_displacement = run_cli("-vl", "trace", f"+{src}", f"-{dst_a}", f"-{dst_b}")
    check_clean_process(check, trace_displacement, "trace local displacement sync")
    check.equal(progress_lines(trace_displacement.stdout, "X"), ["X remove.txt"], "trace should include cumulative displacement progress output")


def sftp_trace_checks(check: Check) -> None:
    for level in ("error", "info", "debug", "trace"):
        src = LOCAL_BASE / "sftp_trace" / level / "src"
        write_file(src / "remote.txt", f"{level}\n")
        remote_path = f"{REMOTE_BASE}/{level}"
        prep = run_ssh(f"rm -rf {remote_path!r} && mkdir -p {remote_path!r}")
        check.equal(prep.returncode, 0, f"remote {level} fixture setup failed; stderr={prep.stderr!r}")
        result = run_cli("-vl", level, "--mc", "2", f"+{src}", f"-{REMOTE_URL_BASE}/{level}", timeout=90)
        check_clean_process(check, result, f"{level} SFTP sync")
        lines = pool_lines(result.stdout)
        if level == "trace":
            pattern = re.compile(r"endpoint=ace@ordinarydata\.com:22 connections=\d+/2")
            check.true(lines, "trace should emit SFTP pool acquire/release lines")
            check.true(len(lines) >= 2, f"trace should include acquire and release pool events; lines={lines!r}")
            check.true(all(pattern.search(line) for line in lines), f"trace pool lines should include normalized endpoint and pool counts; lines={lines!r}")
            check.true(any(re.search(r"connections=[1-2]/2", line) for line in lines), f"trace should include at least one acquire above 0/2; lines={lines!r}")
            # A black-box test can observe that acquire and release events are logged,
            # but released connections may remain open until the keep-alive timeout.
        else:
            check.equal(lines, [], f"{level} should not emit SFTP pool acquire/release lines")


def list_dir_failure_checks(check: Check) -> None:
    for level in ("error", "info", "debug", "trace"):
        src = LOCAL_BASE / "sftp_missing" / level / "src"
        write_file(src / "blocked" / "present.txt", "present\n")
        remote_path = f"{REMOTE_BASE}/list-dir-failure-{level}"
        prep = run_ssh(
            f"rm -rf {remote_path!r} && mkdir -p {remote_path!r}/blocked && chmod 000 {remote_path!r}/blocked"
        )
        check.equal(prep.returncode, 0, f"list_dir {level} fixture setup failed; stderr={prep.stderr!r}")
        result = run_cli("-vl", level, f"+{src}", f"{REMOTE_URL_BASE}/list-dir-failure-{level}", timeout=60)
        run_ssh(f"chmod 700 {remote_path!r}/blocked")
        check.equal(result.stderr, "", f"list_dir {level} failure should keep stderr empty; stderr={result.stderr!r}")
        error_lines = [
            line
            for line in result.stdout.splitlines()
            if REMOTE_HOST in line and REMOTE_USER in line and "blocked" in line
        ]
        check.true(
            error_lines,
            f"list_dir {level} failure should identify affected peer and directory; stdout={result.stdout!r}",
        )


def main() -> int:
    check = Check()
    reset_local_base()
    reset_remote_base(check)
    try:
        argument_parsing_checks(check)
        local_progress_checks(check)
        sftp_trace_checks(check)
        list_dir_failure_checks(check)
    except subprocess.TimeoutExpired as exc:
        check.failures.append(f"subprocess timed out: {exc}")
    except Exception as exc:
        check.failures.append(f"unexpected test error: {exc!r}")
    finally:
        shutil.rmtree(LOCAL_BASE, ignore_errors=True)
        run_ssh(f"rm -rf {REMOTE_BASE!r}")

    if check.failures:
        print("FAILURES:")
        for failure in check.failures:
            print(f"- {failure}")
        return 1

    print("all logging checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
