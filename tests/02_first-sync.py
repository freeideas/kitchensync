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

PROJECT_DIR = Path("C:/Users/human/Desktop/prjx/kitchensync")
JAVA = Path("C:/Users/human/Desktop/prjx/kitchensync/tools/compiler/jdk/bin/java.exe")
JAR = Path("C:/Users/human/Desktop/prjx/kitchensync/released/kitchensync.jar")

SUGGESTION = "First sync? Mark the authoritative peer with a leading +"
TEST_ROOT = PROJECT_DIR / "tmp" / "tests" / "02_first-sync"


def run_cli(*peers: Path | str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(JAVA), "-jar", str(JAR), *(str(peer) for peer in peers)],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=60,
        check=False,
    )


def reset_dir(path: Path) -> None:
    if path.exists():
        shutil.rmtree(path)
    path.mkdir(parents=True)


def output_of(result: subprocess.CompletedProcess[str]) -> str:
    return result.stdout + result.stderr


def record(condition: bool, failures: list[str], message: str) -> None:
    if not condition:
        failures.append(message)


def check_refuses_first_sync_without_snapshots(failures: list[str]) -> None:
    root = TEST_ROOT / "no-snapshots"
    peer_a = root / "peer-a"
    peer_b = root / "peer-b"
    reset_dir(peer_a)
    reset_dir(peer_b)
    (peer_a / "only-on-a.txt").write_text("must not be synced\n", encoding="utf-8")

    result = run_cli(peer_a, peer_b)
    combined = output_of(result)

    record(
        result.returncode == 1,
        failures,
        f"02.1 expected exit code 1 without snapshots, got {result.returncode}. "
        f"Output:\n{combined}",
    )
    record(
        SUGGESTION in combined,
        failures,
        f"02.1 expected suggestion {SUGGESTION!r} without snapshots. "
        f"Output:\n{combined}",
    )
    record(
        not (peer_b / "only-on-a.txt").exists(),
        failures,
        "02.1 expected no sync work without snapshots; peer-b received only-on-a.txt.",
    )


def check_refuses_first_sync_with_empty_snapshots(failures: list[str]) -> None:
    root = TEST_ROOT / "empty-snapshots"
    peer_a = root / "peer-a"
    peer_b = root / "peer-b"
    reset_dir(peer_a)
    reset_dir(peer_b)

    # Use the product to create zero-row snapshots: first sync of two empty dirs
    setup = run_cli(f"+{peer_a}", peer_b)
    setup_output = output_of(setup)
    record(
        setup.returncode == 0,
        failures,
        f"02.2 setup expected canon sync of empty peers to exit 0, got "
        f"{setup.returncode}. Output:\n{setup_output}",
    )
    record(
        (peer_a / ".kitchensync" / "snapshot.db").is_file(),
        failures,
        "02.2 setup expected peer-a to have .kitchensync/snapshot.db.",
    )
    record(
        (peer_b / ".kitchensync" / "snapshot.db").is_file(),
        failures,
        "02.2 setup expected peer-b to have .kitchensync/snapshot.db.",
    )

    (peer_a / "created-after-empty-snapshot.txt").write_text(
        "must not be synced\n",
        encoding="utf-8",
    )
    result = run_cli(peer_a, peer_b)
    combined = output_of(result)

    record(
        result.returncode == 1,
        failures,
        f"02.2 expected exit code 1 with empty snapshots, got {result.returncode}. "
        f"Output:\n{combined}",
    )
    record(
        SUGGESTION in combined,
        failures,
        f"02.2 expected suggestion {SUGGESTION!r} with empty snapshots. "
        f"Output:\n{combined}",
    )
    record(
        not (peer_b / "created-after-empty-snapshot.txt").exists(),
        failures,
        "02.2 expected no sync work with empty snapshots; peer-b received "
        "created-after-empty-snapshot.txt.",
    )


def main() -> int:
    failures: list[str] = []
    reset_dir(TEST_ROOT)

    check_refuses_first_sync_without_snapshots(failures)
    check_refuses_first_sync_with_empty_snapshots(failures)

    if failures:
        for index, failure in enumerate(failures, 1):
            print(f"{index}. {failure}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
