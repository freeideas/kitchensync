# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

import os
import re
import shutil
import sqlite3
import subprocess
import sys
import tempfile
from pathlib import Path


sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")


WORKSPACE_ROOT = Path("/home/ace/Desktop/prjx/kitchensync")
KITCHENSYNC_EXE = WORKSPACE_ROOT / "released" / "kitchensync.exe"
TIMESTAMP_RE = re.compile(r"^\d{4}-\d{2}-\d{2}_\d{2}-\d{2}-\d{2}_\d{6}Z$")
BASE62 = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"
MASK64 = 0xFFFFFFFFFFFFFFFF


def xxhash64(data: bytes) -> int:
    prime1 = 11400714785074694791
    prime2 = 14029467366897019727
    prime3 = 1609587929392839161
    prime4 = 9650029242287828579
    prime5 = 2870177450012600261

    def rotl(value: int, count: int) -> int:
        return ((value << count) | (value >> (64 - count))) & MASK64

    def round_acc(acc: int, lane: int) -> int:
        acc = (acc + lane * prime2) & MASK64
        acc = rotl(acc, 31)
        acc = (acc * prime1) & MASK64
        return acc

    def merge_round(acc: int, lane: int) -> int:
        acc ^= round_acc(0, lane)
        acc = (acc * prime1 + prime4) & MASK64
        return acc

    length = len(data)
    offset = 0
    if length >= 32:
        v1 = (prime1 + prime2) & MASK64
        v2 = prime2
        v3 = 0
        v4 = (-prime1) & MASK64
        limit = length - 32
        while offset <= limit:
            v1 = round_acc(v1, int.from_bytes(data[offset : offset + 8], "little"))
            offset += 8
            v2 = round_acc(v2, int.from_bytes(data[offset : offset + 8], "little"))
            offset += 8
            v3 = round_acc(v3, int.from_bytes(data[offset : offset + 8], "little"))
            offset += 8
            v4 = round_acc(v4, int.from_bytes(data[offset : offset + 8], "little"))
            offset += 8
        h64 = (
            rotl(v1, 1)
            + rotl(v2, 7)
            + rotl(v3, 12)
            + rotl(v4, 18)
        ) & MASK64
        h64 = merge_round(h64, v1)
        h64 = merge_round(h64, v2)
        h64 = merge_round(h64, v3)
        h64 = merge_round(h64, v4)
    else:
        h64 = prime5

    h64 = (h64 + length) & MASK64

    while offset + 8 <= length:
        lane = int.from_bytes(data[offset : offset + 8], "little")
        h64 ^= round_acc(0, lane)
        h64 = (rotl(h64, 27) * prime1 + prime4) & MASK64
        offset += 8

    if offset + 4 <= length:
        lane = int.from_bytes(data[offset : offset + 4], "little")
        h64 ^= (lane * prime1) & MASK64
        h64 = (rotl(h64, 23) * prime2 + prime3) & MASK64
        offset += 4

    while offset < length:
        h64 ^= (data[offset] * prime5) & MASK64
        h64 = (rotl(h64, 11) * prime1) & MASK64
        offset += 1

    h64 ^= h64 >> 33
    h64 = (h64 * prime2) & MASK64
    h64 ^= h64 >> 29
    h64 = (h64 * prime3) & MASK64
    h64 ^= h64 >> 32
    return h64 & MASK64


def path_id(relative_path: str) -> str:
    value = xxhash64(relative_path.encode("utf-8"))
    chars = []
    for _ in range(11):
        value, remainder = divmod(value, 62)
        chars.append(BASE62[remainder])
    return "".join(reversed(chars))


def run_sync(failures: list[str], *args: str) -> subprocess.CompletedProcess[str] | None:
    try:
        result = subprocess.run(
            [str(KITCHENSYNC_EXE), *args],
            cwd=str(WORKSPACE_ROOT),
            text=True,
            encoding="utf-8",
            errors="replace",
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=30,
            check=False,
        )
    except subprocess.TimeoutExpired:
        failures.append(f"kitchensync timed out for arguments: {args!r}")
        return None
    except OSError as exc:
        failures.append(f"failed to launch kitchensync: {exc}")
        return None

    if result.returncode != 0:
        failures.append(
            f"kitchensync exited {result.returncode} for {args!r}; "
            f"stdout={result.stdout!r}; stderr={result.stderr!r}"
        )
    if result.stderr != "":
        failures.append(f"kitchensync wrote to stderr for {args!r}: {result.stderr!r}")
    if "sync complete" not in result.stdout.splitlines():
        failures.append(f"kitchensync stdout did not contain a sync complete line: {result.stdout!r}")
    return result


