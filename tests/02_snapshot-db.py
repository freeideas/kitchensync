#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

from __future__ import annotations

import calendar
import os
import re
import shutil
import sqlite3
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")


PROJECT_DIR = Path("C:/Users/human/Desktop/prjx/kitchensync")
JAVA = Path("C:/Users/human/Desktop/prjx/kitchensync/tools/compiler/jdk/bin/java.exe")
JAR = Path("C:/Users/human/Desktop/prjx/kitchensync/released/kitchensync.jar")
WORK = PROJECT_DIR / "tests" / "tmp" / "02_snapshot-db"
ALPHABET = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"
TIMESTAMP_RE = re.compile(r"^\d{4}-\d{2}-\d{2}_\d{2}-\d{2}-\d{2}_\d{6}Z$")
EXPECTED_FILE_MOD_TIMES = {
    "alpha.txt": "2024-01-02_03-04-05_123456Z",
    "dir/child.txt": "2024-01-02_03-04-06_123456Z",
    "dir/nested/leaf.txt": "2024-01-02_03-04-07_123456Z",
    "slash/path.txt": "2024-01-02_03-04-08_123456Z",
}
MASK = (1 << 64) - 1
P1 = 11400714785074694791
P2 = 14029467366897019727
P3 = 1609587929392839161
P4 = 9650029242287828579
P5 = 2870177450012600261


def rotl(value: int, bits: int) -> int:
    value &= MASK
    return ((value << bits) | (value >> (64 - bits))) & MASK


def round64(acc: int, value: int) -> int:
    acc = (acc + value * P2) & MASK
    acc = rotl(acc, 31)
    return (acc * P1) & MASK


def merge_round(acc: int, value: int) -> int:
    acc ^= round64(0, value)
    return (acc * P1 + P4) & MASK


def avalanche(value: int) -> int:
    value ^= value >> 33
    value = (value * P2) & MASK
    value ^= value >> 29
    value = (value * P3) & MASK
    value ^= value >> 32
    return value & MASK


def xxhash64(data: bytes, seed: int = 0) -> int:
    index = 0
    length = len(data)
    if length >= 32:
        v1 = (seed + P1 + P2) & MASK
        v2 = (seed + P2) & MASK
        v3 = seed & MASK
        v4 = (seed - P1) & MASK
        stop = length - 32
        while index <= stop:
            v1 = round64(v1, int.from_bytes(data[index:index + 8], "little"))
            v2 = round64(v2, int.from_bytes(data[index + 8:index + 16], "little"))
            v3 = round64(v3, int.from_bytes(data[index + 16:index + 24], "little"))
            v4 = round64(v4, int.from_bytes(data[index + 24:index + 32], "little"))
            index += 32
        result = (rotl(v1, 1) + rotl(v2, 7) + rotl(v3, 12) + rotl(v4, 18)) & MASK
        result = merge_round(result, v1)
        result = merge_round(result, v2)
        result = merge_round(result, v3)
        result = merge_round(result, v4)
    else:
        result = (seed + P5) & MASK

    result = (result + length) & MASK
    while index + 8 <= length:
        lane = round64(0, int.from_bytes(data[index:index + 8], "little"))
        result ^= lane
        result = (rotl(result, 27) * P1 + P4) & MASK
        index += 8
    if index + 4 <= length:
        result ^= int.from_bytes(data[index:index + 4], "little") * P1
        result = (rotl(result, 23) * P2 + P3) & MASK
        index += 4
    while index < length:
        result ^= data[index] * P5
        result = (rotl(result, 11) * P1) & MASK
        index += 1
    return avalanche(result)


def path_id(relative_path: str) -> str:
    """11-char zero-padded base62 encoding of xxHash64(seed=0) of a forward-slash relative path."""
    value = xxhash64(relative_path.encode("utf-8"), 0)
    chars: list[str] = []
    for _ in range(11):
        value, digit = divmod(value, 62)
        chars.append(ALPHABET[digit])
    return "".join(reversed(chars))


ROOT_SENTINEL = path_id("/")  # parent_id for root-level entries; must never appear as a row id


