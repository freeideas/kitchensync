#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

from __future__ import annotations

import os
import shutil
import subprocess
import tempfile
from pathlib import Path


PROJECT_DIR = Path("/home/ace/Desktop/prjx/kitchensync")
JAVA = Path("/home/ace/Desktop/prjx/kitchensync/tools/compiler/jdk/bin/java")
JAR = Path("/home/ace/Desktop/prjx/kitchensync/released/kitchensync.jar")
BASE = Path(tempfile.gettempdir()) / "kitchensync_03_directory_decisions"


failures: list[str] = []


def record(condition: bool, message: str) -> None:
    if condition:
        print(f"PASS: {message}")
    else:
        print(f"FAIL: {message}")
        failures.append(message)


def peer(path: Path, subordinate: bool = False) -> str:
    text = str(path)
    return f"-{text}" if subordinate else text


def canon_peer(path: Path) -> str:
    return f"+{path}"


def reset(path: Path) -> None:
    shutil.rmtree(path, ignore_errors=True)
    path.mkdir(parents=True, exist_ok=True)


def mkdir(path: Path, mtime: int | None = None) -> None:
    path.mkdir(parents=True, exist_ok=True)
    if mtime is not None:
        os.utime(path, (mtime, mtime))


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")


def run_sync(label: str, *args: str) -> bool:
    try:
        result = subprocess.run(
            [str(JAVA), "-jar", str(JAR), "-vl", "error", *args],
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
    except subprocess.TimeoutExpired as exc:
        failures.append(f"{label}: sync timed out")
        print(f"FAIL: {label}: sync timed out after {exc.timeout}s")
        return False

    if result.returncode != 0:
        failures.append(f"{label}: sync exited {result.returncode}")
        print(f"FAIL: {label}: sync exited {result.returncode}")
        print(f"stdout:\n{result.stdout[-2000:]}")
        print(f"stderr:\n{result.stderr[-2000:]}")
        return False
    else:
        print(f"PASS: {label}: sync exited 0")
        return True


def bak_entries(peer_root: Path, basename: str) -> list[Path]:
    bak = peer_root / ".kitchensync" / "BAK"
    if not bak.exists():
        return []
    return [path for path in bak.rglob(basename) if path.name == basename]


def scenario_live_directory_wins_over_tombstone() -> None:
    root = BASE / "live-wins"
    p1 = root / "p1"
    p2 = root / "p2"
    reset(p1)
    reset(p2)

    dirname = "old-live-dir"
    synced = True
    synced &= run_sync("03.9/03.13 establish empty snapshots", canon_peer(p1), peer(p2))
    mkdir(p1 / dirname)
    synced &= run_sync("03.9/03.13 record live directory on both peers", canon_peer(p1), peer(p2))
    shutil.rmtree(p1 / dirname, ignore_errors=True)
    shutil.rmtree(p2 / dirname, ignore_errors=True)
    synced &= run_sync("03.9/03.13 record tombstones on both peers", peer(p1), peer(p2))

    mkdir(p1 / dirname, mtime=946684800)
    synced &= run_sync("03.9/03.13 old live directory displaces newer tombstones", peer(p1), peer(p2))

    record(synced and (p1 / dirname).is_dir(), "03.13 keeps an old live directory instead of deleting it by directory mod_time")
    record(synced and (p2 / dirname).is_dir(), "03.9 creates a directory on a contributing peer that lacks it")


def scenario_tombstones_delete_remaining_peer_and_no_row_does_not_block() -> None:
    root = BASE / "tombstones-with-no-row"
    p1 = root / "p1"
    p2 = root / "p2"
    p3 = root / "p3"
    p4 = root / "p4"
    for path in (p1, p2, p3, p4):
        reset(path)

    dirname = "deleted-by-all-who-knew"
    marker = "subordinate-only.txt"
    synced = True
    synced &= run_sync("03.10/03.11 establish p3 as contributing peer with no row", canon_peer(p1), peer(p2), peer(p3))
    mkdir(p1 / dirname)
    mkdir(p2 / dirname)
    synced &= run_sync("03.10/03.11 record live row on peers that know the directory", canon_peer(p1), peer(p2))
    shutil.rmtree(p1 / dirname, ignore_errors=True)
    shutil.rmtree(p2 / dirname, ignore_errors=True)
    synced &= run_sync("03.10/03.11 record tombstones on every peer that knew it", peer(p1), peer(p2))

    write_text(p4 / dirname / marker, "preserve this displaced subordinate content\n")
    synced &= run_sync("03.10/03.11 delete remaining subordinate copy", peer(p1), peer(p2), peer(p3), peer(p4, subordinate=True))

    backups = bak_entries(p4, dirname)
    record(synced and not (p4 / dirname).exists(), "03.10 displaces a remaining peer's directory when all peers that knew it tombstoned it")
    record(synced and any((entry / marker).is_file() for entry in backups), "03.10 places the displaced directory under BAK/")
    record(synced and not (p3 / dirname).exists(), "03.11 a contributing peer with no snapshot row does not preserve or create the directory")


def scenario_no_live_and_no_snapshot_row_deletes_subordinate() -> None:
    root = BASE / "no-row-anywhere"
    p1 = root / "p1"
    p2 = root / "p2"
    p3 = root / "p3"
    for path in (p1, p2, p3):
        reset(path)

    dirname = "never-known"
    marker = "subordinate-content.txt"
    seed = "snapshot-history.txt"
    synced = True
    write_text(p1 / seed, "seed snapshot history without creating a row for the tested directory\n")
    synced &= run_sync("03.12 establish contributing snapshots with no directory row", canon_peer(p1), peer(p2))
    write_text(p3 / dirname / marker, "this directory is not in any contributing listing or snapshot\n")
    synced &= run_sync("03.12 delete subordinate directory unknown to contributors", peer(p1), peer(p2), peer(p3, subordinate=True))

    backups = bak_entries(p3, dirname)
    record(synced and not (p3 / dirname).exists(), "03.12 removes a subordinate directory absent from all contributing listings and snapshots")
    record(synced and any((entry / marker).is_file() for entry in backups), "03.12 places the unknown subordinate directory under BAK/")


def main() -> int:
    shutil.rmtree(BASE, ignore_errors=True)
    BASE.mkdir(parents=True, exist_ok=True)

    scenario_live_directory_wins_over_tombstone()
    scenario_tombstones_delete_remaining_peer_and_no_row_does_not_block()
    scenario_no_live_and_no_snapshot_row_deletes_subordinate()

    if failures:
        print("\nFailures:")
        for failure in failures:
            print(f"- {failure}")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
