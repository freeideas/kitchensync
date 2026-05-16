#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["xxhash==3.5.0"]
# ///

from __future__ import annotations

import os
import re
import shutil
import sqlite3
import subprocess
import sys
import tempfile
from datetime import UTC, datetime
from pathlib import Path

import xxhash


PROJECT_DIR = Path("/home/ace/Desktop/prjx/kitchensync")
JAVA = PROJECT_DIR / "tools/compiler/jdk/bin/java"
JAR = PROJECT_DIR / "released/kitchensync.jar"
BASE = Path(tempfile.gettempdir()) / "kitchensync-test-02-snapshot-db"
PEER_A = BASE / "peer-a"
PEER_B = BASE / "peer-b"
TIMESTAMP_RE = re.compile(r"^\d{4}-\d{2}-\d{2}_\d{2}-\d{2}-\d{2}_\d{6}Z$")
BASE62 = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"

# Not reasonably testable through the public CLI after a completed run:
# - 02.24: a same-filesystem TMP-to-final atomic rename is an operation history,
#   not a durable post-run state.
# - 02.42: whether each product connection enabled SQLite foreign-key enforcement
#   is per-connection state that is not recorded in the database file.
# - 02.49: use of a local temporary working copy is an internal transfer choice;
#   the public result is the uploaded peer snapshot.
# Timestamp monotonicity in 02.40/02.45 is only partly observable because the
# call order is internal. This test checks the durable consequences available
# through the public surface: timestamp format and no reused generated values
# among visible last_seen and BAK/TMP timestamp names from a run.


def run_cli(*args: str) -> subprocess.CompletedProcess[str]:
    env = os.environ.copy()
    env["LC_ALL"] = "C.UTF-8"
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
        env=env,
    )


def check(failures: list[str], condition: bool, message: str) -> None:
    if not condition:
        failures.append(message)