def run_sync(*peers: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(JAVA), "-jar", str(JAR), *peers],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=60,
        check=False,
    )


def read_rows(peer: Path) -> list[sqlite3.Row]:
    db = peer / ".kitchensync" / "snapshot.db"
    con = sqlite3.connect(str(db))
    con.row_factory = sqlite3.Row
    try:
        return list(con.execute("SELECT * FROM snapshot"))
    finally:
        con.close()


def rows_by_path(rows: list[sqlite3.Row]) -> dict[str, sqlite3.Row]:
    """Reconstruct relative paths from the parent_id chain and return a {rel_path: row} map."""
    ids = {row["id"] for row in rows}
    children: dict[str, list[sqlite3.Row]] = {}
    roots: list[sqlite3.Row] = []
    for row in rows:
        if row["parent_id"] in ids:
            children.setdefault(row["parent_id"], []).append(row)
        else:
            roots.append(row)

    by_path: dict[str, sqlite3.Row] = {}

    def visit(row: sqlite3.Row, prefix: str) -> None:
        rel = row["basename"] if not prefix else f"{prefix}/{row['basename']}"
        by_path[rel] = row
        for child in sorted(children.get(row["id"], []), key=lambda r: r["basename"]):
            visit(child, rel)

    for root in sorted(roots, key=lambda r: r["basename"]):
        visit(root, "")
    return by_path


def check(condition: bool, message: str, failures: list[str]) -> None:
    if not condition:
        failures.append(message)