def fetch_schema(db_path: Path) -> tuple[list[tuple], list[tuple], list[tuple], list[tuple]]:
    with sqlite3.connect(str(db_path)) as conn:
        tables = conn.execute(
            "SELECT name FROM sqlite_schema WHERE type = 'table' ORDER BY name"
        ).fetchall()
        views = conn.execute(
            "SELECT name FROM sqlite_schema WHERE type = 'view' ORDER BY name"
        ).fetchall()
        columns = conn.execute("PRAGMA table_info(snapshot)").fetchall()
        indexes = conn.execute("PRAGMA index_list(snapshot)").fetchall()
    return tables, views, columns, indexes


def fetch_rows(db_path: Path) -> dict[str, dict[str, object]]:
    with sqlite3.connect(str(db_path)) as conn:
        conn.row_factory = sqlite3.Row
        rows = conn.execute(
            """
            SELECT id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time
            FROM snapshot
            """
        ).fetchall()
    return {str(row["basename"]): dict(row) for row in rows}


def check(condition: bool, failures: list[str], message: str) -> None:
    if not condition:
        failures.append(message)


def check_timestamp(value: object, failures: list[str], label: str) -> None:
    check(isinstance(value, str) and TIMESTAMP_RE.match(value) is not None, failures, label)


def check_schema(db_path: Path, failures: list[str]) -> None:
    tables, views, columns, indexes = fetch_schema(db_path)

    check(tables == [("snapshot",)], failures, f"{db_path}: expected exactly one snapshot table, got {tables!r}")
    check(views == [], failures, f"{db_path}: expected no SQLite views, got {views!r}")

    expected_columns = [
        ("id", "TEXT"),
        ("parent_id", "TEXT"),
        ("basename", "TEXT"),
        ("mod_time", "TEXT"),
        ("byte_size", "INTEGER"),
        ("last_seen", "TEXT"),
        ("deleted_time", "TEXT"),
    ]
    observed_columns = [(row[1], row[2].upper()) for row in columns]
    check(
        observed_columns == expected_columns,
        failures,
        f"{db_path}: snapshot columns were {observed_columns!r}",
    )

    column_by_name = {row[1]: row for row in columns}
    check(column_by_name.get("id", [None] * 6)[5] == 1, failures, f"{db_path}: id is not the primary key")
    for name in ("basename", "mod_time", "byte_size"):
        check(
            column_by_name.get(name, [None, None, None, 0])[3] == 1,
            failures,
            f"{db_path}: {name} is not marked NOT NULL",
        )
    for name in ("last_seen", "deleted_time"):
        check(
            column_by_name.get(name, [None, None, None, 1])[3] == 0,
            failures,
            f"{db_path}: {name} should allow NULL",
        )

    indexed_columns = set()
    with sqlite3.connect(str(db_path)) as conn:
        for index in indexes:
            index_name = index[1]
            for indexed_column in conn.execute(f"PRAGMA index_info({index_name})").fetchall():
                indexed_columns.add(indexed_column[2])
    for name in ("parent_id", "last_seen", "deleted_time"):
        check(name in indexed_columns, failures, f"{db_path}: missing index on {name}")


