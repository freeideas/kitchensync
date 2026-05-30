#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# ///

"""End-to-end verification for 002_help-screen."""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")


WORKSPACE_ROOT = Path(r"C:\Users\human\Desktop\prjx\kitchensync")
RELEASED_BINARY = (
    WORKSPACE_ROOT / "released" / "kitchensync.exe"
    if sys.platform == "win32"
    else WORKSPACE_ROOT / "released" / "kitchensync"
)
HELP_SPEC_PATH = WORKSPACE_ROOT / "specs" / "help.md"


def _normalize_text(value: str) -> str:
    return value.replace("\r\n", "\n").replace("\r", "\n")


def _load_expected_help_text() -> str:
    raw = HELP_SPEC_PATH.read_text(encoding="utf-8", errors="replace")
    match = re.search(r"(?ms)^# Help Screen.*?```(.*?)```", raw)
    assert match, "failed to extract help block from specs/help.md section 'Help Screen'"
    return _normalize_text(match.group(1)).strip("\r\n")


def _run_kitchensync(extra_args: list[str]) -> subprocess.CompletedProcess[str]:
    assert (
        RELEASED_BINARY.is_file()
    ), f"released executable does not exist at expected path: {RELEASED_BINARY}"
    return subprocess.run(
        [str(RELEASED_BINARY), *extra_args],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        check=False,
    )


def _run_case(name: str, check: object) -> str | None:
    try:
        check()
    except AssertionError as err:
        return f"{name}: assertion failed: {err}"
    except Exception as err:  # noqa: BLE001
        return f"{name}: unexpected exception: {err!r}"
    return None


def main() -> int:
    expected_help = _load_expected_help_text()
    failures: list[str] = []

    no_args = _run_kitchensync([])
    no_args_stdout = _normalize_text(no_args.stdout).strip("\r\n")
    no_args_stderr = _normalize_text(no_args.stderr).strip("\r\n")

    def check_002_1() -> None:
        assert no_args.returncode == 0, f"expected exit code 0, got {no_args.returncode}"
        assert (
            no_args_stdout == expected_help
        ), "no-argument output did not exactly match help screen block"

    failures.append(
        _run_case("002.1 writes exact help text on no-argument invocation", check_002_1)
    )

    def check_002_2() -> None:
        assert no_args.returncode == 0, f"expected exit code 0, got {no_args.returncode}"

    failures.append(_run_case("002.2 no-argument invocation exits 0", check_002_2))

    def check_002_3() -> None:
        assert no_args_stderr == "", f"expected stderr to be empty, got: {no_args_stderr!r}"

    failures.append(_run_case("002.3 no-argument invocation leaves stderr empty", check_002_3))

    one_peer = _run_kitchensync(["alpha"])
    one_peer_stdout = _normalize_text(one_peer.stdout).strip("\r\n")
    one_peer_stderr = _normalize_text(one_peer.stderr).strip("\r\n")

    def check_002_4() -> None:
        assert one_peer.returncode == 1, f"expected exit code 1, got {one_peer.returncode}"
        assert (
            expected_help in one_peer_stdout
        ), "validation error path did not include the exact help text"
        help_index = one_peer_stdout.rfind(expected_help)
        assert help_index != -1, "failed to locate help text in validation output"
        assert help_index > 0, "help text was not preceded by validation error output"
        assert one_peer_stdout.rstrip().endswith(
            expected_help
        ), "help text was not printed after the validation error message"
        assert one_peer_stderr == "", "expected stderr to be empty on validation error"

    failures.append(_run_case("002.4 non-help validation error includes help text after message", check_002_4))

    def check_002_5() -> None:
        assert one_peer.returncode == 1, f"expected exit code 1, got {one_peer.returncode}"

    failures.append(_run_case("002.5 non-help validation rejection exits 1", check_002_5))

    failures = [failure for failure in failures if failure]
    if failures:
        for failure in failures:
            print(f"FAIL: {failure}")
        print(f"FAILED: {len(failures)} checks failed.")
        return 1

    print("PASS: all checks for req 002_help-screen passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
