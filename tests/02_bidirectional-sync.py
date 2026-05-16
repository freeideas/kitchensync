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


PROJECT_DIR = Path("/home/ace/Desktop/prjx/kitchensync")
JAVA = Path("/home/ace/Desktop/prjx/kitchensync/tools/compiler/jdk/bin/java")
JAR = Path("/home/ace/Desktop/prjx/kitchensync/released/kitchensync.jar")
WORK_DIR = PROJECT_DIR / "tests" / ".tmp" / "02_bidirectional_sync"


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")


def run_cli(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(JAVA), "-jar", str(JAR), *args],
        cwd=PROJECT_DIR,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=60,
        check=False,
    )


def prepare_peers() -> tuple[Path, Path]:
    if WORK_DIR.exists():
        shutil.rmtree(WORK_DIR)

    peer_a = WORK_DIR / "peer-a"
    peer_b = WORK_DIR / "peer-b"
    peer_a.mkdir(parents=True)
    peer_b.mkdir(parents=True)
    write_text(peer_a / "recipe.txt", "salt\npepper\n")
    write_text(peer_a / "nested" / "notes.txt", "prep list\n")
    return peer_a, peer_b


def snapshot_path(peer: Path) -> Path:
    return peer / ".kitchensync" / "snapshot.db"


def describe_result(result: subprocess.CompletedProcess[str]) -> str:
    return (
        f"exit={result.returncode} "
        f"stdout={result.stdout!r} "
        f"stderr={result.stderr!r}"
    )


def main() -> int:
    failures: list[str] = []
    peer_a, peer_b = prepare_peers()

    try:
        initial = run_cli(f"+{peer_a}", str(peer_b))
    except Exception as exc:
        failures.append(f"02.6 setup: initial canon sync did not run: {exc!r}")
    else:
        if initial.returncode != 0:
            failures.append(
                "02.6 setup: initial canon sync should establish peer snapshots; "
                f"{describe_result(initial)}"
            )

    snapshots_ready = True
    for peer in (peer_a, peer_b):
        if not snapshot_path(peer).is_file():
            snapshots_ready = False
            failures.append(
                f"02.6 setup: expected existing snapshot at {snapshot_path(peer)} before no-canon sync"
            )

    if snapshots_ready:
        try:
            bidirectional = run_cli(str(peer_a), str(peer_b))
        except Exception as exc:
            failures.append(f"02.6: no-canon sync with existing snapshots did not run: {exc!r}")
        else:
            if bidirectional.returncode != 0:
                failures.append(
                    "02.6: expected sync with existing snapshots and no + peer to exit 0; "
                    f"{describe_result(bidirectional)}"
                )

    if failures:
        print("FAIL tests/02_bidirectional-sync.py")
        for failure in failures:
            print(f"- {failure}")
        return 1

    print("PASS tests/02_bidirectional-sync.py (02.6)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
