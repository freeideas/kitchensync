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
from pathlib import Path


PROJECT_DIR = Path(".")
JAVA = Path("tools/compiler/jdk/bin/java")
JAR = Path("released/kitchensync.jar")
TMP = Path("tests/.tmp/02_first-sync")
SUGGESTION = "First sync? Mark the authoritative peer with a leading +"


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


def reset_dir(path: Path) -> None:
    if path.exists():
        shutil.rmtree(path)
    path.mkdir(parents=True)


def combined_output(result: subprocess.CompletedProcess[str]) -> str:
    return result.stdout + result.stderr


def add_check(failures: list[str], condition: bool, message: str) -> None:
    if not condition:
        failures.append(message)


def describe_result(result: subprocess.CompletedProcess[str]) -> str:
    return (
        f"exit={result.returncode}, "
        f"stdout={result.stdout!r}, "
        f"stderr={result.stderr!r}"
    )


def create_empty_snapshot(peer: Path) -> None:
    snapshot = peer / ".kitchensync" / "snapshot.db"
    snapshot.parent.mkdir(parents=True, exist_ok=True)
    with sqlite3.connect(snapshot) as db:
        db.execute(
            """
            CREATE TABLE snapshot (
                id TEXT PRIMARY KEY,
                parent_id TEXT NOT NULL,
                basename TEXT NOT NULL,
                mod_time TEXT NOT NULL,
                byte_size INTEGER NOT NULL,
                last_seen TEXT,
                deleted_time TEXT
            )
            """
        )


def snapshot_row_count(peer: Path) -> int:
    with sqlite3.connect(peer / ".kitchensync" / "snapshot.db") as db:
        return int(db.execute("SELECT COUNT(*) FROM snapshot").fetchone()[0])


def check_first_sync_without_snapshots(failures: list[str]) -> None:
    case = TMP / "no-snapshots"
    peer_a = case / "peer-a"
    peer_b = case / "peer-b"
    reset_dir(peer_a)
    reset_dir(peer_b)
    (peer_a / "from-a.txt").write_text("from peer a\n", encoding="utf-8", newline="\n")
    (peer_b / "from-b.txt").write_text("from peer b\n", encoding="utf-8", newline="\n")

    result = run_cli(str(peer_a), str(peer_b))
    detail = describe_result(result)

    add_check(
        failures,
        result.returncode == 1,
        f"02.1 expected exit 1 when no peer has snapshot history; got {detail}",
    )
    add_check(
        failures,
        SUGGESTION in combined_output(result),
        f"02.1 expected suggestion {SUGGESTION!r}; got {detail}",
    )
    add_check(
        failures,
        not (peer_a / "from-b.txt").exists() and not (peer_b / "from-a.txt").exists(),
        "02.1 expected no file propagation before canon peer is designated",
    )
    add_check(
        failures,
        not (peer_a / ".kitchensync" / "snapshot.db").exists()
        and not (peer_b / ".kitchensync" / "snapshot.db").exists(),
        "02.1 expected no peer snapshots to be written on rejected first sync",
    )


def check_first_sync_with_empty_snapshots(failures: list[str]) -> None:
    case = TMP / "empty-snapshots"
    peer_a = case / "peer-a"
    peer_b = case / "peer-b"
    reset_dir(peer_a)
    reset_dir(peer_b)
    create_empty_snapshot(peer_a)
    create_empty_snapshot(peer_b)

    (peer_a / "after-empty-snapshot-a.txt").write_text(
        "created after empty snapshot\n", encoding="utf-8", newline="\n"
    )
    (peer_b / "after-empty-snapshot-b.txt").write_text(
        "created after empty snapshot\n", encoding="utf-8", newline="\n"
    )

    result = run_cli(str(peer_a), str(peer_b))
    detail = describe_result(result)

    add_check(
        failures,
        result.returncode == 1,
        f"02.2 expected exit 1 when every reachable snapshot has zero rows; got {detail}",
    )
    add_check(
        failures,
        SUGGESTION in combined_output(result),
        f"02.2 expected suggestion {SUGGESTION!r}; got {detail}",
    )
    add_check(
        failures,
        not (peer_a / "after-empty-snapshot-b.txt").exists()
        and not (peer_b / "after-empty-snapshot-a.txt").exists(),
        "02.2 expected no file propagation when zero-row snapshots require a canon peer",
    )
    add_check(
        failures,
        snapshot_row_count(peer_a) == 0 and snapshot_row_count(peer_b) == 0,
        "02.2 expected rejected first sync to leave zero-row snapshots unchanged",
    )


def main() -> int:
    failures: list[str] = []
    reset_dir(TMP)

    check_first_sync_without_snapshots(failures)
    check_first_sync_with_empty_snapshots(failures)

    if failures:
        print("FAIL")
        for failure in failures:
            print(f"- {failure}")
        return 1

    print("PASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
