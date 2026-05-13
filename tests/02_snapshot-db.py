#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Snapshot database: schema, path identity, timestamps, and tombstones."""

from __future__ import annotations

import os, re, shutil, sqlite3, subprocess, sys
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

TMP = Path(PROJECT) / "tmp" / "testks" / "02_snapshot-db"
PEER_A = TMP / "peer_a"
PEER_B = TMP / "peer_b"

TS_RE = re.compile(r"^\d{4}-\d{2}-\d{2}_\d{2}-\d{2}-\d{2}_\d{6}Z$")
B62_RE = re.compile(r"^[0-9A-Za-z]{11}$")
REQUIRED_COLS = {"basename", "mod_time", "byte_size", "last_seen", "deleted_time"}

EXPECTED_PATHS = [
    "file1.txt",
    "subdir",
    "subdir/file2.txt",
    "subdir/nested",
    "subdir/nested/file3.txt",
] + [f"mono{i}.txt" for i in range(10)]
EXPECTED_BASENAMES = sorted(path.rsplit("/", 1)[-1] for path in EXPECTED_PATHS)
EXPECTED_DIRS = {"subdir", "subdir/nested"}
EXPECTED_SIZES = {
    "file1.txt": len("hello".encode("utf-8")),
    "subdir/file2.txt": len("world".encode("utf-8")),
    "subdir/nested/file3.txt": len("deep".encode("utf-8")),
    **{f"mono{i}.txt": len(f"x{i}".encode("utf-8")) for i in range(10)},
}

MASK64 = 0xFFFFFFFFFFFFFFFF
P1 = 0x9E3779B185EBCA87
P2 = 0xC2B2AE3D27D4EB4F
P3 = 0x165667B19E3779F9
P4 = 0x85EBCA77C2B2AE63
P5 = 0x27D4EB2F165667C5
ALPHABET = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"


def _u64(value: int) -> int:
    return value & MASK64


def _rotl(value: int, bits: int) -> int:
    value &= MASK64
    return ((value << bits) | (value >> (64 - bits))) & MASK64


def _read_u64_le(data: bytes, pos: int) -> int:
    return int.from_bytes(data[pos:pos + 8], "little")


def _read_u32_le(data: bytes, pos: int) -> int:
    return int.from_bytes(data[pos:pos + 4], "little")


def _merge_round(acc: int, value: int) -> int:
    acc ^= _u64(_rotl(_u64(value * P2), 31) * P1)
    return _u64(acc * P1 + P4)


def _xxhash64_seed0(data: bytes) -> int:
    length = len(data)
    pos = 0
    if length >= 32:
        v1 = _u64(P1 + P2)
        v2 = P2
        v3 = 0
        v4 = _u64(-P1)
        limit = length - 32
        while pos <= limit:
            v1 = _u64(_rotl(_u64(v1 + _read_u64_le(data, pos) * P2), 31) * P1)
            pos += 8
            v2 = _u64(_rotl(_u64(v2 + _read_u64_le(data, pos) * P2), 31) * P1)
            pos += 8
            v3 = _u64(_rotl(_u64(v3 + _read_u64_le(data, pos) * P2), 31) * P1)
            pos += 8
            v4 = _u64(_rotl(_u64(v4 + _read_u64_le(data, pos) * P2), 31) * P1)
            pos += 8
        h64 = _u64(_rotl(v1, 1) + _rotl(v2, 7) + _rotl(v3, 12) + _rotl(v4, 18))
        h64 = _merge_round(h64, v1)
        h64 = _merge_round(h64, v2)
        h64 = _merge_round(h64, v3)
        h64 = _merge_round(h64, v4)
    else:
        h64 = P5

    h64 = _u64(h64 + length)
    while pos + 8 <= length:
        h64 ^= _u64(_rotl(_u64(_read_u64_le(data, pos) * P2), 31) * P1)
        h64 = _u64(_rotl(h64, 27) * P1 + P4)
        pos += 8
    if pos + 4 <= length:
        h64 ^= _u64(_read_u32_le(data, pos) * P1)
        h64 = _u64(_rotl(h64, 23) * P2 + P3)
        pos += 4
    while pos < length:
        h64 ^= _u64(data[pos] * P5)
        h64 = _u64(_rotl(h64, 11) * P1)
        pos += 1

    h64 ^= h64 >> 33
    h64 = _u64(h64 * P2)
    h64 ^= h64 >> 29
    h64 = _u64(h64 * P3)
    h64 ^= h64 >> 32
    return h64 & MASK64


