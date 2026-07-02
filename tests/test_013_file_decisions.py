# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

from __future__ import annotations

import os
import sqlite3
import subprocess
import sys
import tempfile
from datetime import datetime, timezone
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

WORKSPACE = Path("/home/ace/Desktop/prjx/kitchensync")
KITCHENSYNC = Path("/home/ace/Desktop/prjx/kitchensync/released/kitchensync.exe")

P1 = 11400714785074694791
P2 = 14029467366897019727
P3 = 1609587929392839161
P4 = 9650029242287828579
P5 = 2870177450012600261
MASK = (1 << 64) - 1
BASE62 = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"


def rotl(value: int, bits: int) -> int:
    value &= MASK
    return ((value << bits) | (value >> (64 - bits))) & MASK


def xxh64(data: bytes, seed: int = 0) -> int:
    def round64(acc: int, lane: int) -> int:
        acc = (acc + lane * P2) & MASK
        acc = rotl(acc, 31)
        return (acc * P1) & MASK

    def merge(acc: int, lane_acc: int) -> int:
        acc ^= round64(0, lane_acc)
        return (acc * P1 + P4) & MASK

    index = 0
    length = len(data)
    if length >= 32:
        v1 = (seed + P1 + P2) & MASK
        v2 = (seed + P2) & MASK
        v3 = seed & MASK
        v4 = (seed - P1) & MASK
        limit = length - 32
        while index <= limit:
            v1 = round64(v1, int.from_bytes(data[index : index + 8], "little"))
            index += 8
            v2 = round64(v2, int.from_bytes(data[index : index + 8], "little"))
            index += 8
            v3 = round64(v3, int.from_bytes(data[index : index + 8], "little"))
            index += 8
            v4 = round64(v4, int.from_bytes(data[index : index + 8], "little"))
            index += 8
        h = (rotl(v1, 1) + rotl(v2, 7) + rotl(v3, 12) + rotl(v4, 18)) & MASK
        h = merge(h, v1)
        h = merge(h, v2)
        h = merge(h, v3)
        h = merge(h, v4)
    else:
        h = (seed + P5) & MASK

    h = (h + length) & MASK
    while index + 8 <= length:
        lane = int.from_bytes(data[index : index + 8], "little")
        h ^= round64(0, lane)
        h = (rotl(h, 27) * P1 + P4) & MASK
        index += 8
    if index + 4 <= length:
        lane = int.from_bytes(data[index : index + 4], "little")
        h ^= (lane * P1) & MASK
        h = (rotl(h, 23) * P2 + P3) & MASK
        index += 4
    while index < length:
        h ^= (data[index] * P5) & MASK
        h = (rotl(h, 11) * P1) & MASK
        index += 1

    h ^= h >> 33
    h = (h * P2) & MASK
    h ^= h >> 29
    h = (h * P3) & MASK
    h ^= h >> 32
    return h & MASK


def base62_11(value: int) -> str:
    chars: list[str] = []
    for _ in range(11):
        value, digit = divmod(value, 62)
        chars.append(BASE62[digit])
    return "".join(reversed(chars))


def path_id(relpath: str) -> str:
    return base62_11(xxh64(relpath.encode("utf-8")))


def ts(seconds: int) -> str:
    return datetime.fromtimestamp(seconds, timezone.utc).strftime("%Y-%m-%d_%H-%M-%S_%fZ")


def write_file(path: Path, data: bytes, mtime: int) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)
    os.utime(path, (mtime, mtime))


def read_bytes(path: Path) -> bytes | None:
    if not path.exists():
        return None
    return path.read_bytes()


