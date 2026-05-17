#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

from __future__ import annotations

import os
import shutil
import subprocess
import sys
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

JAVA = Path("C:/Users/human/Desktop/prjx/kitchensync/tools/compiler/jdk/bin/java.exe")
JAR = Path("C:/Users/human/Desktop/prjx/kitchensync/released/kitchensync.jar")
TEST_ROOT = Path("C:/Users/human/Desktop/prjx/kitchensync/tmp/test_03_directory_decisions")


def run_sync(*peers: Path | str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(JAVA), "-jar", str(JAR), *(str(p) for p in peers)],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=60,
        check=False,
    )


def record_sync_failure(
    failures: list[str], label: str, result: subprocess.CompletedProcess[str]
) -> bool:
    if result.returncode == 0:
        return False
    failures.append(
        f"{label}: sync exited {result.returncode}\n"
        f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}"
    )
    return True


def reset_dir(path: Path) -> None:
    if path.exists():
        shutil.rmtree(path)
    path.mkdir(parents=True)


def bak_contains_dir(peer: Path, basename: str) -> bool:
    bak = peer / ".kitchensync" / "BAK"
    if not bak.exists():
        return False
    return any(candidate.is_dir() for candidate in bak.rglob(basename))


def check(condition: bool, failures: list[str], message: str) -> None:
    if not condition:
        failures.append(message)


def scenario_directory_creation(failures: list[str]) -> None:
    # 03.9: any contributing peer has a directory -> create on every peer lacking it.
    root = TEST_ROOT / "creation"
    reset_dir(root)
    peer_a = root / "a"
    peer_b = root / "b"
    peer_a.mkdir()
    peer_b.mkdir()

    # Establish snapshot history so the decision sync does not need a + peer.
    (peer_a / "seed.txt").write_text("seed\n", encoding="utf-8")
    (peer_b / "seed.txt").write_text("seed\n", encoding="utf-8")
    if record_sync_failure(failures, "03.9 baseline", run_sync(f"+{peer_a}", peer_b)):
        return

    (peer_a / "subdir").mkdir()

    if record_sync_failure(failures, "03.9 sync", run_sync(peer_a, peer_b)):
        return

    check(
        (peer_b / "subdir").is_dir(),
        failures,
        "03.9: directory present on one contributing peer was not created on the peer that lacked it.",
    )


def scenario_all_tombstones_displace_subordinate(failures: list[str]) -> None:
    # 03.10/03.11: when every contributing peer with a snapshot row for a directory
    # has a tombstone and none has it live, remaining peers are displaced to BAK/.
    # A contributing peer with no snapshot row for the directory does not block deletion.
    root = TEST_ROOT / "all-tombstones"
    reset_dir(root)
    peer_a = root / "a"
    peer_b = root / "b"
    peer_c = root / "c"
    peer_d = root / "d"
    for peer in (peer_a, peer_b, peer_c, peer_d):
        peer.mkdir()

    dirname = "removed-everywhere"

    # Give peer_d a snapshot with no row for dirname by syncing before dirname is introduced.
    (peer_a / "seed.txt").write_text("seed\n", encoding="utf-8")
    (peer_d / "seed.txt").write_text("seed\n", encoding="utf-8")
    if record_sync_failure(
        failures, "03.11 peer_d seed sync", run_sync(f"+{peer_a}", peer_d)
    ):
        return

    # Baseline: peer_a, peer_b, peer_c all get dirname (peer_d excluded).
    for peer in (peer_a, peer_b, peer_c):
        (peer / dirname).mkdir()
    if record_sync_failure(
        failures, "all-tombstones baseline", run_sync(f"+{peer_a}", peer_b, peer_c)
    ):
        return

    # Delete dirname from peer_a and peer_b to produce tombstones on both.
    shutil.rmtree(peer_a / dirname)
    shutil.rmtree(peer_b / dirname)

    # Decision run: peer_d is contributing (has a snapshot) but has no row for dirname.
    if record_sync_failure(
        failures,
        "all-tombstones decision",
        run_sync(peer_a, peer_b, peer_d, f"-{peer_c}"),
    ):
        return

    check(
        not (peer_c / dirname).exists(),
        failures,
        "03.10/03.11: subordinate peer still has directory after every contributing peer "
        "with a snapshot row deleted it; a contributing peer with no row must not block deletion.",
    )
    check(
        bak_contains_dir(peer_c, dirname),
        failures,
        "03.10: displaced directory was not moved to .kitchensync/BAK.",
    )
    check(
        not (peer_d / dirname).exists(),
        failures,
        "03.11: contributing peer with no snapshot row for the directory should not receive it.",
    )


