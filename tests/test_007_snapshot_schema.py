# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

from __future__ import annotations

import os
import re
import sqlite3
import subprocess
import sys
import tempfile
from pathlib import Path


sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")


WORKSPACE_ROOT = Path("/home/ace/Desktop/prjx/kitchensync")
PRIMARY_EXE = Path("/home/ace/Desktop/prjx/kitchensync/released/kitchensync.exe")
SCRIPT_ROOT = Path(__file__).resolve().parents[1]
RELEASED_EXE = PRIMARY_EXE if PRIMARY_EXE.exists() else SCRIPT_ROOT / "released" / "kitchensync.exe"
TIMESTAMP_RE = re.compile(r"^\d{4}-\d{2}-\d{2}_\d{2}-\d{2}-\d{2}_\d{6}Z$")

BASE62 = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"
MASK64 = (1 << 64) - 1
XXH64_PRIME1 = 11400714785074694791
XXH64_PRIME2 = 14029467366897019727
XXH64_PRIME3 = 1609587929392839161
XXH64_PRIME4 = 9650029242287828579
XXH64_PRIME5 = 2870177450012600261


def rotl64(value: int, bits: int) -> int:
    value &= MASK64
    return ((value << bits) | (value >> (64 - bits))) & MASK64


def xxh64_round(acc: int, lane: int) -> int:
    acc = (acc + lane * XXH64_PRIME2) & MASK64
    acc = rotl64(acc, 31)
    return (acc * XXH64_PRIME1) & MASK64


def xxh64_merge(acc: int, lane_acc: int) -> int:
    acc ^= xxh64_round(0, lane_acc)
    acc = (acc * XXH64_PRIME1 + XXH64_PRIME4) & MASK64
    return acc


def xxh64(data: bytes) -> int:
    offset = 0
    length = len(data)
    if length >= 32:
        v1 = (XXH64_PRIME1 + XXH64_PRIME2) & MASK64
        v2 = XXH64_PRIME2
        v3 = 0
        v4 = (-XXH64_PRIME1) & MASK64
        limit = length - 32
        while offset <= limit:
            v1 = xxh64_round(v1, int.from_bytes(data[offset : offset + 8], "little"))
            offset += 8
            v2 = xxh64_round(v2, int.from_bytes(data[offset : offset + 8], "little"))
            offset += 8
            v3 = xxh64_round(v3, int.from_bytes(data[offset : offset + 8], "little"))
            offset += 8
            v4 = xxh64_round(v4, int.from_bytes(data[offset : offset + 8], "little"))
            offset += 8
        h64 = (rotl64(v1, 1) + rotl64(v2, 7) + rotl64(v3, 12) + rotl64(v4, 18)) & MASK64
        h64 = xxh64_merge(h64, v1)
        h64 = xxh64_merge(h64, v2)
        h64 = xxh64_merge(h64, v3)
        h64 = xxh64_merge(h64, v4)
    else:
        h64 = XXH64_PRIME5

    h64 = (h64 + length) & MASK64

    while offset + 8 <= length:
        lane = int.from_bytes(data[offset : offset + 8], "little")
        h64 ^= xxh64_round(0, lane)
        h64 = (rotl64(h64, 27) * XXH64_PRIME1 + XXH64_PRIME4) & MASK64
        offset += 8

    if offset + 4 <= length:
        lane = int.from_bytes(data[offset : offset + 4], "little")
        h64 ^= (lane * XXH64_PRIME1) & MASK64
        h64 = (rotl64(h64, 23) * XXH64_PRIME2 + XXH64_PRIME3) & MASK64
        offset += 4

    while offset < length:
        h64 ^= (data[offset] * XXH64_PRIME5) & MASK64
        h64 = (rotl64(h64, 11) * XXH64_PRIME1) & MASK64
        offset += 1

    h64 ^= h64 >> 33
    h64 = (h64 * XXH64_PRIME2) & MASK64
    h64 ^= h64 >> 29
    h64 = (h64 * XXH64_PRIME3) & MASK64
    h64 ^= h64 >> 32
    return h64 & MASK64


def path_id(relative_path: str) -> str:
    value = xxh64(relative_path.encode("utf-8"))
    chars = []
    for _ in range(11):
        value, digit = divmod(value, 62)
        chars.append(BASE62[digit])
    return "".join(reversed(chars))