def write_file(path: Path, content: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(content)


def encode_base62(value: int) -> str:
    chars: list[str] = []
    for _ in range(11):
        value, digit = divmod(value, 62)
        chars.append(BASE62[digit])
    return "".join(reversed(chars))


def path_id(relative_path: str) -> str:
    digest = xxhash.xxh64(relative_path.encode("utf-8"), seed=0).intdigest()
    return encode_base62(digest)


def timestamp_from_stat(path: Path) -> str | None:
    try:
        micros_since_epoch = path.stat().st_mtime_ns // 1_000
    except OSError:
        return None
    seconds, micros = divmod(micros_since_epoch, 1_000_000)
    dt = datetime.fromtimestamp(seconds, tz=UTC)
    return f"{dt:%Y-%m-%d_%H-%M-%S}_{micros:06d}Z"


def db_path(peer: Path) -> Path:
    return peer / ".kitchensync" / "snapshot.db"


def connect_snapshot(peer: Path) -> sqlite3.Connection:
    conn = sqlite3.connect(f"file:{db_path(peer).as_posix()}?mode=ro", uri=True)
    conn.row_factory = sqlite3.Row
    return conn


def rows_by_id(peer: Path) -> dict[str, sqlite3.Row]:
    with connect_snapshot(peer) as conn:
        rows = conn.execute("SELECT * FROM snapshot").fetchall()
    return {str(row["id"]): row for row in rows}


def visible_generated_timestamps(peer: Path) -> list[str]:
    values: list[str] = []
    with connect_snapshot(peer) as conn:
        values.extend(
            str(row["last_seen"])
            for row in conn.execute(
                "SELECT last_seen FROM snapshot WHERE deleted_time IS NULL AND last_seen IS NOT NULL"
            )
        )

    for kitchensync in peer.glob("**/.kitchensync"):
        for name in ["BAK", "TMP"]:
            directory = kitchensync / name
            if directory.exists():
                values.extend(path.name for path in directory.iterdir() if path.is_dir())
    return values


def check_process_timestamp_uniqueness(failures: list[str], label: str, peers: list[Path]) -> None:
    values: list[str] = []
    for peer in peers:
        values.extend(visible_generated_timestamps(peer))
    check(
        failures,
        len(values) == len(set(values)),
        f"{label} visible generated timestamps are unique across all peers in the process",
    )


def assert_run_ok(failures: list[str], label: str, result: subprocess.CompletedProcess[str]) -> None:
    if result.returncode != 0:
        failures.append(
            f"{label} exited {result.returncode}; stdout={result.stdout[-2000:]!r}; "
            f"stderr={result.stderr[-2000:]!r}"
        )


def collect_schema_checks(failures: list[str], peer: Path, expected_paths: set[str]) -> None:
    snapshot = db_path(peer)
    check(failures, snapshot.is_file(), f"{peer.name} has .kitchensync/snapshot.db")

    with connect_snapshot(peer) as conn:
        tables = [
            row["name"]
            for row in conn.execute(
                "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'"
            )
        ]
        check(failures, tables == ["snapshot"], f"{peer.name} schema has exactly one table named snapshot, got {tables}")

        table_info = conn.execute("PRAGMA table_info(snapshot)").fetchall()
        columns = {row["name"]: row for row in table_info}
        for column in ["id", "parent_id", "basename", "mod_time", "byte_size", "last_seen", "deleted_time"]:
            check(failures, column in columns, f"{peer.name} snapshot table includes {column}")

        if "id" in columns:
            check(failures, columns["id"]["pk"] == 1, f"{peer.name} snapshot.id is the primary key")
        indexed_columns: set[str] = set()
        for index in conn.execute("PRAGMA index_list(snapshot)").fetchall():
            for info in conn.execute(f"PRAGMA index_info({index['name']})").fetchall():
                indexed_columns.add(str(info["name"]))
        for column in ["parent_id", "last_seen", "deleted_time"]:
            check(failures, column in indexed_columns, f"{peer.name} snapshot has an index on {column}")

        journal_mode = conn.execute("PRAGMA journal_mode").fetchone()[0]
        check(
            failures,
            journal_mode.lower() in {"delete", "truncate", "persist"},
            f"{peer.name} snapshot uses rollback-journal mode, got {journal_mode!r}",
        )

        rows = conn.execute("SELECT * FROM snapshot").fetchall()

    expected_ids = {path_id(path) for path in expected_paths}
    actual_live_ids = {row["id"] for row in rows if row["deleted_time"] is None}
    check(
        failures,
        actual_live_ids == expected_ids,
        f"{peer.name} live snapshot rows are exactly tracked descendants; "
        f"missing ids={sorted(expected_ids - actual_live_ids)}, "
        f"extra ids={sorted(actual_live_ids - expected_ids)}",
    )
    check(failures, path_id("/") not in {row["id"] for row in rows}, f"{peer.name} snapshot has no row for sync root")

    for row in rows:
        rid = row["id"]
        check(failures, isinstance(rid, str) and len(rid) == 11, f"{peer.name} row id {rid!r} is 11 characters")
        check(failures, isinstance(rid, str) and set(rid) <= set(BASE62), f"{peer.name} row id {rid!r} uses base62 alphabet")
        for column in ["basename", "mod_time", "byte_size"]:
            check(failures, row[column] is not None, f"{peer.name} row {rid} has non-null {column}")
        check(
            failures,
            TIMESTAMP_RE.match(row["mod_time"]) is not None,
            f"{peer.name} row {rid} mod_time uses required UTC timestamp format: {row['mod_time']!r}",
        )
        check(
            failures,
            TIMESTAMP_RE.match(row["last_seen"]) is not None,
            f"{peer.name} row {rid} last_seen uses required UTC timestamp format: {row['last_seen']!r}",
        )
        if row["deleted_time"] is not None:
            check(
                failures,
                TIMESTAMP_RE.match(row["deleted_time"]) is not None,
                f"{peer.name} row {rid} deleted_time uses required UTC timestamp format: {row['deleted_time']!r}",
            )

    live_last_seen = [row["last_seen"] for row in rows if row["deleted_time"] is None]
    check(
        failures,
        len(live_last_seen) == len(set(live_last_seen)),
        f"{peer.name} live last_seen timestamps are unique within the run",
    )


def assert_entry(
    failures: list[str],
    peer: Path,
    rows: dict[str, sqlite3.Row],
    relative_path: str,
    basename: str,
    parent_path: str,
    expected_size: int,
) -> None:
    rid = path_id(relative_path)
    row = rows.get(rid)
    check(failures, row is not None, f"snapshot contains row for {relative_path} with id {rid}")
    if row is None:
        return
    expected_parent = path_id(parent_path)
    check(failures, row["parent_id"] == expected_parent, f"{relative_path} parent_id is hash of {parent_path!r}")
    check(failures, row["basename"] == basename, f"{relative_path} basename stores final path component")
    check(failures, row["byte_size"] == expected_size, f"{relative_path} byte_size is {expected_size}")
    entry_path = peer.joinpath(*relative_path.split("/"))
    current_mod_time = timestamp_from_stat(entry_path)
    check(failures, current_mod_time is not None, f"{peer.name} {relative_path} exists on disk")
    if expected_size >= 0:
        check(
            failures,
            row["mod_time"] == current_mod_time,
            f"{peer.name} {relative_path} mod_time matches the current filesystem entry",
        )
    check(failures, row["deleted_time"] is None, f"{relative_path} is live with deleted_time NULL")


def main() -> int:
    failures: list[str] = []
    if BASE.exists():
        shutil.rmtree(BASE)
    PEER_A.mkdir(parents=True)
    PEER_B.mkdir(parents=True)

    write_file(PEER_A / "top.txt", b"top-level\n")
    write_file(PEER_A / "alpha" / "beta.txt", b"nested beta\n")
    write_file(PEER_A / "replace_me" / "nested" / "old.txt", b"old descendant\n")

    first = run_cli("+%s" % PEER_A, "-%s" % PEER_B)
    assert_run_ok(failures, "initial sync", first)

    expected_first_paths = {
        "top.txt",
        "alpha",
        "alpha/beta.txt",
        "replace_me",
        "replace_me/nested",
        "replace_me/nested/old.txt",
    }
    for peer in [PEER_A, PEER_B]:
        collect_schema_checks(failures, peer, expected_first_paths)
        rows = rows_by_id(peer)
        assert_entry(failures, peer, rows, "top.txt", "top.txt", "/", len(b"top-level\n"))
        assert_entry(failures, peer, rows, "alpha", "alpha", "/", -1)
        assert_entry(failures, peer, rows, "alpha/beta.txt", "beta.txt", "alpha", len(b"nested beta\n"))
        assert_entry(failures, peer, rows, "replace_me/nested/old.txt", "old.txt", "replace_me/nested", len(b"old descendant\n"))
    check_process_timestamp_uniqueness(failures, "initial sync", [PEER_A, PEER_B])

    peer_b_replace_me_before = rows_by_id(PEER_B).get(path_id("replace_me"))
    peer_b_replace_me_last_seen_before = peer_b_replace_me_before["last_seen"] if peer_b_replace_me_before is not None else None

    for sidecar in ["snapshot.db-wal", "snapshot.db-shm"]:
        write_file(PEER_A / ".kitchensync" / sidecar, b"not sync state\n")

    shutil.rmtree(PEER_A / "replace_me")
    write_file(PEER_A / "replace_me", b"replacement file\n")

    second = run_cli("+%s" % PEER_A, str(PEER_B))
    assert_run_ok(failures, "directory displacement sync", second)

    for sidecar in ["snapshot.db-wal", "snapshot.db-shm"]:
        check(failures, not (PEER_B / ".kitchensync" / sidecar).exists(), f"{sidecar} was not synced to peer-b")

    expected_second_paths = {"top.txt", "alpha", "alpha/beta.txt", "replace_me"}
    for peer in [PEER_A, PEER_B]:
        collect_schema_checks(failures, peer, expected_second_paths)
        rows = rows_by_id(peer)
        assert_entry(failures, peer, rows, "replace_me", "replace_me", "/", len(b"replacement file\n"))

    peer_b_rows = rows_by_id(PEER_B)
    displaced_ids = [path_id("replace_me/nested"), path_id("replace_me/nested/old.txt")]
    displaced_rows = [peer_b_rows.get(rid) for rid in displaced_ids]
    check(failures, all(row is not None for row in displaced_rows), "peer-b kept snapshot rows for displaced subtree")
    deleted_values = [row["deleted_time"] for row in displaced_rows if row is not None]
    check(failures, all(value is not None for value in deleted_values), "peer-b displaced directory subtree rows were tombstoned")
    check(
        failures,
        bool(deleted_values) and all(value == peer_b_replace_me_last_seen_before for value in deleted_values),
        "peer-b displaced descendants received the displaced entry deletion estimate",
    )

    for peer in [PEER_A, PEER_B]:
        bak_timestamps: list[str] = []
        tmp_timestamps: list[str] = []
        for kitchensync in peer.glob("**/.kitchensync"):
            bak = kitchensync / "BAK"
            if bak.exists():
                bak_timestamps.extend(path.name for path in bak.iterdir() if path.is_dir())
            tmp = kitchensync / "TMP"
            if tmp.exists():
                tmp_timestamps.extend(path.name for path in tmp.iterdir() if path.is_dir())
        for stamp in bak_timestamps + tmp_timestamps:
            check(failures, TIMESTAMP_RE.match(stamp) is not None, f"{peer.name} staging/archive timestamp {stamp!r} has required format")
        check(
            failures,
            len(bak_timestamps + tmp_timestamps) == len(set(bak_timestamps + tmp_timestamps)),
            f"{peer.name} visible BAK/TMP timestamp directories are unique",
        )
    check_process_timestamp_uniqueness(failures, "directory displacement sync", [PEER_A, PEER_B])

    if failures:
        print("FAILURES:")
        for failure in failures:
            print(f"- {failure}")
        return 1

    print("02_snapshot-db checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
