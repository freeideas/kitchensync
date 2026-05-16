#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = [
#   "xxhash>=3.5.0",
# ]
# ///

from __future__ import annotations

import shutil
import sqlite3
import subprocess
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path

import xxhash


PROJECT_DIR = Path("/home/ace/Desktop/prjx/kitchensync")
JAVA = Path("/home/ace/Desktop/prjx/kitchensync/tools/compiler/jdk/bin/java")
JAR = Path("/home/ace/Desktop/prjx/kitchensync/released/kitchensync.jar")
WORK = Path("/home/ace/Desktop/prjx/kitchensync/tests/.tmp/04_retention")
PEER_A = WORK / "peer-a"
PEER_B = WORK / "peer-b"
BASE62 = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"


def clean_start() -> None:
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


def describe_result(result: subprocess.CompletedProcess[str]) -> str:
    return (
        f"exit={result.returncode}\n"
        f"stdout:\n{result.stdout[-2000:]}\n"
        f"stderr:\n{result.stderr[-2000:]}"
    )


def check(failures: list[str], condition: bool, message: str) -> None:
    if not condition:
        failures.append(message)


def check_success(failures: list[str], label: str, result: subprocess.CompletedProcess[str]) -> None:
    check(failures, result.returncode == 0, f"{label} should exit 0; got {describe_result(result)}")


def timestamp(days_from_now: int) -> str:
    value = datetime.now(timezone.utc) + timedelta(days=days_from_now)
    return value.strftime("%Y-%m-%d_%H-%M-%S_") + f"{value.microsecond:06d}Z"


def encode_base62_11(value: int) -> str:
    chars: list[str] = []
    if value == 0:
        chars.append("0")
    while value:
        value, remainder = divmod(value, 62)
        chars.append(BASE62[remainder])
    return "".join(reversed(chars)).rjust(11, "0")


def path_id(relative_path: str) -> str:
    digest = xxhash.xxh64(relative_path.encode("utf-8"), seed=0).intdigest()
    return encode_base62_11(digest)


def snapshot_db(peer: Path) -> Path:
    return peer / ".kitchensync" / "snapshot.db"


def snapshot_ids(peer: Path) -> set[str]:
    con = sqlite3.connect(snapshot_db(peer))
    try:
        return {str(row[0]) for row in con.execute("SELECT id FROM snapshot").fetchall()}
    finally:
        con.close()


def insert_retention_rows(peer: Path, old_time: str, recent_time: str) -> dict[str, str]:
    ids = {
        "old_deleted": path_id("__retention_old_deleted.txt"),
        "old_live": path_id("__retention_old_live.txt"),
        "null_live": path_id("__retention_null_live.txt"),
        "recent_deleted": path_id("__retention_recent_deleted.txt"),
        "recent_live": path_id("__retention_recent_live.txt"),
    }
    con = sqlite3.connect(snapshot_db(peer))
    con.row_factory = sqlite3.Row
    try:
        template = con.execute("SELECT * FROM snapshot LIMIT 1").fetchone()
        if template is None:
            raise RuntimeError("snapshot fixture did not contain a template row")
        columns = list(template.keys())

        rows = [
            ("old_deleted", "__retention_old_deleted.txt", old_time, old_time),
            ("old_live", "__retention_old_live.txt", old_time, None),
            ("null_live", "__retention_null_live.txt", None, None),
            ("recent_deleted", "__retention_recent_deleted.txt", recent_time, recent_time),
            ("recent_live", "__retention_recent_live.txt", recent_time, None),
        ]
        placeholders = ",".join("?" for _ in columns)
        sql = f"INSERT OR REPLACE INTO snapshot ({','.join(columns)}) VALUES ({placeholders})"
        for key, basename, last_seen, deleted_time in rows:
            values = dict(template)
            values.update(
                {
                    "id": ids[key],
                    "parent_id": path_id("/"),
                    "basename": basename,
                    "mod_time": recent_time,
                    "byte_size": 1,
                    "last_seen": last_seen,
                    "deleted_time": deleted_time,
                }
            )
            con.execute(sql, [values[column] for column in columns])
        con.commit()
    finally:
        con.close()
    return ids