def run_kitchensync(args: list[str], failures: list[str]) -> subprocess.CompletedProcess[str] | None:
    try:
        result = subprocess.run(
            [str(RELEASED_EXE), *args],
            cwd=str(SCRIPT_ROOT),
            text=True,
            encoding="utf-8",
            errors="replace",
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=30,
            check=False,
        )
    except Exception as exc:
        failures.append(f"launch failed for {args}: {exc}")
        return None
    if result.returncode != 0:
        failures.append(
            f"kitchensync exited {result.returncode} for {args}; "
            f"stdout={result.stdout!r}; stderr={result.stderr!r}"
        )
    if result.stderr:
        failures.append(f"kitchensync wrote to stderr for {args}: {result.stderr!r}")
    return result


def add_check(failures: list[str], condition: bool, message: str) -> None:
    if not condition:
        failures.append(message)


def read_rows(db_path: Path, failures: list[str]) -> dict[str, sqlite3.Row]:
    if not db_path.exists():
        failures.append(f"snapshot database does not exist: {db_path}")
        return {}
    connection = sqlite3.connect(str(db_path))
    connection.row_factory = sqlite3.Row
    try:
        return {
            row["basename"]: row
            for row in connection.execute(
                "SELECT id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time "
                "FROM snapshot"
            )
        }
    except sqlite3.Error as exc:
        failures.append(f"could not read snapshot rows from {db_path}: {exc}")
        return {}
    finally:
        connection.close()


def check_schema(db_path: Path, failures: list[str]) -> None:
    connection = sqlite3.connect(str(db_path))
    try:
        table_names = [
            row[0]
            for row in connection.execute(
                "SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name"
            )
        ]
        add_check(failures, table_names == ["snapshot"], f"{db_path} tables were {table_names!r}")

        view_names = [
            row[0]
            for row in connection.execute(
                "SELECT name FROM sqlite_master WHERE type = 'view' ORDER BY name"
            )
        ]
        add_check(failures, view_names == [], f"{db_path} views were {view_names!r}")

        columns = connection.execute("PRAGMA table_info(snapshot)").fetchall()
        observed_names = [column[1] for column in columns]
        expected_names = ["id", "parent_id", "basename", "mod_time", "byte_size", "last_seen", "deleted_time"]
        add_check(failures, observed_names == expected_names, f"{db_path} columns were {observed_names!r}")

        expected = {
            "id": ("TEXT", 0, 1),
            "parent_id": ("TEXT", 0, 0),
            "basename": ("TEXT", 1, 0),
            "mod_time": ("TEXT", 1, 0),
            "byte_size": ("INTEGER", 1, 0),
            "last_seen": ("TEXT", 0, 0),
            "deleted_time": ("TEXT", 0, 0),
        }
        for _, name, column_type, not_null, _, primary_key in columns:
            exp_type, exp_not_null, exp_pk = expected.get(name, ("", -1, -1))
            add_check(failures, column_type.upper() == exp_type, f"{db_path} {name} type was {column_type!r}")
            add_check(failures, not_null == exp_not_null, f"{db_path} {name} notnull was {not_null}")
            add_check(failures, primary_key == exp_pk, f"{db_path} {name} primary-key status was {primary_key}")

        indexed_columns: set[str] = set()
        for index in connection.execute("PRAGMA index_list(snapshot)").fetchall():
            index_name = index[1]
            for index_column in connection.execute(f"PRAGMA index_info({index_name!r})").fetchall():
                indexed_columns.add(index_column[2])
        for column_name in ("parent_id", "last_seen", "deleted_time"):
            add_check(failures, column_name in indexed_columns, f"{db_path} has no index on {column_name}")
    finally:
        connection.close()


