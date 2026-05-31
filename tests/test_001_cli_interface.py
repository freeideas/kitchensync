#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.10"
# dependencies = []
# ///

from pathlib import Path
import os
import shutil
import subprocess
import sys
import tempfile


def _configure_stdio() -> None:
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    if hasattr(sys.stderr, "reconfigure"):
        sys.stderr.reconfigure(encoding="utf-8", errors="replace")


_WORKSPACE_HINT = Path(r"C:\Users\human\Desktop\prjx\kitchensync")
_PROJECT_DIR_HINT = Path(r"C:\Users\human\Desktop\prjx\kitchensync\proj")
_RELEASED_EXE_HINT_WIN = Path(r"C:\Users\human\Desktop\prjx\kitchensync\released\kitchensync.exe")
_RELEASED_EXE_HINT_UNIX = Path(r"C:\Users\human\Desktop\prjx\kitchensync\released\kitchensync")


def _resolve_workspace_root() -> Path:
    return _WORKSPACE_HINT if _WORKSPACE_HINT.is_dir() else Path(__file__).resolve().parents[1]


def _resolve_executable() -> Path:
    if os.name == "nt":
        if _RELEASED_EXE_HINT_WIN.is_file():
            return _RELEASED_EXE_HINT_WIN
        return _resolve_workspace_root() / "released" / "kitchensync.exe"
    if _RELEASED_EXE_HINT_UNIX.is_file():
        return _RELEASED_EXE_HINT_UNIX
    return _resolve_workspace_root() / "released" / "kitchensync"


def _short_text(value: str, limit: int = 600) -> str:
    value = value.replace("\x00", "\\x00")
    if len(value) > limit:
        return f"{value[:limit]}..."
    return value


def _run_cli(executable: Path, cwd: Path, args: list[str]) -> dict[str, object]:
    if not executable.is_file():
        return {
            "return_code": None,
            "stdout": "",
            "stderr": f"Executable not found: {executable}",
            "timed_out": False,
        }

    cmd = [str(executable), *args]
    try:
        completed = subprocess.run(
            cmd,
            cwd=str(cwd),
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=6,
            check=False,
        )
        return {
            "return_code": completed.returncode,
            "stdout": completed.stdout or "",
            "stderr": completed.stderr or "",
            "timed_out": False,
            "command": cmd,
        }
    except subprocess.TimeoutExpired as exc:
        return {
            "return_code": None,
            "stdout": exc.stdout or "",
            "stderr": exc.stderr or "",
            "timed_out": True,
            "command": cmd,
            "error": f"timeout after {exc.timeout}s",
        }


def _record(failures: list[str], check_id: str, message: str, run: dict[str, object] | None = None) -> None:
    if run is None:
        failures.append(f"[{check_id}] {message}")
        return

    stdout = _short_text(str(run.get("stdout", "")))
    stderr = _short_text(str(run.get("stderr", "")))
    code = run.get("return_code")
    timed_out = run.get("timed_out")
    failures.append(
        f"[{check_id}] {message} | return_code={code}, timed_out={timed_out}, "
        f"stdout={stdout!s}, stderr={stderr!s}"
    )


def _assert(condition: bool, failures: list[str], check_id: str, message: str, run: dict[str, object] | None = None) -> None:
    if not condition:
        _record(failures, check_id, message, run)