def _base62_11(value: int) -> str:
    chars = []
    value &= MASK64
    for _ in range(11):
        chars.append(ALPHABET[value % 62])
        value //= 62
    return "".join(reversed(chars))


def _identify(path: str) -> str:
    hash_path = "" if path in ("", "/") else path
    return _base62_11(_xxhash64_seed0(hash_path.encode("utf-8")))


def _parent_path(path: str) -> str:
    slash = path.rfind("/")
    return "/" if slash < 0 else path[:slash]


def _quote_ident(name: str) -> str:
    return '"' + name.replace('"', '""') + '"'


def _sync(url_a: str, url_b: str, canon_a: bool = True) -> subprocess.CompletedProcess:
    a_arg = ("+" if canon_a else "") + url_a
    return subprocess.run(
        [str(UV), "run", "--script", str(BUILD_PY), "invoke-cli", PROJECT, a_arg, url_b],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        text=True, encoding="utf-8",
        timeout=60,
    )


def _snapshot_rows(peer: Path) -> tuple[list[dict], str | None, str | None]:
    db = peer / ".kitchensync" / "snapshot.db"
    if not db.exists():
        return [], None, "missing"
    try:
        con = sqlite3.connect(str(db))
        try:
            cur = con.execute("SELECT name FROM sqlite_master WHERE type='table'")
            for (name,) in cur.fetchall():
                quoted = _quote_ident(name)
                cols = {row[1] for row in con.execute(f"PRAGMA table_info({quoted})")}
                if REQUIRED_COLS.issubset(cols):
                    col_names = [d[0] for d in con.execute(f"SELECT * FROM {quoted} LIMIT 0").description]
                    rows = [dict(zip(col_names, r)) for r in con.execute(f"SELECT * FROM {quoted}").fetchall()]
                    return rows, name, None
        finally:
            con.close()
    except sqlite3.DatabaseError as exc:
        return [], None, str(exc)
    return [], None, "no table with required columns"


def _rows_by_basename(rows: list[dict]) -> dict[str, dict]:
    return {str(row.get("basename")): row for row in rows}