def check_common_rows(db_path: Path, rows: dict[str, sqlite3.Row], failures: list[str]) -> None:
    root_id = path_id("/")
    expected = {
        "alpha.txt": ("alpha.txt", root_id, len("alpha")),
        "docs": ("docs", root_id, -1),
        "readme.txt": ("docs/readme.txt", path_id("docs"), len("readme")),
    }
    for basename, (relative_path, expected_parent, expected_size) in expected.items():
        row = rows.get(basename)
        add_check(failures, row is not None, f"{db_path} missing row for {relative_path}")
        if row is None:
            continue
        add_check(failures, row["id"] == path_id(relative_path), f"{db_path} wrong id for {relative_path}: {row['id']!r}")
        add_check(
            failures,
            isinstance(row["id"], str) and len(row["id"]) == 11 and all(ch in BASE62 for ch in row["id"]),
            f"{db_path} id for {relative_path} is not 11-character base62: {row['id']!r}",
        )
        add_check(failures, row["parent_id"] == expected_parent, f"{db_path} wrong parent_id for {relative_path}")
        add_check(failures, row["basename"] == basename, f"{db_path} wrong basename for {relative_path}")
        add_check(failures, row["byte_size"] == expected_size, f"{db_path} wrong byte_size for {relative_path}")
        add_check(failures, row["mod_time"] and TIMESTAMP_RE.match(row["mod_time"]), f"{db_path} bad mod_time for {relative_path}: {row['mod_time']!r}")
        add_check(failures, row["last_seen"] and TIMESTAMP_RE.match(row["last_seen"]), f"{db_path} bad last_seen for {relative_path}: {row['last_seen']!r}")
        add_check(failures, row["deleted_time"] is None, f"{db_path} live row {relative_path} has deleted_time {row['deleted_time']!r}")

    all_ids = {row["id"] for row in rows.values()}
    add_check(failures, root_id not in all_ids, f"{db_path} contains a row for the sync root")


def check_tombstone(db_path: Path, failures: list[str]) -> None:
    connection = sqlite3.connect(str(db_path))
    connection.row_factory = sqlite3.Row
    try:
        row = connection.execute(
            "SELECT id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time "
            "FROM snapshot WHERE basename = 'gone.txt'"
        ).fetchone()
    finally:
        connection.close()

    add_check(failures, row is not None, f"{db_path} missing tombstone row for gone.txt")
    if row is None:
        return
    add_check(failures, row["id"] == path_id("gone.txt"), f"{db_path} wrong tombstone id: {row['id']!r}")
    add_check(failures, row["parent_id"] == path_id("/"), f"{db_path} wrong tombstone parent_id: {row['parent_id']!r}")
    add_check(failures, row["basename"] == "gone.txt", f"{db_path} wrong tombstone basename: {row['basename']!r}")
    add_check(failures, row["byte_size"] == len("gone"), f"{db_path} wrong tombstone byte_size: {row['byte_size']}")
    add_check(failures, row["deleted_time"] is not None, f"{db_path} tombstone deleted_time is NULL")
    add_check(
        failures,
        row["deleted_time"] == row["last_seen"],
        f"{db_path} tombstone deleted_time {row['deleted_time']!r} did not copy last_seen {row['last_seen']!r}",
    )
    add_check(failures, TIMESTAMP_RE.match(row["deleted_time"] or ""), f"{db_path} tombstone deleted_time has bad format")


def main() -> int:
    failures: list[str] = []
    # not reasonably testable: 007.12 exact source of the observed filesystem mod_time.
    # not reasonably testable: 007.18 exact source of the generated last_seen timestamp.
    # not reasonably testable: 007.19 incomplete-copy NULL last_seen without forcing a failed/interrupted copy.

    with tempfile.TemporaryDirectory(prefix="kitchensync_007_") as temp_name:
        temp_root = Path(temp_name)
        peer_a = temp_root / "peer_a"
        peer_b = temp_root / "peer_b"
        peer_a.mkdir()
        peer_b.mkdir()

        (peer_a / "docs").mkdir()
        (peer_a / "alpha.txt").write_text("alpha", encoding="utf-8", newline="")
        (peer_a / "gone.txt").write_text("gone", encoding="utf-8", newline="")
        (peer_a / "docs" / "readme.txt").write_text("readme", encoding="utf-8", newline="")
        os.utime(peer_a / "alpha.txt", (1_700_000_000, 1_700_000_000))
        os.utime(peer_a / "gone.txt", (1_700_000_010, 1_700_000_010))
        os.utime(peer_a / "docs" / "readme.txt", (1_700_000_020, 1_700_000_020))

        run_kitchensync([f"+{peer_a}", str(peer_b)], failures)

        db_a = peer_a / ".kitchensync" / "snapshot.db"
        db_b = peer_b / ".kitchensync" / "snapshot.db"
        for db_path in (db_a, db_b):
            if db_path.exists():
                check_schema(db_path, failures)
                check_common_rows(db_path, read_rows(db_path, failures), failures)

        (peer_a / "gone.txt").unlink()
        run_kitchensync([f"+{peer_a}", str(peer_b)], failures)
        for db_path in (db_a, db_b):
            if db_path.exists():
                check_tombstone(db_path, failures)

    if failures:
        print("test_007_snapshot_schema failed:")
        for failure in failures:
            print(f"- {failure}")
        return 1

    print("test_007_snapshot_schema passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