def scenario_unknown_to_contributors(failures: list[str]) -> None:
    # 03.12: when no contributing peer has the directory live or in any snapshot row,
    # subordinate peers that have it are displaced to BAK/.
    root = TEST_ROOT / "unknown-subordinate"
    reset_dir(root)
    peer_a = root / "a"
    peer_b = root / "b"
    peer_c = root / "c"
    for peer in (peer_a, peer_b, peer_c):
        peer.mkdir()

    # Establish snapshots on contributing peers without dirname.
    (peer_a / "history.txt").write_text("history\n", encoding="utf-8")
    if record_sync_failure(
        failures, "03.12 baseline", run_sync(f"+{peer_a}", peer_b, peer_c)
    ):
        return

    # Create dirname only on the subordinate peer after contributors have snapshots.
    dirname = "subordinate-only"
    (peer_c / dirname).mkdir()

    if record_sync_failure(
        failures, "03.12 decision", run_sync(peer_a, peer_b, f"-{peer_c}")
    ):
        return

    check(
        not (peer_c / dirname).exists(),
        failures,
        "03.12: subordinate-only directory still present even though no contributing "
        "peer had it live or in any snapshot row.",
    )
    check(
        bak_contains_dir(peer_c, dirname),
        failures,
        "03.12: subordinate-only directory was not displaced to .kitchensync/BAK.",
    )


def scenario_mtime_ignored_for_existence(failures: list[str]) -> None:
    # 03.9/03.13: directory mod_time does not govern whether a directory is created;
    # a live directory on one contributing peer wins regardless of its mtime.
    root = TEST_ROOT / "mtime-ignored"
    reset_dir(root)
    peer_a = root / "a"
    peer_b = root / "b"
    peer_a.mkdir()
    peer_b.mkdir()

    dirname = "old-mtime-dir"

    # Baseline: both peers have dirname; snapshot records it.
    (peer_a / dirname).mkdir()
    (peer_b / dirname).mkdir()
    if record_sync_failure(
        failures, "mtime-ignored baseline", run_sync(f"+{peer_a}", peer_b)
    ):
        return

    # Remove dirname from peer_b (creates a tombstone) and set an ancient mtime on
    # peer_a's dirname. Under a mtime-based rule the "newer" deletion would win; under
    # existence-based rules (03.13) the live copy must win.
    shutil.rmtree(peer_b / dirname)
    ancient = 946684800  # 2000-01-01
    os.utime(peer_a / dirname, (ancient, ancient))

    if record_sync_failure(failures, "mtime-ignored decision", run_sync(peer_a, peer_b)):
        return

    check(
        (peer_a / dirname).is_dir(),
        failures,
        "03.13: live directory was displaced even though another peer merely had "
        "a newer deletion record; directory mod_time must not cause deletion to win.",
    )
    check(
        (peer_b / dirname).is_dir(),
        failures,
        "03.9/03.13: directory present on one contributing peer was not re-created on "
        "the peer that deleted it; directory mod_time must not affect existence decisions.",
    )


def main() -> int:
    failures: list[str] = []
    if TEST_ROOT.exists():
        shutil.rmtree(TEST_ROOT)
    TEST_ROOT.mkdir(parents=True)

    scenario_directory_creation(failures)
    scenario_all_tombstones_displace_subordinate(failures)
    scenario_unknown_to_contributors(failures)
    scenario_mtime_ignored_for_existence(failures)

    if failures:
        print("FAIL")
        for index, failure in enumerate(failures, 1):
            print(f"\n{index}. {failure}")
        return 1

    print("PASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