def _check_snapshot_content(label: str, rows: list[dict], failures: list[str]) -> None:
    basenames = sorted(str(row.get("basename")) for row in rows)
    print(f"[02.20 {label}] one row per tracked descendant: {basenames == EXPECTED_BASENAMES}")
    if basenames != EXPECTED_BASENAMES:
        failures.append(f"02.20 {label}: expected basenames {EXPECTED_BASENAMES}, got {basenames}")

    root_id = _identify("/")
    root_rows = [
        row for row in rows
        if row.get("id") == root_id or row.get("basename") in ("", "/")
    ]
    print(f"[02.22 {label}] sync root itself has no row: {not root_rows}")
    if root_rows:
        failures.append(f"02.22 {label}: sync root appears in snapshot rows: {root_rows}")

    by_name = _rows_by_basename(rows)
    for path in sorted(EXPECTED_DIRS):
        row = by_name.get(path.rsplit("/", 1)[-1])
        ok = row is not None and row.get("byte_size") == -1
        print(f"[02.21a {label}] {path} directory row byte_size=-1: {ok}")
        if not ok:
            failures.append(
                f"02.21a {label}: {path} byte_size={row.get('byte_size') if row else 'no row'}"
            )

    for path, expected_size in EXPECTED_SIZES.items():
        row = by_name.get(path.rsplit("/", 1)[-1])
        ok = row is not None and row.get("byte_size") == expected_size
        print(f"[02.21b {label}] {path} byte_size={expected_size}: {ok}")
        if not ok:
            failures.append(
                f"02.21b {label}: {path} byte_size="
                f"{row.get('byte_size') if row else 'no row'}, expected {expected_size}"
            )

    bad_timestamps = []
    for row in rows:
        for col in ("mod_time", "last_seen", "deleted_time"):
            value = row.get(col)
            if value is not None and not TS_RE.match(str(value)):
                bad_timestamps.append(f"{row.get('basename')}.{col}={value!r}")
    print(f"[02.23 {label}] all timestamp values use required UTC format: {not bad_timestamps}")
    if bad_timestamps:
        failures.append(f"02.23 {label}: malformed timestamps: {bad_timestamps[:3]}")

    missing_identity_cols = [col for col in ("id", "parent_id") if rows and col not in rows[0]]
    if missing_identity_cols:
        print(f"[02.38 {label}] FAIL: missing columns {missing_identity_cols}")
        failures.append(f"02.38 {label}: missing columns {missing_identity_cols}")
        return

    for path in EXPECTED_PATHS:
        basename = path.rsplit("/", 1)[-1]
        row = by_name.get(basename)
        expected_id = _identify(path)
        expected_parent_id = _identify(_parent_path(path))
        id_ok = row is not None and row.get("id") == expected_id
        parent_ok = row is not None and row.get("parent_id") == expected_parent_id
        shape_ok = (
            row is not None
            and B62_RE.match(str(row.get("id"))) is not None
            and B62_RE.match(str(row.get("parent_id"))) is not None
        )
        print(f"[02.38 {label}] {path} id/parent_id exact: {id_ok and parent_ok and shape_ok}")
        if row is None:
            failures.append(f"02.38 {label}: missing row for {path}")
        else:
            if not shape_ok:
                failures.append(f"02.38 {label}: {path} id/parent_id not 11-char base62: {row}")
            if not id_ok:
                failures.append(f"02.38 {label}: {path} id={row.get('id')!r}, expected {expected_id!r}")
            if not parent_ok:
                failures.append(
                    f"02.38 {label}: {path} parent_id={row.get('parent_id')!r}, "
                    f"expected {expected_parent_id!r}"
                )


def _check_subtree_tombstones(label: str, rows: list[dict], failures: list[str]) -> None:
    by_name = _rows_by_basename(rows)
    subdir = by_name.get("subdir")
    nested = by_name.get("nested")
    file2 = by_name.get("file2.txt")
    file3 = by_name.get("file3.txt")
    subdir_deleted = subdir.get("deleted_time") if subdir else None
    descendant_deleted_times = [
        row.get("deleted_time") if row else None
        for row in (file2, nested, file3)
    ]
    chain_ok = (
        subdir is not None
        and file2 is not None
        and nested is not None
        and file3 is not None
        and file2.get("parent_id") == subdir.get("id")
        and nested.get("parent_id") == subdir.get("id")
        and file3.get("parent_id") == nested.get("id")
    )
    tombstone_ok = (
        subdir_deleted is not None
        and all(deleted_time == subdir_deleted for deleted_time in descendant_deleted_times)
    )
    outside_ok = True
    for name in ("file1.txt", "mono0.txt", "mono9.txt"):
        row = by_name.get(name)
        if row is None or row.get("deleted_time") is not None:
            outside_ok = False
    print(f"[02.25 {label}] directory and descendant tombstoned through parent_id chain: {chain_ok and tombstone_ok}")
    if not chain_ok:
        failures.append(f"02.25 {label}: descendants are not linked under subdir by parent_id")
    if not tombstone_ok:
        failures.append(
            f"02.25 {label}: subdir deleted_time={subdir_deleted!r}, "
            f"descendant deleted_time values={descendant_deleted_times!r}"
        )
    print(f"[02.25 {label}] unrelated rows remain live: {outside_ok}")
    if not outside_ok:
        failures.append(f"02.25 {label}: unrelated rows were tombstoned")