def write_file(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def set_mtime(path: Path, timestamp: str) -> None:
    dt = datetime.strptime(timestamp, "%Y-%m-%d_%H-%M-%S_%fZ").replace(tzinfo=timezone.utc)
    ns = calendar.timegm(dt.utctimetuple()) * 1_000_000_000 + dt.microsecond * 1_000
    os.utime(path, ns=(ns, ns))


def check_initial_snapshot(
    peer: Path,
    expected_paths: list[str],
    failures: list[str],
) -> None:
    rows_list = read_rows(peer)
    rows = rows_by_path(rows_list)

    # 02.18 -- snapshot ends up with one row per tracked descendant
    check(
        len(rows) == len(expected_paths),
        f"{peer.name}: {len(rows)} snapshot rows, expected {len(expected_paths)}: {expected_paths}",
        failures,
    )

    # 02.22 -- the sync root itself has no row (relative path of the root is "", so id = path_id(""))
    row_ids = {row["id"] for row in rows_list}
    root_self_id = path_id("")
    check(
        root_self_id not in row_ids,
        f"{peer.name}: snapshot contains a row whose id matches path_id('') ({root_self_id}); "
        "the sync root directory must not be tracked",
        failures,
    )
    # data integrity: parent_id sentinel must never coincide with any actual row id
    check(
        ROOT_SENTINEL not in row_ids,
        f"{peer.name}: a row id collides with the root parent_id sentinel ({ROOT_SENTINEL})",
        failures,
    )

    for relative_path in expected_paths:
        row = rows.get(relative_path)
        check(row is not None, f"{peer.name}: missing snapshot row for {relative_path!r}", failures)
        if row is None:
            continue

        parent = "/" if "/" not in relative_path else relative_path.rsplit("/", 1)[0]
        expected_basename = relative_path.rsplit("/", 1)[-1]
        disk_path = peer.joinpath(*relative_path.split("/"))

        # 02.38, 02.53, 02.55 -- id and parent_id: xxHash64 of forward-slash path, 11-char base62
        check(
            row["id"] == path_id(relative_path),
            f"{peer.name} {relative_path!r}: id={row['id']!r} does not match xxHash64/base62 of path",
            failures,
        )
        check(
            len(row["id"]) == 11 and all(ch in ALPHABET for ch in row["id"]),
            f"{peer.name} {relative_path!r}: id={row['id']!r} is not 11-char base62",
            failures,
        )
        check(
            row["parent_id"] == path_id(parent),
            f"{peer.name} {relative_path!r}: parent_id={row['parent_id']!r}, expected hash of {parent!r}",
            failures,
        )
        check(
            len(row["parent_id"]) == 11 and all(ch in ALPHABET for ch in row["parent_id"]),
            f"{peer.name} {relative_path!r}: parent_id={row['parent_id']!r} is not 11-char base62",
            failures,
        )

        # 02.56 -- basename is the final path component only
        check(
            row["basename"] == expected_basename,
            f"{peer.name} {relative_path!r}: basename={row['basename']!r}, expected {expected_basename!r}",
            failures,
        )

        # 02.23 -- timestamp format for mod_time and last_seen
        for ts_col in ("mod_time", "last_seen"):
            ts_val = row[ts_col]
            check(
                ts_val is not None and bool(TIMESTAMP_RE.match(ts_val)),
                f"{peer.name} {relative_path!r}: {ts_col}={ts_val!r} wrong format (need YYYY-MM-DD_HH-mm-ss_ffffffZ)",
                failures,
            )

        # 02.52 -- present entry: deleted_time is NULL
        check(
            row["deleted_time"] is None,
            f"{peer.name} {relative_path!r}: present entry has deleted_time={row['deleted_time']!r}",
            failures,
        )

        # 02.21 -- byte_size: -1 for directories, actual size for files
        if disk_path.is_dir():
            check(
                row["byte_size"] == -1,
                f"{peer.name} {relative_path!r}: directory byte_size={row['byte_size']}, expected -1",
                failures,
            )
        else:
            expected_size = disk_path.stat().st_size
            check(
                row["byte_size"] == expected_size,
                f"{peer.name} {relative_path!r}: file byte_size={row['byte_size']}, expected {expected_size}",
                failures,
            )
            check(
                row["byte_size"] >= 0,
                f"{peer.name} {relative_path!r}: file byte_size is negative",
                failures,
            )
            expected_mod_time = EXPECTED_FILE_MOD_TIMES.get(relative_path)
            if expected_mod_time is not None:
                check(
                    row["mod_time"] == expected_mod_time,
                    f"{peer.name} {relative_path!r}: mod_time={row['mod_time']!r}, expected current file mod_time {expected_mod_time!r}",
                    failures,
                )

    # 02.40, 02.45 -- last_seen values within one process run are strictly monotonic (no duplicates)
    last_seen_values = [row["last_seen"] for row in rows.values() if row["last_seen"] is not None]
    check(
        len(last_seen_values) == len(set(last_seen_values)),
        f"{peer.name}: duplicate last_seen values in one run -- not strictly monotonic: {last_seen_values}",
        failures,
    )


def main() -> int:
    failures: list[str] = []

    # idempotency: clean leftover state from previous runs at the start
    if WORK.exists():
        shutil.rmtree(WORK)
    peer_a = WORK / "peer-a"
    peer_b = WORK / "peer-b"
    peer_a.mkdir(parents=True)
    peer_b.mkdir(parents=True)  # no .kitchensync/ yet -- tests 02.50

    write_file(peer_a / "alpha.txt", "alpha\n")
    write_file(peer_a / "dir" / "child.txt", "child\n")
    write_file(peer_a / "dir" / "nested" / "leaf.txt", "leaf\n")
    write_file(peer_a / "slash" / "path.txt", "slash\n")
    for relative_path, timestamp in EXPECTED_FILE_MOD_TIMES.items():
        set_mtime(peer_a.joinpath(*relative_path.split("/")), timestamp)

    # 02.18, 02.20-02.23, 02.38, 02.40-02.43, 02.45, 02.50-02.56
    # 02.50: peer-b has no snapshot.db -- must not be treated as an error
    first = run_sync("+" + str(peer_a), str(peer_b))
    check(
        first.returncode == 0,
        f"initial sync exited {first.returncode}\nstdout={first.stdout}\nstderr={first.stderr}",
        failures,
    )

    for peer in (peer_a, peer_b):
        # 02.18 -- snapshot.db must exist at peer root after a successful sync
        snapshot = peer / ".kitchensync" / "snapshot.db"
        check(snapshot.is_file(), f"{peer.name}: .kitchensync/snapshot.db missing", failures)

        # 02.43 -- sidecar SQLite files are not synced (check before opening the db)
        check(
            not (peer / ".kitchensync" / "snapshot.db-wal").exists(),
            f"{peer.name}: snapshot.db-wal present (WAL sidecar must not be synced)",
            failures,
        )
        check(
            not (peer / ".kitchensync" / "snapshot.db-shm").exists(),
            f"{peer.name}: snapshot.db-shm present (SHM sidecar must not be synced)",
            failures,
        )

    expected_paths = [
        "alpha.txt",
        "dir",
        "dir/child.txt",
        "dir/nested",
        "dir/nested/leaf.txt",
        "slash",
        "slash/path.txt",
    ]
    initial_last_seen_values: list[tuple[str, str, str]] = []
    for peer in (peer_a, peer_b):
        if (peer / ".kitchensync" / "snapshot.db").is_file():
            check_initial_snapshot(peer, expected_paths, failures)
            rows = rows_by_path(read_rows(peer))
            initial_last_seen_values.extend(
                (peer.name, relative_path, rows[relative_path]["last_seen"])
                for relative_path in expected_paths
                if relative_path in rows and rows[relative_path]["last_seen"] is not None
            )
    last_seen_values = [value for _, _, value in initial_last_seen_values]
    check(
        len(last_seen_values) == len(set(last_seen_values)),
        "02.40/02.45: last_seen values are not unique across peers in one run: "
        + repr(initial_last_seen_values),
        failures,
    )

    # 02.41, 02.51, 02.54, 02.20 (column presence), 02.42 (journal mode), 02.56 (NOT NULL)
    if (peer_a / ".kitchensync" / "snapshot.db").is_file():
        con = sqlite3.connect(str(peer_a / ".kitchensync" / "snapshot.db"))
        try:
            # 02.41 -- exactly one table named "snapshot"
            tables = [
                r[0]
                for r in con.execute(
                    "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'"
                )
            ]
            check(tables == ["snapshot"], f"schema tables={tables!r}, expected exactly ['snapshot']", failures)

            # 02.51 -- indexes on parent_id, last_seen, deleted_time
            indexed_columns = {
                tuple(info[2] for info in con.execute(f"PRAGMA index_info({r[1]!r})"))
                for r in con.execute("PRAGMA index_list(snapshot)")
            }
            for col in ("parent_id", "last_seen", "deleted_time"):
                check(
                    (col,) in indexed_columns,
                    f"snapshot table is missing an index on '{col}'",
                    failures,
                )

            # 02.54 -- id is the primary key
            table_info = list(con.execute("PRAGMA table_info(snapshot)"))
            id_info = [c for c in table_info if c[1] == "id"]
            check(
                bool(id_info and id_info[0][5] == 1),
                "snapshot.id is not the primary key",
                failures,
            )

            # 02.20 -- required columns present
            col_names = {c[1] for c in table_info}
            for col in ("id", "parent_id", "basename", "mod_time", "byte_size", "last_seen", "deleted_time"):
                check(col in col_names, f"snapshot table missing column '{col}'", failures)

            # 02.56 -- basename, mod_time, byte_size are declared NOT NULL
            for col in ("basename", "mod_time", "byte_size"):
                info = [c for c in table_info if c[1] == col]
                check(
                    bool(info and info[0][3] == 1),
                    f"snapshot.{col} is not declared NOT NULL",
                    failures,
                )

            # 02.42 -- rollback-journal mode (not WAL)
            journal_mode = con.execute("PRAGMA journal_mode").fetchone()[0]
            check(
                journal_mode.lower() in {"delete", "truncate", "persist"},
                f"snapshot database journal_mode={journal_mode!r}, expected rollback-journal mode",
                failures,
            )
        finally:
            con.close()

    # not reasonably testable: 02.42 foreign-key enforcement is a per-connection setting, not durable state
    # not reasonably testable: 02.24 atomic TMP rename and 02.49 local tmp copy are internal run mechanics
    # not reasonably testable: 02.40 exact generation order and collision-avoidance (1us bump) have no public signal; uniqueness is checked above

    # 02.43 -- sidecar files at a peer are never synced to other peers
    write_file(peer_a / ".kitchensync" / "snapshot.db-wal", "not sqlite state\n")
    write_file(peer_a / ".kitchensync" / "snapshot.db-shm", "not sqlite state\n")
    second = run_sync("+" + str(peer_a), str(peer_b))
    check(
        second.returncode == 0,
        f"sidecar sync exited {second.returncode}\nstdout={second.stdout}\nstderr={second.stderr}",
        failures,
    )
    check(
        not (peer_b / ".kitchensync" / "snapshot.db-wal").exists(),
        "snapshot.db-wal was synced to peer-b",
        failures,
    )
    check(
        not (peer_b / ".kitchensync" / "snapshot.db-shm").exists(),
        "snapshot.db-shm was synced to peer-b",
        failures,
    )

    # 02.25, 02.57 -- directory displacement cascades deleted_time to all descendants;
    # each descendant receives the displaced entry's deletion estimate (same timestamp, not a new one)
    shutil.rmtree(peer_a / "dir")
    write_file(peer_a / "dir", "replacement file\n")
    displacement = run_sync("+" + str(peer_a), str(peer_b))
    check(
        displacement.returncode == 0,
        f"displacement sync exited {displacement.returncode}\nstdout={displacement.stdout}\nstderr={displacement.stderr}",
        failures,
    )
    for label, displaced_peer in (("peer-b", peer_b), ("peer-a", peer_a)):
        rows_after = rows_by_path(read_rows(displaced_peer))
        dir_row    = rows_after.get("dir")
        child_row  = rows_after.get("dir/child.txt")
        nested_row = rows_after.get("dir/nested")
        leaf_row   = rows_after.get("dir/nested/leaf.txt")

        # the displaced directory path is now a file -- its row should reflect the new file state
        check(
            dir_row is not None and dir_row["deleted_time"] is None and dir_row["byte_size"] >= 0,
            f"{label}: displaced directory path was not upserted as a present replacement file",
            failures,
        )
        descendant_deleted_times = [
            row["deleted_time"] for row in (child_row, nested_row, leaf_row) if row is not None
        ]
        check(
            len(descendant_deleted_times) == 3,
            f"02.25 {label}: {3 - len(descendant_deleted_times)} displaced directory descendant(s) missing from snapshot",
            failures,
        )
        check(
            all(v is not None and TIMESTAMP_RE.match(v) for v in descendant_deleted_times),
            f"02.25 {label}: descendant deleted_time values invalid or NULL: {descendant_deleted_times}",
            failures,
        )
        check(
            len(set(descendant_deleted_times)) == 1,
            f"02.57 {label}: descendants did not receive the same copied deletion estimate: {descendant_deleted_times}",
            failures,
        )

    # 02.50 -- reachable peer with no existing snapshot.db creates a new empty one rather than failing
    fresh_peer = WORK / "fresh-peer"
    fresh_peer.mkdir()
    missing_snapshot = run_sync("+" + str(peer_a), str(fresh_peer))
    check(
        missing_snapshot.returncode == 0,
        f"02.50: peer without existing snapshot was treated as an error\nstdout={missing_snapshot.stdout}\nstderr={missing_snapshot.stderr}",
        failures,
    )
    check(
        (fresh_peer / ".kitchensync" / "snapshot.db").is_file(),
        "02.50: peer without existing snapshot did not receive a new snapshot.db",
        failures,
    )

    # 02.45 -- BAK and TMP timestamp directory names are unique across the entire run
    tmp_bak_names = [
        path.name
        for peer in (peer_a, peer_b, fresh_peer)
        for path in peer.rglob("*")
        if path.parent.name in {"TMP", "BAK"} and TIMESTAMP_RE.match(path.name)
    ]
    check(
        len(tmp_bak_names) == len(set(tmp_bak_names)),
        f"02.45: TMP/BAK timestamp directory names are not unique: {tmp_bak_names}",
        failures,
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
