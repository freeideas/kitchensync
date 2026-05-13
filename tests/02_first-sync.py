#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""02.1: No-snapshot, no-canon-peer invocation prints guidance and exits 1."""

from __future__ import annotations

import os, shutil, subprocess, sys, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

TMP = Path(PROJECT) / "tmp" / "testks" / "02_first-sync"
PEER1 = TMP / "peer1"
PEER2 = TMP / "peer2"


def main() -> int:
    # Idempotent cleanup at start
    if TMP.exists():
        shutil.rmtree(TMP)
    PEER1.mkdir(parents=True)
    PEER2.mkdir(parents=True)

    failures = []

    try:
        peer1_url = PEER1.resolve().as_uri()
        peer2_url = PEER2.resolve().as_uri()

        proc = subprocess.run(
            [str(UV), "run", "--script", str(BUILD_PY),
             "invoke-cli", PROJECT, peer1_url, peer2_url],
            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            text=True, encoding="utf-8",
            timeout=30,
        )

        combined = proc.stdout + proc.stderr
        expected_msg = "First sync? Mark the authoritative peer with a leading +"

        print(f"[02.1a] exit code: {proc.returncode}")
        if proc.returncode != 1:
            failures.append(f"02.1a: expected exit code 1, got {proc.returncode}")

        print(f"[02.1b] output contains guidance message: {expected_msg!r}")
        if expected_msg not in combined:
            failures.append(
                f"02.1b: guidance message not found in output.\n"
                f"  stdout: {proc.stdout!r}\n"
                f"  stderr: {proc.stderr!r}"
            )

    finally:
        shutil.rmtree(TMP, ignore_errors=True)

    if failures:
        print("\nFAILURES:")
        for f in failures:
            print(f"  - {f}")
        return 1
    print("\nAll assertions passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
