#!/usr/bin/env -S uv run --script
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

PROJECT_DIR = Path("C:/Users/human/Desktop/prjx/kitchensync")
JAVA = PROJECT_DIR / "tools/compiler/jdk/bin/java.exe"
JAR = PROJECT_DIR / "released/kitchensync.jar"
FIXTURE_ROOT = PROJECT_DIR / "tmp" / "test-02-bidirectional-sync"


def run_sync(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(JAVA), "-jar", str(JAR), *args],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=60,
        check=False,
    )


def detail(result: subprocess.CompletedProcess[str]) -> str:
    return (
        f"exit={result.returncode}\n"
        f"--- stdout ---\n{result.stdout}"
        f"--- stderr ---\n{result.stderr}"
    )


def check(failures: list[str], cond: bool, label: str, extra: str = "") -> None:
    if cond:
        print(f"PASS: {label}")
    else:
        print(f"FAIL: {label}")
        if extra:
            print(extra)
        failures.append(label)


def main() -> int:
    failures: list[str] = []

    if FIXTURE_ROOT.exists():
        shutil.rmtree(FIXTURE_ROOT)
    peer_a = FIXTURE_ROOT / "peer_a"
    peer_b = FIXTURE_ROOT / "peer_b"
    peer_a.mkdir(parents=True)
    peer_b.mkdir(parents=True)
    (peer_a / "seed.txt").write_text("seed content\n", encoding="utf-8")

    # Setup: first sync with + to create snapshot.db on both peers.
    setup = run_sync("+" + str(peer_a), str(peer_b))
    if setup.returncode != 0:
        print(f"FAIL: setup (first sync with +) failed -- cannot continue\n{detail(setup)}")
        return 1

    snap_a = peer_a / ".kitchensync" / "snapshot.db"
    snap_b = peer_b / ".kitchensync" / "snapshot.db"
    if not (snap_a.exists() and snap_b.exists()):
        print(
            f"FAIL: setup did not create snapshot.db on both peers "
            f"(peer_a={snap_a.exists()}, peer_b={snap_b.exists()}) -- cannot continue"
        )
        return 1

    # Add a change so the bidirectional run has real work to do.
    (peer_a / "update.txt").write_text("updated\n", encoding="utf-8")

    # REQ 02.6: every reachable peer has snapshot.db, no + peer -- sync exits 0.
    result = run_sync(str(peer_a), str(peer_b))
    check(
        failures,
        result.returncode == 0,
        "02.6: bidirectional sync with all peers snapshotted and no canon peer exits 0",
        detail(result),
    )

    if failures:
        print(f"\n{len(failures)} check(s) failed.")
        return 1
    print("All checks passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
