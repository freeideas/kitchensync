#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

from __future__ import annotations

import shutil
import sqlite3
import subprocess
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")


PROJECT_DIR = Path("C:/Users/human/Desktop/prjx/kitchensync")
JAVA = Path("C:/Users/human/Desktop/prjx/kitchensync/tools/compiler/jdk/bin/java.exe")
JAR = Path("C:/Users/human/Desktop/prjx/kitchensync/released/kitchensync.jar")
WORK = PROJECT_DIR / "tests" / ".tmp" / "04_retention"
PEER_A = WORK / "peer-a"
PEER_B = WORK / "peer-b"


def reset_workspace() -> None:
    if WORK.exists():
        shutil.rmtree(WORK)
    PEER_A.mkdir(parents=True)
    PEER_B.mkdir(parents=True)


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")


def run_sync(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(JAVA), "-jar", str(JAR), *args],
        cwd=str(PROJECT_DIR),
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=60,
        check=False,
    )


def describe(result: subprocess.CompletedProcess[str]) -> str:
    return (
        f"exit={result.returncode}\n"
        f"stdout:\n{result.stdout[-2000:]}\n"
        f"stderr:\n{result.stderr[-2000:]}"
    )


def check(failures: list[str], condition: bool, message: str) -> None:
    if not condition:
        failures.append(message)


def timestamp(days_from_now: int) -> str:
    value = datetime.now(timezone.utc) + timedelta(days=days_from_now)
    return value.strftime("%Y-%m-%d_%H-%M-%S_") + f"{value.microsecond:06d}Z"


def create_snapshot(peer: Path, old_time: str, fresh_time: str) -> None:
    db = peer / ".kitchensync" / "snapshot.db"
    db.parent.mkdir(parents=True, exist_ok=True)
    con = sqlite3.connect(str(db))
    try:
        con.executescript(
            """
            CREATE TABLE snapshot (
                id TEXT PRIMARY KEY,
                parent_id TEXT,
                basename TEXT NOT NULL,
                mod_time TEXT NOT NULL,
                byte_size INTEGER NOT NULL,
                last_seen TEXT,
                deleted_time TEXT
            );
            CREATE INDEX snapshot_parent_id ON snapshot(parent_id);
            CREATE INDEX snapshot_last_seen ON snapshot(last_seen);
            CREATE INDEX snapshot_deleted_time ON snapshot(deleted_time);
            """
        )
        rows = [
            ("00000000001", "00000000000", "expired-tombstone.txt", old_time, 1, old_time, old_time),
            ("00000000002", "00000000000", "fresh-tombstone.txt", fresh_time, 1, fresh_time, fresh_time),
            ("00000000003", "00000000000", "expired-live-row.txt", old_time, 1, old_time, None),
            ("00000000004", "00000000000", "null-last-seen-row.txt", old_time, 1, None, None),
            ("00000000005", "00000000000", "fresh-live-row.txt", fresh_time, 1, fresh_time, None),
        ]
        con.executemany(
            """
            INSERT INTO snapshot
                (id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            """,
            rows,
        )
        con.commit()
    finally:
        con.close()


def snapshot_basenames(peer: Path) -> set[str]:
    con = sqlite3.connect(str(peer / ".kitchensync" / "snapshot.db"))
    try:
        return {str(row[0]) for row in con.execute("SELECT basename FROM snapshot")}
    finally:
        con.close()


def make_retention_dirs(level: Path, old_time: str, fresh_time: str) -> dict[str, Path]:
    bak = level / ".kitchensync" / "BAK"
    tmp = level / ".kitchensync" / "TMP"
    paths = {
        "old_bak": bak / old_time,
        "fresh_bak": bak / fresh_time,
        "old_tmp": tmp / old_time,
        "fresh_tmp": tmp / fresh_time,
    }
    write_text(paths["old_bak"] / "stale.txt", "old bak\n")
    write_text(paths["fresh_bak"] / "kept.txt", "fresh bak\n")
    write_text(paths["old_tmp"] / "uuid-old" / "stale.txt", "old tmp\n")
    write_text(paths["fresh_tmp"] / "uuid-fresh" / "kept.txt", "fresh tmp\n")
    return paths


def main() -> int:
    failures: list[str] = []
    reset_workspace()

    write_text(PEER_A / "root.txt", "root fixture\n")
    write_text(PEER_A / "nested" / "keep.txt", "nested fixture\n")

    old_time = timestamp(-40)
    fresh_time = timestamp(-1)
    create_snapshot(PEER_A, old_time, fresh_time)
    create_snapshot(PEER_B, old_time, fresh_time)
    retention_dirs = {
        "root": make_retention_dirs(PEER_A, old_time, fresh_time),
        "nested": make_retention_dirs(PEER_A / "nested", old_time, fresh_time),
    }

    result = run_sync("--bd", "10", "--xd", "10", "--td", "10", f"+{PEER_A}", str(PEER_B))
    check(failures, result.returncode == 0, f"retention sync should exit 0; got {describe(result)}")

    for peer in (PEER_A, PEER_B):
        if not (peer / ".kitchensync" / "snapshot.db").is_file():
            failures.append(f"retention sync did not leave a readable {peer.name} .kitchensync/snapshot.db")
            continue

        basenames = snapshot_basenames(peer)
        check(
            failures,
            "expired-tombstone.txt" not in basenames,
            f"04.1 {peer.name}: startup should purge tombstone rows whose deleted_time is older than --td days",
        )
        check(
            failures,
            "fresh-tombstone.txt" in basenames,
            f"04.1 {peer.name}: startup should keep tombstone rows whose deleted_time is not older than --td days",
        )
        check(
            failures,
            "expired-live-row.txt" not in basenames,
            f"04.2 {peer.name}: startup should purge live rows whose last_seen is older than --td days",
        )
        check(
            failures,
            "null-last-seen-row.txt" not in basenames,
            f"04.2 {peer.name}: startup should purge live rows whose last_seen is NULL",
        )
        check(
            failures,
            "fresh-live-row.txt" in basenames,
            f"04.2 {peer.name}: startup should keep live rows whose last_seen is not older than --td days",
        )

    for level, paths in retention_dirs.items():
        check(
            failures,
            not paths["old_bak"].exists(),
            f"04.3 multi-tree walk should remove stale {level} .kitchensync/BAK timestamp directory",
        )
        check(
            failures,
            paths["fresh_bak"].is_dir(),
            f"04.3 multi-tree walk should keep fresh {level} .kitchensync/BAK timestamp directory",
        )
        check(
            failures,
            not paths["old_tmp"].exists(),
            f"04.4 multi-tree walk should remove stale {level} .kitchensync/TMP timestamp directory",
        )
        check(
            failures,
            paths["fresh_tmp"].is_dir(),
            f"04.4 multi-tree walk should keep fresh {level} .kitchensync/TMP timestamp directory",
        )

    if failures:
        print("FAIL")
        for failure in failures:
            print(f"- {failure}")
        return 1

    print("PASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
