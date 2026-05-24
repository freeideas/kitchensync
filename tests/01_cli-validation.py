#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

from __future__ import annotations

import shutil
import subprocess
import sys
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

JAVA = Path("C:/Users/human/Desktop/prjx/kitchensync/tools/compiler/jdk/bin/java.exe")
JAR = Path("C:/Users/human/Desktop/prjx/kitchensync/released/kitchensync.jar")
TMP = Path("C:/Users/human/Desktop/prjx/kitchensync/tests/.tmp_01_cli_validation")


def run_cli(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(JAVA), "-jar", str(JAR), *args],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=30,
        check=False,
    )


def check_failure(label: str, result: subprocess.CompletedProcess[str], failures: list[str]) -> None:
    out = result.stdout or ""
    low = out.lower()
    before = len(failures)

    if result.returncode != 1:
        failures.append(f"{label}: exit {result.returncode}, want 1")

    err_pos = low.find("error")
    help_markers = [m for m in ("usage", "--mc", "--ct") if m in low]
    help_pos = min((low.find(m) for m in help_markers), default=-1)

    if err_pos < 0:
        failures.append(f"{label}: no validation error in output")
    if help_pos < 0:
        failures.append(f"{label}: no help text in output")
    elif err_pos >= 0 and err_pos > help_pos:
        failures.append(f"{label}: validation error must appear before help text")

    if len(failures) > before:
        snippet = out[-1500:] if len(out) > 1500 else out
        failures.append(f"{label}: output:\n{snippet}")


def main() -> int:
    # idempotency: reset temp state before creating it
    if TMP.exists():
        shutil.rmtree(TMP)
    TMP.mkdir(parents=True)
    left = TMP / "left"
    right = TMP / "right"
    left.mkdir()
    right.mkdir()
    L = str(left)
    R = str(right)

    failures: list[str] = []

    cases: list[tuple[str, tuple[str, ...]]] = [
        # 01.10: exactly one peer is a validation error (zero args is help invocation, not tested here)
        ("01.10 one-peer", (L,)),
        # 01.11: more than one canon (+) peer is a validation error
        ("01.11 two-canon-peers", (f"+{L}", f"+{R}")),
        # 01.12: unrecognized flags are a validation error
        ("01.12 unknown-flag", ("--no-such-flag", L, R)),
        # 01.14: -vl value outside error/info/debug/trace is a validation error
        ("01.14 -vl=warn", ("-vl", "warn", L, R)),
        ("01.14 -vl=verbose", ("-vl", "verbose", L, R)),
        # Command-line excludes must be relative slash paths.
        ("01.x -x missing value", ("-x",)),
        ("01.x -x absolute", ("-x", "/absolute", L, R)),
        ("01.x -x backslash", ("-x", "bad\\path", L, R)),
        ("01.x -x dotdot", ("-x", "bad/../path", L, R)),
        ("01.x -x trailing slash", ("-x", "bad/path/", L, R)),
    ]

    # 01.13: non-positive-integer values for any numeric option are a validation error
    for opt in ("--mc", "--ct", "--ka", "--xd", "--bd", "--td"):
        cases.append((f"01.13 {opt}=0", (opt, "0", L, R)))
        cases.append((f"01.13 {opt}=-1", (opt, "-1", L, R)))
        cases.append((f"01.13 {opt}=abc", (opt, "abc", L, R)))

    for label, args in cases:
        try:
            result = run_cli(*args)
        except subprocess.TimeoutExpired:
            failures.append(f"{label}: timed out before reporting validation failure")
            continue
        check_failure(label, result, failures)

    if TMP.exists():
        shutil.rmtree(TMP)

    if failures:
        print("\n".join(failures))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