def main() -> int:
    _configure_stdio()

    workspace_root = _resolve_workspace_root()
    executable = _resolve_executable()

    failures: list[str] = []

    # Not reasonably testable: 001.45 (cannot pass NUL bytes in subprocess arguments portably)

    # Keep runtime behavior simple and deterministic:
    # - build a fresh, local temporary test fixture once
    # - reuse it for all cases (this is CLI validation-only and keeps runtime short)
    temp_root = Path(tempfile.mkdtemp(prefix="kitchensync-cli-001-"))
    peer_a = "peer-left"
    peer_b = "peer-right"
    (temp_root / peer_a).mkdir(parents=True, exist_ok=True)
    (temp_root / peer_b).mkdir(parents=True, exist_ok=True)

    # 001.1
    expected_name = "kitchensync.exe" if os.name == "nt" else "kitchensync"
    _assert(
        executable.name == expected_name,
        failures,
        "001.1",
        (
            f"released executable must be named {expected_name}, "
            f"actual name is {executable.name!r}"
        ),
        {"return_code": "N/A", "stdout": "", "stderr": "", "timed_out": False},
    )
    _assert(
        executable.is_file(),
        failures,
        "001.1",
        f"released executable must exist at {executable}",
    )

    # 001.2
    result = _run_cli(executable, temp_root, ["--dry-run", peer_a, peer_b])
    _assert(
        not result.get("timed_out") and result.get("return_code") in {0, 1},
        failures,
        "001.2",
        "non-help invocation should accept <peer> <peer> form",
        result,
    )

    # 001.3
    result = _run_cli(executable, temp_root, ["--dry-run", peer_a])
    _assert(
        not result.get("timed_out") and result.get("return_code") == 1,
        failures,
        "001.3",
        "non-help invocation with fewer than two peers should be rejected",
        result,
    )

    # 001.4
    result = _run_cli(executable, temp_root, ["--dry-run", f"+{peer_a}", f"+{peer_b}"])
    _assert(
        not result.get("timed_out") and result.get("return_code") == 1,
        failures,
        "001.4",
        "non-help invocation should reject more than one '+' peer",
        result,
    )

    # 001.5
    result = _run_cli(executable, temp_root, ["--dry-run", peer_a, peer_b])
    _assert(
        not result.get("timed_out") and result.get("return_code") in {0, 1},
        failures,
        "001.5",
        "--dry-run should be accepted as a global flag without value",
        result,
    )

    positive_integer_options = [
        ("001.6", "--max-copies", "3"),
        ("001.9", "--retries-copy", "3"),
        ("001.12", "--retries-list", "3"),
        ("001.15", "--timeout-conn", "30"),
        ("001.18", "--timeout-idle", "30"),
        ("001.21", "--keep-tmp-days", "2"),
        ("001.24", "--keep-bak-days", "2"),
        ("001.27", "--keep-del-days", "2"),
    ]
    for check_id, flag, value in positive_integer_options:
        result = _run_cli(executable, temp_root, ["--dry-run", flag, value, peer_a, peer_b])
        _assert(
            not result.get("timed_out") and result.get("return_code") in {0, 1},
            failures,
            check_id,
            f"{flag} should accept a positive integer value",
            result,
        )

    non_positive_integer_options = [
        ("001.7", "--max-copies", "0"),
        ("001.10", "--retries-copy", "0"),
        ("001.13", "--retries-list", "0"),
        ("001.16", "--timeout-conn", "0"),
        ("001.19", "--timeout-idle", "0"),
        ("001.22", "--keep-tmp-days", "0"),
        ("001.25", "--keep-bak-days", "0"),
        ("001.28", "--keep-del-days", "0"),
    ]
    for check_id, flag, value in non_positive_integer_options:
        result = _run_cli(executable, temp_root, ["--dry-run", flag, value, peer_a, peer_b])
        _assert(
            not result.get("timed_out") and result.get("return_code") == 1,
            failures,
            check_id,
            f"{flag} should reject a non-positive value",
            result,
        )

    non_integer_options = [
        ("001.8", "--max-copies", "two"),
        ("001.11", "--retries-copy", "three"),
        ("001.14", "--retries-list", "three"),
        ("001.17", "--timeout-conn", "abc"),
        ("001.20", "--timeout-idle", "abc"),
        ("001.23", "--keep-tmp-days", "abc"),
        ("001.26", "--keep-bak-days", "abc"),
        ("001.29", "--keep-del-days", "abc"),
    ]
    for check_id, flag, value in non_integer_options:
        result = _run_cli(executable, temp_root, ["--dry-run", flag, value, peer_a, peer_b])
        _assert(
            not result.get("timed_out") and result.get("return_code") == 1,
            failures,
            check_id,
            f"{flag} should reject non-integer values",
            result,
        )

    # 001.30 - 001.34
    for check_id, level in {
        "001.30": "error",
        "001.31": "info",
        "001.32": "debug",
        "001.33": "trace",
    }.items():
        result = _run_cli(executable, temp_root, ["--dry-run", "--verbosity", level, peer_a, peer_b])
        _assert(
            not result.get("timed_out") and result.get("return_code") in {0, 1},
            failures,
            check_id,
            f"--verbosity {level} should be accepted",
            result,
        )

    result = _run_cli(executable, temp_root, ["--dry-run", "--verbosity", "verbose", peer_a, peer_b])
    _assert(
        not result.get("timed_out") and result.get("return_code") == 1,
        failures,
        "001.34",
        "--verbosity should reject values outside error/info/debug/trace",
        result,
    )

    # 001.35
    result = _run_cli(executable, temp_root, ["--dry-run", "--does-not-exist", peer_a, peer_b])
    _assert(
        not result.get("timed_out") and result.get("return_code") == 1,
        failures,
        "001.35",
        "unrecognized flags should be rejected",
        result,
    )

    # 001.36
    result = _run_cli(executable, temp_root, ["--dry-run", "-x", "logs", peer_a, peer_b])
    _assert(
        not result.get("timed_out") and result.get("return_code") in {0, 1},
        failures,
        "001.36",
        "-x should accept a single-segment relative path",
        result,
    )

    # 001.37
    result = _run_cli(executable, temp_root, ["--dry-run", "-x", "foo/bar", peer_a, peer_b])
    _assert(
        not result.get("timed_out") and result.get("return_code") in {0, 1},
        failures,
        "001.37",
        "-x should accept slash-separated multi-segment paths",
        result,
    )

    # 001.38
    result = _run_cli(
        executable,
        temp_root,
        ["--dry-run", "-x", "logs", "-x", "tmp/cache", peer_a, peer_b],
    )
    _assert(
        not result.get("timed_out") and result.get("return_code") in {0, 1},
        failures,
        "001.38",
        "-x should be repeatable",
        result,
    )

    exclude_rejections = [
        ("001.39", "-x", "/logs"),
        ("001.40", "-x", "logs/"),
        ("001.41", "-x", "logs\\cache"),
        ("001.42", "-x", "foo//cache"),
        ("001.43", "-x", "foo/./cache"),
        ("001.44", "-x", "foo/../cache"),
    ]
    for check_id, flag, path_value in exclude_rejections:
        result = _run_cli(executable, temp_root, ["--dry-run", flag, path_value, peer_a, peer_b])
        _assert(
            not result.get("timed_out") and result.get("return_code") == 1,
            failures,
            check_id,
            f"{flag} should reject relative path value {path_value!r}",
            result,
        )

    # 001.46
    result = _run_cli(executable, temp_root, ["--dry-run", "--max-copies", "0", peer_a, peer_b])
    _assert(
        not result.get("timed_out") and result.get("return_code") == 1,
        failures,
        "001.46",
        "non-help argument validation errors should exit with code 1",
        result,
    )

    # 001.47
    value_taking_options = [
        "--max-copies",
        "--retries-copy",
        "--retries-list",
        "--timeout-conn",
        "--timeout-idle",
        "--keep-tmp-days",
        "--keep-bak-days",
        "--keep-del-days",
        "--verbosity",
    ]
    for flag in value_taking_options:
        result = _run_cli(executable, temp_root, ["--dry-run", peer_a, peer_b, flag])
        _assert(
            not result.get("timed_out") and result.get("return_code") == 1,
            failures,
            "001.47",
            f"{flag} should be rejected when value is omitted",
            result,
        )

    # 001.48
    result = _run_cli(executable, temp_root, ["--dry-run", peer_a, peer_b, "-x", "foo/bar"])
    _assert(
        not result.get("timed_out") and result.get("return_code") in {0, 1},
        failures,
        "001.48",
        "-x should be accepted after peer operands",
        result,
    )

    # Defensive idempotency cleanup at start of test fixture: remove temp fixture path if it exists.
    shutil.rmtree(temp_root, ignore_errors=True)

    if failures:
        print("kitchensync CLI interface requirements failed:", file=sys.stderr)
        for item in failures:
            print(f"- {item}", file=sys.stderr)
        return 1

    print("kitchensync CLI interface requirements passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