def create_snapshot(peer: Path, rows: list[dict[str, object]]) -> None:
    meta = peer / ".kitchensync"
    meta.mkdir(parents=True, exist_ok=True)
    db_path = meta / "snapshot.db"
    if db_path.exists():
        db_path.unlink()
    with sqlite3.connect(db_path) as db:
        db.execute(
            """
            CREATE TABLE snapshot (
                id TEXT PRIMARY KEY,
                parent_id TEXT,
                basename TEXT NOT NULL,
                mod_time TEXT NOT NULL,
                byte_size INTEGER NOT NULL,
                last_seen TEXT,
                deleted_time TEXT
            )
            """
        )
        db.execute("CREATE INDEX idx_snapshot_parent_id ON snapshot(parent_id)")
        db.execute("CREATE INDEX idx_snapshot_last_seen ON snapshot(last_seen)")
        db.execute("CREATE INDEX idx_snapshot_deleted_time ON snapshot(deleted_time)")
        for row in rows:
            relpath = str(row["path"]).replace("\\", "/").strip("/")
            parent = relpath.rsplit("/", 1)[0] if "/" in relpath else "/"
            basename = relpath.rsplit("/", 1)[-1]
            db.execute(
                """
                INSERT INTO snapshot
                    (id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time)
                VALUES (?, ?, ?, ?, ?, ?, ?)
                """,
                (
                    path_id(relpath),
                    path_id(parent),
                    basename,
                    ts(int(row["mod_time"])),
                    int(row["byte_size"]),
                    None if row.get("last_seen") is None else ts(int(row["last_seen"])),
                    None
                    if row.get("deleted_time") is None
                    else ts(int(row["deleted_time"])),
                ),
            )
        db.commit()


def make_peer(base: Path, name: str, rows: list[dict[str, object]] | None = None) -> Path:
    peer = base / name
    peer.mkdir(parents=True, exist_ok=True)
    if rows is not None:
        create_snapshot(peer, rows)
    return peer


def history_row() -> dict[str, object]:
    return {
        "path": "__history__.txt",
        "mod_time": 1_700_000_000,
        "byte_size": 1,
        "last_seen": 1_700_000_100,
        "deleted_time": None,
    }


def file_row(
    relpath: str,
    mod_time: int,
    byte_size: int,
    last_seen: int | None = 1_700_000_100,
    deleted_time: int | None = None,
) -> dict[str, object]:
    return {
        "path": relpath,
        "mod_time": mod_time,
        "byte_size": byte_size,
        "last_seen": last_seen,
        "deleted_time": deleted_time,
    }


def run_sync(args: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(KITCHENSYNC), *args],
        cwd=str(WORKSPACE),
        text=True,
        encoding="utf-8",
        errors="replace",
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=30,
        check=False,
    )


def peer_arg(peer: Path, prefix: str = "") -> str:
    return prefix + str(peer)


def has_bak_entry(peer: Path, basename: str) -> bool:
    bak = peer / ".kitchensync" / "BAK"
    return bak.exists() and any(path.name == basename for path in bak.rglob(basename))


def check(condition: bool, failures: list[str], message: str) -> None:
    if not condition:
        failures.append(message)


def expect_success(result: subprocess.CompletedProcess[str], failures: list[str], label: str) -> None:
    check(result.returncode == 0, failures, f"{label}: expected exit 0, got {result.returncode}")
    check(result.stderr == "", failures, f"{label}: stderr should be empty, got {result.stderr!r}")
    check(
        "sync complete" in result.stdout.splitlines(),
        failures,
        f"{label}: stdout should include sync complete, got {result.stdout!r}",
    )


def expect_failure(
    result: subprocess.CompletedProcess[str],
    failures: list[str],
    label: str,
    required_stdout: str | None = None,
) -> None:
    check(result.returncode == 1, failures, f"{label}: expected exit 1, got {result.returncode}")
    check(result.stderr == "", failures, f"{label}: stderr should be empty, got {result.stderr!r}")
    if required_stdout is not None:
        check(
            required_stdout in result.stdout,
            failures,
            f"{label}: stdout should contain {required_stdout!r}, got {result.stdout!r}",
        )


