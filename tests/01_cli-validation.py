#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

from __future__ import annotations

import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path


PROJECT_DIR = Path("/home/ace/Desktop/prjx/kitchensync")
JAVA = PROJECT_DIR / "tools/compiler/jdk/bin/java"
JAR = PROJECT_DIR / "released/kitchensync.jar"


@dataclass(frozen=True)
class Case:
    req_id: str
    name: str
    args: tuple[str, ...]


CASES = [
    Case("01.10", "fewer than two peers", ("peer-a",)),
    Case("01.11", "multiple canon peers", ("+peer-a", "+peer-b")),
    Case("01.12", "unrecognized flag", ("--definitely-not-a-kitchensync-flag", "peer-a", "peer-b")),
    Case("01.13", "non-positive --mc", ("--mc", "0", "peer-a", "peer-b")),
    Case("01.13", "non-positive --ct", ("--ct", "0", "peer-a", "peer-b")),
    Case("01.13", "non-positive --ka", ("--ka", "0", "peer-a", "peer-b")),
    Case("01.13", "non-positive --xd", ("--xd", "0", "peer-a", "peer-b")),
    Case("01.13", "non-positive --bd", ("--bd", "0", "peer-a", "peer-b")),
    Case("01.13", "non-positive --td", ("--td", "0", "peer-a", "peer-b")),
    Case("01.13", "negative --mc", ("--mc", "-1", "peer-a", "peer-b")),
    Case("01.13", "negative --ct", ("--ct", "-1", "peer-a", "peer-b")),
    Case("01.13", "negative --ka", ("--ka", "-1", "peer-a", "peer-b")),
    Case("01.13", "negative --xd", ("--xd", "-1", "peer-a", "peer-b")),
    Case("01.13", "negative --bd", ("--bd", "-1", "peer-a", "peer-b")),
    Case("01.13", "negative --td", ("--td", "-1", "peer-a", "peer-b")),
    Case("01.13", "non-integer numeric option", ("--mc", "not-an-integer", "peer-a", "peer-b")),
    Case("01.13", "non-integer --ct", ("--ct", "not-an-integer", "peer-a", "peer-b")),
    Case("01.13", "non-integer --ka", ("--ka", "not-an-integer", "peer-a", "peer-b")),
    Case("01.13", "non-integer --xd", ("--xd", "not-an-integer", "peer-a", "peer-b")),
    Case("01.13", "non-integer --bd", ("--bd", "not-an-integer", "peer-a", "peer-b")),
    Case("01.13", "non-integer --td", ("--td", "not-an-integer", "peer-a", "peer-b")),
    Case("01.14", "invalid verbosity", ("-vl", "verbose", "peer-a", "peer-b")),
]


HELP_TOKENS = (
    "--mc",
    "--ct",
    "--ka",
    "-vl",
    "--xd",
    "--bd",
    "--td",
    "error",
    "info",
    "debug",
    "trace",
)


def run_cli(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(JAVA), "-jar", str(JAR), *args],
        cwd=PROJECT_DIR,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=30,
        check=False,
    )


def combined_output(result: subprocess.CompletedProcess[str]) -> str:
    return result.stdout


def has_validation_error(output: str) -> bool:
    lowered = output.lower()
    return "error" in lowered or "invalid" in lowered


def starts_with_validation_error(output: str) -> bool:
    for line in output.splitlines():
        stripped = line.strip().lower()
        if stripped:
            return "error" in stripped or "invalid" in stripped
    return False


def missing_help_tokens(output: str) -> list[str]:
    return [token for token in HELP_TOKENS if token not in output]


def main() -> int:
    failures: list[str] = []

    for case in CASES:
        try:
            result = run_cli(*case.args)
        except Exception as exc:
            failures.append(f"{case.req_id} {case.name}: command failed to run: {exc}")
            continue

        output = combined_output(result)

        if result.returncode != 1:
            failures.append(
                f"{case.req_id} {case.name}: expected exit code 1, got {result.returncode}"
            )

        if not has_validation_error(output):
            failures.append(
                f"{case.req_id} {case.name}: expected validation error text in stdout/stderr"
            )

        missing = missing_help_tokens(output)
        if missing:
            failures.append(
                f"{case.req_id} {case.name}: expected help text; missing tokens: {', '.join(missing)}"
            )

        if not starts_with_validation_error(output):
            failures.append(
                f"{case.req_id} {case.name}: expected validation error before help text"
            )

    if failures:
        print("FAIL tests/01_cli-validation.py")
        for failure in failures:
            print(f"- {failure}")
        return 1

    print(f"PASS tests/01_cli-validation.py ({len(CASES)} checks)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
