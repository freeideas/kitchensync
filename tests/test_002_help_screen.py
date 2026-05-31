#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.10"
# dependencies = []
# ///

from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path
from tempfile import TemporaryDirectory


WORKSPACE_ROOT = Path(__file__).resolve().parents[1]
RELEASED_EXE = WORKSPACE_ROOT / "released" / ("kitchensync.exe" if os.name == "nt" else "kitchensync")
HELP_MARKDOWN = WORKSPACE_ROOT / "specs" / "help.md"


def _configure_stdio() -> None:
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    if hasattr(sys.stderr, "reconfigure"):
        sys.stderr.reconfigure(encoding="utf-8", errors="replace")


def _normalize_newlines(text: str) -> str:
    return text.replace("\r\n", "\n").replace("\r", "\n")


def _trim_terminal_newlines(text: str) -> str:
    return _normalize_newlines(text).rstrip("\n")


def _extract_help_text() -> str:
    raw = HELP_MARKDOWN.read_text(encoding="utf-8", errors="replace")
    marker = raw.find("# Help Screen")
    if marker == -1:
        raise RuntimeError(f"Missing '# Help Screen' section in {HELP_MARKDOWN}")

    start_fence = raw.find("```", marker)
    if start_fence == -1:
        raise RuntimeError(f"Missing help text opening fence in {HELP_MARKDOWN}")

    end_fence = raw.find("```", start_fence + 3)
    if end_fence == -1:
        raise RuntimeError(f"Missing help text closing fence in {HELP_MARKDOWN}")

    block = _normalize_newlines(raw[start_fence + 3 : end_fence])
    if block.startswith("\n"):
        block = block[1:]
    return block.rstrip("\n")


def _run_cli(executable: Path, cwd: Path, args: list[str], timeout_seconds: float = 6.0) -> dict[str, object]:
    command = [str(executable), *args]
    try:
        completed = subprocess.run(
            command,
            cwd=str(cwd),
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout_seconds,
            check=False,
        )
        return {
            "command": command,
            "return_code": completed.returncode,
            "stdout": _normalize_newlines(completed.stdout or ""),
            "stderr": _normalize_newlines(completed.stderr or ""),
            "timed_out": False,
            "error": None,
        }
    except subprocess.TimeoutExpired as exc:
        return {
            "command": command,
            "return_code": None,
            "stdout": _normalize_newlines(exc.stdout or ""),
            "stderr": _normalize_newlines(exc.stderr or ""),
            "timed_out": True,
            "error": f"timeout after {timeout_seconds}s",
        }
    except FileNotFoundError as exc:
        return {
            "command": command,
            "return_code": None,
            "stdout": "",
            "stderr": str(exc),
            "timed_out": False,
            "error": str(exc),
        }


def _record_failure(failures: list[str], check_id: str, message: str, run: dict[str, object] | None = None) -> None:
    if run is None:
        failures.append(f"[{check_id}] {message}")
        return

    failures.append(
        f"[{check_id}] {message} | "
        f"return_code={run.get('return_code')}, timed_out={run.get('timed_out')}, "
        f"stdout={run.get('stdout')!r}, stderr={run.get('stderr')!r}, error={run.get('error')!r}"
    )


def _assert(failures: list[str], check_id: str, message: str, condition: bool, run: dict[str, object] | None = None) -> None:
    if not condition:
        _record_failure(failures, check_id, message, run)


def main() -> int:
    _configure_stdio()

    failures: list[str] = []

    if not HELP_MARKDOWN.is_file():
        failures.append(f"[000] Missing help source file: {HELP_MARKDOWN}")
    if not RELEASED_EXE.is_file():
        failures.append(f"[000] Missing released executable: {RELEASED_EXE}")
    if not WORKSPACE_ROOT.is_dir():
        failures.append(f"[000] Workspace root missing: {WORKSPACE_ROOT}")

    if failures:
        for failure in failures:
            print(failure, file=sys.stderr)
        return 1

    try:
        expected_help = _extract_help_text()
    except Exception as exc:  # pragma: no cover
        print(f"Failed to read expected help text from {HELP_MARKDOWN}: {exc}", file=sys.stderr)
        return 1

    with TemporaryDirectory(prefix="kitchensync-002-help-") as temp_root:
        runtime_root = Path(temp_root)

        no_arg_result = _run_cli(RELEASED_EXE, runtime_root, [])
        invalid_result = _run_cli(RELEASED_EXE, runtime_root, ["--does-not-exist"])

        no_arg_stdout = _trim_terminal_newlines(no_arg_result.get("stdout", ""))
        invalid_stdout = _trim_terminal_newlines(invalid_result.get("stdout", ""))
        expected = _trim_terminal_newlines(expected_help)

        # 002.1
        _assert(
            failures,
            "002.1",
            "kitchensync with no arguments must print the exact help block to stdout",
            not no_arg_result.get("timed_out")
            and no_arg_result.get("error") is None
            and no_arg_stdout == expected,
            no_arg_result,
        )

        # 002.2
        _assert(
            failures,
            "002.2",
            "kitchensync with no arguments must exit 0",
            not no_arg_result.get("timed_out") and no_arg_result.get("return_code") == 0,
            no_arg_result,
        )

        # 002.3
        _assert(
            failures,
            "002.3",
            "kitchensync with no arguments must leave stderr empty",
            not no_arg_result.get("timed_out") and no_arg_result.get("stderr", "") == "",
            no_arg_result,
        )

        # 002.4
        _assert(
            failures,
            "002.4",
            "non-help validation failure must append the exact help block to stdout after the error message",
            not invalid_result.get("timed_out")
            and invalid_result.get("error") is None
            and invalid_stdout.endswith(expected)
            and invalid_stdout != expected,
            invalid_result,
        )

        # 002.5
        _assert(
            failures,
            "002.5",
            "non-help validation failure must exit 1",
            not invalid_result.get("timed_out") and invalid_result.get("return_code") == 1,
            invalid_result,
        )

    if failures:
        print("FAIL: test_002_help_screen.py", file=sys.stderr)
        for entry in failures:
            print(f" - {entry}", file=sys.stderr)
        return 1

    print("PASS: test_002_help_screen.py")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
