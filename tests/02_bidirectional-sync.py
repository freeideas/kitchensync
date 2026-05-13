#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""02.6: Bidirectional sync (no + peer) completes and exits 0 when all peers have snapshots."""

from __future__ import annotations

import os, shutil, subprocess, sys
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

TMP = Path(PROJECT) / "tmp" / "testks" / "02_bidirectional-sync"
PEER1 = TMP / "peer1"
PEER2 = TMP / "peer2"


def main() -> int:
    # Idempotent cleanup at start
    if TMP.exists():
        shutil.rmtree(TMP)
    PEER1.mkdir(parents=True)
    PEER2.mkdir(parents=True)

    (PEER1 / "alpha.txt").write_text("alpha")
    (PEER2 / "beta.txt").write_text("beta")

    failures = []

    try:
        url1 = PEER1.resolve().as_uri()
        url2 = PEER2.resolve().as_uri()

        # Establish snapshots on both peers via a first sync with a canon peer.
        setup = subprocess.run(
            [str(UV), "run", "--script", str(BUILD_PY),
             "invoke-cli", PROJECT, "+" + url1, url2],
            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            text=True, encoding="utf-8",
            timeout=60,
        )
        if setup.returncode != 0:
            print(f"[setup] first sync (with +) failed: exit {setup.returncode}")
            print(f"  stdout: {setup.stdout!r}")
            print(f"  stderr: {setup.stderr!r}")
            failures.append("setup: first sync with + failed; cannot test 02.6")
            print("\nFAILURES:")
            for f in failures:
                print(f"  - {f}")
            return 1
        print("[setup] first sync with + succeeded (exit 0), snapshots established")

        # 02.6 — second sync with no + peer must exit 0
        proc = subprocess.run(
            [str(UV), "run", "--script", str(BUILD_PY),
             "invoke-cli", PROJECT, url1, url2],
            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            text=True, encoding="utf-8",
            timeout=60,
        )

        print(f"[02.6] bidirectional sync (no +) exit code: {proc.returncode}")
        if proc.returncode != 0:
            failures.append(
                f"02.6: expected exit 0, got {proc.returncode}\n"
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