def scenario_startup_errors(base: Path, failures: list[str]) -> None:
    first_a = make_peer(base, "first_a")
    first_b = make_peer(base, "first_b")
    result = run_sync([peer_arg(first_a), peer_arg(first_b)])
    expect_failure(
        result,
        failures,
        "013.13/013.14 first run without canon",
        "First sync? Mark the authoritative peer with a leading +",
    )

    reachable = make_peer(base, "reachable")
    missing_canon = base / "missing_canon"
    result = run_sync(["--dry-run", peer_arg(missing_canon, "+"), peer_arg(reachable)])
    expect_failure(result, failures, "013.12 unreachable canon")

    missing = base / "missing"
    result = run_sync(["--dry-run", peer_arg(missing), peer_arg(reachable)])
    expect_failure(result, failures, "013.15 fewer than two reachable peers")

    sub_a = make_peer(base, "sub_a", [history_row()])
    sub_b = make_peer(base, "sub_b", [history_row()])
    result = run_sync([peer_arg(sub_a, "-"), peer_arg(sub_b, "-")])
    expect_failure(
        result,
        failures,
        "013.16/013.17 no contributing peer",
        "No contributing peer reachable - cannot make sync decisions",
    )


def scenario_canon_wins(base: Path, failures: list[str]) -> None:
    canon = make_peer(base, "canon")
    other = make_peer(base, "canon_other")
    target = make_peer(base, "canon_target")
    write_file(canon / "canon.txt", b"canon", 1_700_001_000)
    write_file(other / "canon.txt", b"other-newer", 1_700_002_000)
    result = run_sync([peer_arg(canon, "+"), peer_arg(other), peer_arg(target)])
    expect_success(result, failures, "013.9/013.11 canon file wins")
    check(read_bytes(other / "canon.txt") == b"canon", failures, "013.9: canon file should replace other peer")
    check(read_bytes(target / "canon.txt") == b"canon", failures, "013.9: canon file should copy to missing peer")

    canon_empty = make_peer(base, "canon_empty")
    loser = make_peer(base, "canon_delete_loser")
    write_file(loser / "gone.txt", b"remove me", 1_700_003_000)
    result = run_sync([peer_arg(canon_empty, "+"), peer_arg(loser)])
    expect_success(result, failures, "013.10 canon absence deletes")
    check(not (loser / "gone.txt").exists(), failures, "013.10: canon absence should remove file from other peer")
    check(has_bak_entry(loser, "gone.txt"), failures, "013.10: deleted file should be displaced to BAK")


def scenario_unchanged_ties_and_targets(base: Path, failures: list[str]) -> None:
    p1 = make_peer(base, "unchanged_p1", [file_row("same.txt", 1_700_010_000, 3)])
    p2 = make_peer(base, "unchanged_p2", [file_row("same.txt", 1_700_010_004, 3)])
    p3 = make_peer(base, "unchanged_target")
    write_file(p1 / "same.txt", b"aaa", 1_700_010_000)
    write_file(p2 / "same.txt", b"bbb", 1_700_010_004)
    result = run_sync([peer_arg(p1), peer_arg(p2), peer_arg(p3)])
    expect_success(result, failures, "013.1/013.20-013.22 unchanged tied files")
    check(read_bytes(p1 / "same.txt") == b"aaa", failures, "013.39: identical tied source p1 should not be overwritten")
    check(read_bytes(p2 / "same.txt") == b"bbb", failures, "013.21/013.38/013.39: tied equal-size peers keep their bytes")
    check(
        read_bytes(p3 / "same.txt") in {b"aaa", b"bbb"},
        failures,
        "013.22/013.40: missing active peer should receive one tied source file",
    )


def scenario_modified_size_wins(base: Path, failures: list[str]) -> None:
    p1 = make_peer(base, "size_p1", [file_row("size.txt", 1_700_020_000, 1)])
    p2 = make_peer(base, "size_p2", [file_row("size.txt", 1_700_020_000, 1)])
    write_file(p1 / "size.txt", b"larger", 1_700_020_000)
    write_file(p2 / "size.txt", b"x", 1_700_020_000)
    result = run_sync([peer_arg(p1), peer_arg(p2)])
    expect_success(result, failures, "013.2/013.36 modified size wins")
    check(read_bytes(p2 / "size.txt") == b"larger", failures, "013.2/013.36: larger same-time modified file should win")