def check_initial_rows(db_path: Path, failures: list[str]) -> dict[str, dict[str, object]]:
    rows_by_basename = fetch_rows(db_path)
    expected_ids = {
        "rootfile.txt": path_id("rootfile.txt"),
        "folder": path_id("folder"),
        "nested.bin": path_id("folder/nested.bin"),
    }
    root_id = path_id("/")
    folder_id = path_id("folder")

    check(
        {row["id"] for row in rows_by_basename.values()} == set(expected_ids.values()),
        failures,
        f"{db_path}: rows should represent only tracked children below the sync root",
    )
    check(root_id not in {row["id"] for row in rows_by_basename.values()}, failures, f"{db_path}: sync root has a snapshot row")

    for basename, expected_id in expected_ids.items():
        row = rows_by_basename.get(basename)
        check(row is not None, failures, f"{db_path}: missing row for {basename}")
        if row is None:
            continue
        check(row["id"] == expected_id, failures, f"{db_path}: wrong id for {basename}: {row['id']!r}")
        check(len(str(row["id"])) == 11, failures, f"{db_path}: id for {basename} is not 11 characters")
        check(row["basename"] == basename, failures, f"{db_path}: wrong basename for {basename}")
        check_timestamp(row["mod_time"], failures, f"{db_path}: bad mod_time for {basename}: {row['mod_time']!r}")
        check_timestamp(row["last_seen"], failures, f"{db_path}: bad last_seen for {basename}: {row['last_seen']!r}")
        check(row["deleted_time"] is None, failures, f"{db_path}: {basename} should not be tombstoned")

    rootfile = rows_by_basename.get("rootfile.txt")
    folder = rows_by_basename.get("folder")
    nested = rows_by_basename.get("nested.bin")
    if rootfile is not None:
        check(rootfile["parent_id"] == root_id, failures, f"{db_path}: rootfile parent_id should be root sentinel")
        check(rootfile["byte_size"] == 12, failures, f"{db_path}: rootfile byte_size should be 12")
    if folder is not None:
        check(folder["parent_id"] == root_id, failures, f"{db_path}: folder parent_id should be root sentinel")
        check(folder["byte_size"] == -1, failures, f"{db_path}: directory byte_size should be -1")
    if nested is not None:
        check(nested["parent_id"] == folder_id, failures, f"{db_path}: nested parent_id should be folder id")
        check(nested["byte_size"] == 3, failures, f"{db_path}: nested byte_size should be 3")

    return rows_by_basename


def check_tombstone(db_path: Path, old_last_seen: object, failures: list[str]) -> None:
    with sqlite3.connect(str(db_path)) as conn:
        conn.row_factory = sqlite3.Row
        row = conn.execute(
            """
            SELECT id, basename, last_seen, deleted_time
            FROM snapshot
            WHERE id = ?
            """,
            (path_id("rootfile.txt"),),
        ).fetchone()
    check(row is not None, failures, f"{db_path}: tombstone row for deleted rootfile.txt is missing")
    if row is None:
        return
    check(row["basename"] == "rootfile.txt", failures, f"{db_path}: tombstone row has wrong basename")
    check(row["last_seen"] == old_last_seen, failures, f"{db_path}: tombstone changed last_seen")
    check(row["deleted_time"] == old_last_seen, failures, f"{db_path}: deleted_time should copy current last_seen")
    check_timestamp(row["deleted_time"], failures, f"{db_path}: tombstone deleted_time has bad format")


def main() -> int:
    failures: list[str] = []

    # not reasonably testable: 007.19 requires observing an interrupted copy
    # destination after the decision is made but before the copy completes.

    with tempfile.TemporaryDirectory(prefix="kitchensync-007-") as temp_name:
        temp_root = Path(temp_name)
        peer_a = temp_root / "peer-a"
        peer_b = temp_root / "peer-b"
        peer_a.mkdir()
        peer_b.mkdir()

        (peer_a / "rootfile.txt").write_text("hello world\n", encoding="utf-8", newline="")
        (peer_a / "folder").mkdir()
        (peer_a / "folder" / "nested.bin").write_bytes(b"abc")

        run_sync(failures, f"+{peer_a}", str(peer_b))

        db_a = peer_a / ".kitchensync" / "snapshot.db"
        db_b = peer_b / ".kitchensync" / "snapshot.db"
        for db_path in (db_a, db_b):
            check(db_path.exists(), failures, f"missing snapshot database: {db_path}")
            if db_path.exists():
                check_schema(db_path, failures)

        initial_rows_a: dict[str, dict[str, object]] = {}
        if db_a.exists():
            initial_rows_a = check_initial_rows(db_a, failures)
        if db_b.exists():
            check_initial_rows(db_b, failures)

        old_last_seen = initial_rows_a.get("rootfile.txt", {}).get("last_seen")
        try:
            (peer_a / "rootfile.txt").unlink()
        except OSError as exc:
            failures.append(f"failed to delete rootfile.txt before tombstone run: {exc}")

        run_sync(failures, f"+{peer_a}", str(peer_b))
        if db_a.exists() and old_last_seen is not None:
            check_tombstone(db_a, old_last_seen, failures)

        if peer_b.exists():
            unexpected_rootfile = peer_b / "rootfile.txt"
            check(not unexpected_rootfile.exists(), failures, "deleted file still exists on peer_b after canon deletion")

        shutil.rmtree(temp_root, ignore_errors=True)

    if failures:
        print("FAIL")
        for failure in failures:
            print(f"- {failure}")
        return 1

    print("PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