def make_retention_dirs(level: Path, old_time: str, recent_time: str) -> dict[str, Path]:
    bak = level / ".kitchensync" / "BAK"
    tmp = level / ".kitchensync" / "TMP"
    paths = {
        "old_bak": bak / old_time,
        "recent_bak": bak / recent_time,
        "old_tmp": tmp / old_time,
        "recent_tmp": tmp / recent_time,
    }
    write_text(paths["old_bak"] / "gone.txt", "old bak\n")
    write_text(paths["recent_bak"] / "kept.txt", "recent bak\n")
    write_text(paths["old_tmp"] / "uuid-old" / "gone.txt", "old tmp\n")
    write_text(paths["recent_tmp"] / "uuid-recent" / "kept.txt", "recent tmp\n")
    return paths


def main() -> int:
    failures: list[str] = []
    clean_start()

    write_text(PEER_A / "root.txt", "root fixture\n")
    write_text(PEER_A / "nested" / "keep.txt", "nested fixture\n")

    setup = run_sync(f"+{PEER_A}", f"-{PEER_B}")
    check_success(failures, "initial retention fixture sync", setup)
    check(failures, snapshot_db(PEER_A).is_file(), "setup should create peer-a snapshot.db")

    if not snapshot_db(PEER_A).is_file():
        print("FAIL")
        for failure in failures:
            print(f"- {failure}")
        return 1

    old_time = timestamp(-40)
    recent_time = timestamp(-1)
    retained_file_id = path_id("root.txt")
    retention_ids = insert_retention_rows(PEER_A, old_time, recent_time)
    level_dirs = {
        "root": make_retention_dirs(PEER_A, old_time, recent_time),
        "nested": make_retention_dirs(PEER_A / "nested", old_time, recent_time),
    }

    result = run_sync("--td", "10", "--bd", "10", "--xd", "10", str(PEER_A), str(PEER_B))
    check_success(failures, "retention sync", result)

    ids_after = snapshot_ids(PEER_A) if snapshot_db(PEER_A).is_file() else set()
    check(
        failures,
        retention_ids["old_deleted"] not in ids_after,
        "04.1 expected startup purge to delete tombstone rows older than --td",
    )
    check(
        failures,
        retention_ids["old_live"] not in ids_after,
        "04.2 expected startup purge to delete live rows with last_seen older than --td",
    )
    check(
        failures,
        retention_ids["null_live"] not in ids_after,
        "04.2 expected startup purge to delete live rows with last_seen IS NULL",
    )
    check(
        failures,
        retention_ids["recent_deleted"] in ids_after,
        "04.1 expected tombstone rows newer than --td to remain",
    )
    check(
        failures,
        retention_ids["recent_live"] in ids_after,
        "04.2 expected live rows newer than --td to remain through startup purge",
    )
    check(
        failures,
        retained_file_id in ids_after,
        "retention purge should not remove an ordinary current snapshot row",
    )

    for level, paths in level_dirs.items():
        check(
            failures,
            not paths["old_bak"].exists(),
            f"04.3 expected stale {level} .kitchensync/BAK timestamp directory to be removed",
        )
        check(
            failures,
            paths["recent_bak"].is_dir(),
            f"04.3 expected fresh {level} .kitchensync/BAK timestamp directory to remain",
        )
        check(
            failures,
            not paths["old_tmp"].exists(),
            f"04.4 expected stale {level} .kitchensync/TMP timestamp directory to be removed",
        )
        check(
            failures,
            paths["recent_tmp"].is_dir(),
            f"04.4 expected fresh {level} .kitchensync/TMP timestamp directory to remain",
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