def scenario_modified_mtime_wins(base: Path, failures: list[str]) -> None:
    p1 = make_peer(base, "mtime_p1", [file_row("mtime.txt", 1_700_030_000, 4)])
    p2 = make_peer(base, "mtime_p2", [file_row("mtime.txt", 1_700_030_000, 4)])
    write_file(p1 / "mtime.txt", b"new1", 1_700_030_010)
    write_file(p2 / "mtime.txt", b"old2", 1_700_030_000)
    result = run_sync([peer_arg(p1), peer_arg(p2)])
    expect_success(result, failures, "013.3/013.23/013.35 newer modified file")
    check(read_bytes(p2 / "mtime.txt") == b"new1", failures, "013.3/013.23/013.35: newer modified file should win")
    check(read_bytes(p1 / "mtime.txt") == b"new1", failures, "013.44: peer already matching winner should not be copied over")


def scenario_new_files_and_no_row_peers(base: Path, failures: list[str]) -> None:
    p1 = make_peer(base, "new_p1", [history_row()])
    p2 = make_peer(base, "new_p2", [history_row()])
    p3 = make_peer(base, "new_p3", [history_row()])
    write_file(p1 / "new.txt", b"newest", 1_700_040_020)
    write_file(p2 / "new.txt", b"older", 1_700_040_000)
    result = run_sync([peer_arg(p1), peer_arg(p2), peer_arg(p3)])
    expect_success(result, failures, "013.5/013.8/013.24/013.25 new file winner")
    check(read_bytes(p2 / "new.txt") == b"newest", failures, "013.24: newest new file should replace older new file")
    check(read_bytes(p3 / "new.txt") == b"newest", failures, "013.25: no-row absent peer should receive new winner")


def scenario_deleted_existing_comparisons(base: Path, failures: list[str]) -> None:
    p1 = make_peer(base, "delete_tie_p1", [file_row("revive.txt", 1_700_050_000, 5, deleted_time=1_700_049_000)])
    p2 = make_peer(base, "delete_tie_p2", [file_row("revive.txt", 1_700_050_000, 5, deleted_time=1_700_050_004)])
    write_file(p1 / "revive.txt", b"alive", 1_700_050_000)
    result = run_sync([peer_arg(p1), peer_arg(p2)])
    expect_success(result, failures, "013.4/013.6/013.26/013.29/013.37 existing tied with deletion")
    check(read_bytes(p2 / "revive.txt") == b"alive", failures, "013.29/013.37: existing file tied with deletion should win")

    live = make_peer(base, "delete_newer_live", [file_row("doomed.txt", 1_700_060_000, 6)])
    deleted_a = make_peer(base, "delete_newer_a", [file_row("doomed.txt", 1_700_060_000, 6, deleted_time=1_700_060_008)])
    deleted_b = make_peer(base, "delete_newer_b", [file_row("doomed.txt", 1_700_060_000, 6, deleted_time=1_700_060_012)])
    write_file(live / "doomed.txt", b"doomed", 1_700_060_000)
    result = run_sync([peer_arg(live), peer_arg(deleted_a), peer_arg(deleted_b)])
    expect_success(result, failures, "013.27/013.28 newer deletion wins")
    check(not (live / "doomed.txt").exists(), failures, "013.28: deletion newer than file should remove existing file")
    check(has_bak_entry(live, "doomed.txt"), failures, "013.28: deletion winner should displace live file to BAK")