def main() -> int:
    if TMP.exists():
        shutil.rmtree(TMP)
    PEER_A.mkdir(parents=True)
    PEER_B.mkdir(parents=True)

    (PEER_A / "file1.txt").write_text("hello", encoding="utf-8")
    (PEER_A / "subdir").mkdir()
    (PEER_A / "subdir" / "file2.txt").write_text("world", encoding="utf-8")
    (PEER_A / "subdir" / "nested").mkdir()
    (PEER_A / "subdir" / "nested" / "file3.txt").write_text("deep", encoding="utf-8")
    for i in range(10):
        (PEER_A / f"mono{i}.txt").write_text(f"x{i}", encoding="utf-8")

    url_a = PEER_A.resolve().as_uri()
    url_b = PEER_B.resolve().as_uri()
    db_a = PEER_A / ".kitchensync" / "snapshot.db"
    db_b = PEER_B / ".kitchensync" / "snapshot.db"

    failures = []

    try:
        proc1 = _sync(url_a, url_b)
        first_sync_ok = proc1.returncode == 0
        if not first_sync_ok:
            print(
                f"[setup] sync failed exit {proc1.returncode}\n"
                f"  stdout: {proc1.stdout!r}\n"
                f"  stderr: {proc1.stderr!r}"
            )
            failures.append("setup: first sync failed")
        else:
            print("[setup] first sync completed (exit 0)")

        if first_sync_ok:
            rows_by_peer = {}
            for label, peer, db in (("peer_a", PEER_A, db_a), ("peer_b", PEER_B, db_b)):
                exists = db.exists()
                print(f"[02.18 {label}] .kitchensync/snapshot.db exists: {exists}")
                if not exists:
                    failures.append(f"02.18 {label}: missing .kitchensync/snapshot.db")
                    continue

                rows, table, error = _snapshot_rows(peer)
                sqlite_ok = error is None and table is not None
                print(f"[02.18 {label}] snapshot.db is readable SQLite with snapshot rows: {sqlite_ok}")
                if not sqlite_ok:
                    failures.append(f"02.18 {label}: snapshot.db not readable as required SQLite snapshot ({error})")
                    continue

                print(f"[02.20 {label}] table has required columns: {table is not None}")
                rows_by_peer[label] = rows
                _check_snapshot_content(label, rows, failures)

            # 02.24 — not reasonably testable through the CLI. After a successful
            # run, file:// exposes only the final filesystem state. Proving that
            # snapshot.db landed via a same-filesystem rename from
            # .kitchensync/TMP/ would require instrumenting the transport or OS
            # rename operations, or interrupting the upload mid-run.

            # 02.40 — the exact +1us collision fallback is not reasonably
            # testable through the CLI because there is no hook to freeze or
            # inject the process clock. The observable consequence available
            # here is that program-generated last_seen values are not reused
            # within the sync run.
            seen_values = [
                row["last_seen"]
                for rows in rows_by_peer.values()
                for row in rows
                if row.get("last_seen") is not None
            ]
            distinct_seen = len(seen_values) == len(set(seen_values))
            print(f"[02.40] last_seen values are distinct within one sync process: {distinct_seen}")
            if not distinct_seen:
                duplicates = sorted({value for value in seen_values if seen_values.count(value) > 1})
                failures.append(f"02.40: duplicate last_seen timestamps: {duplicates[:3]}")

            shutil.rmtree(PEER_A / "subdir", ignore_errors=True)
            proc2 = _sync(url_a, url_b)
            if proc2.returncode != 0:
                print(f"[02.25 setup] second sync failed exit {proc2.returncode}")
                failures.append(
                    f"02.25: second sync failed stdout={proc2.stdout!r} stderr={proc2.stderr!r}"
                )
            else:
                print("[02.25 setup] second sync (after directory deletion) completed")
                for label, peer in (("peer_a", PEER_A), ("peer_b", PEER_B)):
                    rows, table, error = _snapshot_rows(peer)
                    if table is None:
                        failures.append(f"02.25 {label}: snapshot table not found after second sync ({error})")
                    else:
                        _check_subtree_tombstones(label, rows, failures)

    finally:
        shutil.rmtree(TMP, ignore_errors=True)

    if failures:
        print("\nFAILURES:")
        for failure in failures:
            print(f"  - {failure}")
        return 1
    print("\nAll assertions passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