def scenario_absent_unconfirmed(base: Path, failures: list[str]) -> None:
    live = make_peer(base, "absent_live", [file_row("maybe.txt", 1_700_070_000, 5)])
    absent = make_peer(base, "absent_deleted", [file_row("maybe.txt", 1_700_070_000, 5, last_seen=1_700_070_010)])
    write_file(live / "maybe.txt", b"maybe", 1_700_070_000)
    result = run_sync([peer_arg(live), peer_arg(absent)])
    expect_success(result, failures, "013.7/013.30 absent-unconfirmed deletion vote")
    check(not (live / "maybe.txt").exists(), failures, "013.30: newer absent-unconfirmed last_seen should delete file")

    source = make_peer(base, "absent_source", [file_row("copy.txt", 1_700_080_000, 4)])
    null_seen = make_peer(base, "absent_null_seen", [file_row("copy.txt", 1_700_080_000, 4, last_seen=None)])
    near_seen = make_peer(base, "absent_near_seen", [file_row("copy.txt", 1_700_080_000, 4, last_seen=1_700_080_004)])
    write_file(source / "copy.txt", b"copy", 1_700_080_000)
    result = run_sync([peer_arg(source), peer_arg(null_seen), peer_arg(near_seen)])
    expect_success(result, failures, "013.31/013.32/013.33 absent-unconfirmed no vote")
    check(read_bytes(null_seen / "copy.txt") == b"copy", failures, "013.31/013.33: NULL last_seen peer should receive existing file")
    check(read_bytes(near_seen / "copy.txt") == b"copy", failures, "013.32/013.33: near last_seen peer should receive existing file")


def scenario_subordinate_rules(base: Path, failures: list[str]) -> None:
    source = make_peer(base, "sub_source", [file_row("sub.txt", 1_700_090_000, 5)])
    neutral = make_peer(base, "sub_neutral", [history_row()])
    subordinate = make_peer(base, "sub_target", [history_row()])
    write_file(source / "sub.txt", b"group", 1_700_090_000)
    write_file(subordinate / "sub.txt", b"subordinate-newer", 1_700_091_000)
    result = run_sync([peer_arg(source), peer_arg(neutral), peer_arg(subordinate, "-")])
    expect_success(result, failures, "013.18/013.19 subordinate ignored but targeted")
    check(read_bytes(neutral / "sub.txt") == b"group", failures, "013.19: active peer should receive contributing outcome")
    check(read_bytes(subordinate / "sub.txt") == b"group", failures, "013.18/013.19: subordinate file should not influence decision")


def scenario_all_absent_no_row_displaces_subordinate(base: Path, failures: list[str]) -> None:
    p1 = make_peer(base, "absent_no_row_p1", [history_row()])
    p2 = make_peer(base, "absent_no_row_p2", [history_row()])
    subordinate = make_peer(base, "absent_no_row_sub", [history_row()])
    write_file(subordinate / "orphan.txt", b"orphan", 1_700_100_000)
    result = run_sync([peer_arg(p1), peer_arg(p2), peer_arg(subordinate, "-")])
    expect_success(result, failures, "013.41/013.42/013.43 all contributing absent no row")
    check(not (subordinate / "orphan.txt").exists(), failures, "013.43: subordinate-only file should be displaced")
    check(has_bak_entry(subordinate, "orphan.txt"), failures, "013.43: displaced subordinate-only file should be in BAK")


def main() -> int:
    failures: list[str] = []
    scenarios = [
        scenario_startup_errors,
        scenario_canon_wins,
        scenario_unchanged_ties_and_targets,
        scenario_modified_size_wins,
        scenario_modified_mtime_wins,
        scenario_new_files_and_no_row_peers,
        scenario_deleted_existing_comparisons,
        scenario_absent_unconfirmed,
        scenario_subordinate_rules,
        scenario_all_absent_no_row_displaces_subordinate,
    ]

    with tempfile.TemporaryDirectory(prefix="ks_013_") as tmp:
        base = Path(tmp)
        for scenario in scenarios:
            try:
                scenario(base, failures)
            except subprocess.TimeoutExpired as exc:
                failures.append(f"{scenario.__name__}: process timed out after {exc.timeout} seconds")
            except Exception as exc:
                failures.append(f"{scenario.__name__}: unexpected exception {type(exc).__name__}: {exc}")

    if failures:
        print("test_013_file_decisions failed:")
        for failure in failures:
            print(f"- {failure}")
        return 1

    print("test_013_file_decisions passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
