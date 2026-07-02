# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

import os
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import time
from datetime import datetime, timezone
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

LITERAL_WORKSPACE_ROOT = Path("/home/ace/Desktop/prjx/kitchensync")
WORKSPACE_ROOT = (
    LITERAL_WORKSPACE_ROOT
    if LITERAL_WORKSPACE_ROOT.exists()
    else Path(__file__).resolve().parents[1]
)
KITCHENSYNC_EXE = WORKSPACE_ROOT / "released" / "kitchensync.exe"

TIMESTAMP_REPLACEMENT = "2024-01-02_03-04-05_000000Z"
OLD_LAST_SEEN = "2024-01-03_00-00-00_000000Z"
OLDER_LAST_SEEN = "2024-01-02_00-00-00_000000Z"
OLD_TOMBSTONE = "2000-01-01_00-00-00_000000Z"

PRIME64_1 = 11400714785074694791
PRIME64_2 = 14029467366897019727
PRIME64_3 = 1609587929392839161
PRIME64_4 = 9650029242287828579
PRIME64_5 = 2870177450012600261
MASK64 = (1 << 64) - 1
BASE62 = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"


def rol64(value, bits):
    return ((value << bits) | (value >> (64 - bits))) & MASK64


def round64(acc, lane):
    acc = (acc + lane * PRIME64_2) & MASK64
    acc = rol64(acc, 31)
    return (acc * PRIME64_1) & MASK64


def merge_round(acc, lane):
    acc ^= round64(0, lane)
    return (acc * PRIME64_1 + PRIME64_4) & MASK64


def xxhash64(data):
    data = data.encode("utf-8")
    length = len(data)
    offset = 0
    if length >= 32:
        v1 = (PRIME64_1 + PRIME64_2) & MASK64
        v2 = PRIME64_2
        v3 = 0
        v4 = (-PRIME64_1) & MASK64
        limit = length - 32
        while offset <= limit:
            v1 = round64(v1, int.from_bytes(data[offset : offset + 8], "little"))
            offset += 8
            v2 = round64(v2, int.from_bytes(data[offset : offset + 8], "little"))
            offset += 8
            v3 = round64(v3, int.from_bytes(data[offset : offset + 8], "little"))
            offset += 8
            v4 = round64(v4, int.from_bytes(data[offset : offset + 8], "little"))
            offset += 8
        acc = (
            rol64(v1, 1) + rol64(v2, 7) + rol64(v3, 12) + rol64(v4, 18)
        ) & MASK64
        acc = merge_round(acc, v1)
        acc = merge_round(acc, v2)
        acc = merge_round(acc, v3)
        acc = merge_round(acc, v4)
    else:
        acc = PRIME64_5
    acc = (acc + length) & MASK64
    while offset + 8 <= length:
        lane = round64(0, int.from_bytes(data[offset : offset + 8], "little"))
        acc ^= lane
        acc = (rol64(acc, 27) * PRIME64_1 + PRIME64_4) & MASK64
        offset += 8
    if offset + 4 <= length:
        acc ^= int.from_bytes(data[offset : offset + 4], "little") * PRIME64_1
        acc = (rol64(acc, 23) * PRIME64_2 + PRIME64_3) & MASK64
        offset += 4
    while offset < length:
        acc ^= data[offset] * PRIME64_5
        acc = (rol64(acc, 11) * PRIME64_1) & MASK64
        offset += 1
    acc ^= acc >> 33
    acc = (acc * PRIME64_2) & MASK64
    acc ^= acc >> 29
    acc = (acc * PRIME64_3) & MASK64
    acc ^= acc >> 32
    return acc & MASK64


def base62_11(value):
    chars = []
    for _ in range(11):
        value, rem = divmod(value, 62)
        chars.append(BASE62[rem])
    return "".join(reversed(chars))


def path_id(relpath):
    return base62_11(xxhash64(relpath))


def parent_id(relpath):
    parent = Path(relpath).parent.as_posix()
    return path_id("/") if parent == "." else path_id(parent)


def basename(relpath):
    return Path(relpath).name


def format_mtime(path):
    return datetime.fromtimestamp(path.stat().st_mtime, timezone.utc).strftime(
        "%Y-%m-%d_%H-%M-%S_%fZ"
    )


def set_mtime(path, stamp):
    when = datetime.strptime(stamp, "%Y-%m-%d_%H-%M-%S_%fZ").replace(
        tzinfo=timezone.utc
    )
    seconds = when.timestamp()
    os.utime(path, (seconds, seconds))


def write_file(path, text, stamp):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")
    set_mtime(path, stamp)


def snapshot_path(peer):
    return peer / ".kitchensync" / "snapshot.db"


def create_snapshot(peer, rows):
    db_path = snapshot_path(peer)
    db_path.parent.mkdir(parents=True, exist_ok=True)
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
        db.execute("CREATE INDEX snapshot_parent_id ON snapshot(parent_id)")
        db.execute("CREATE INDEX snapshot_last_seen ON snapshot(last_seen)")
        db.execute("CREATE INDEX snapshot_deleted_time ON snapshot(deleted_time)")
        for relpath, mod_time, byte_size, last_seen, deleted_time in rows:
            db.execute(
                """
                INSERT INTO snapshot
                    (id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time)
                VALUES (?, ?, ?, ?, ?, ?, ?)
                """,
                (
                    path_id(relpath),
                    parent_id(relpath),
                    basename(relpath),
                    mod_time,
                    byte_size,
                    last_seen,
                    deleted_time,
                ),
            )


def read_row(peer, relpath):
    with sqlite3.connect(snapshot_path(peer)) as db:
        row = db.execute(
            """
            SELECT id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time
            FROM snapshot
            WHERE id = ?
            """,
            (path_id(relpath),),
        ).fetchone()
    if row is None:
        return None
    keys = ("id", "parent_id", "basename", "mod_time", "byte_size", "last_seen", "deleted_time")
    return dict(zip(keys, row))


def run_sync(args):
    return subprocess.run(
        [str(KITCHENSYNC_EXE), *[str(arg) for arg in args]],
        cwd=str(WORKSPACE_ROOT),
        text=True,
        encoding="utf-8",
        errors="replace",
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=40,
        check=False,
    )


def check(failures, condition, message):
    if not condition:
        failures.append(message)


def check_success(failures, result, label):
    check(failures, result.returncode == 0, f"{label}: expected exit 0, got {result.returncode}; stdout={result.stdout!r}")
    check(failures, result.stderr == "", f"{label}: stderr must be empty, got {result.stderr!r}")
    check(failures, "sync complete" in result.stdout.splitlines(), f"{label}: missing sync complete line")


def scenario_present_copy_and_directory(failures):
    with tempfile.TemporaryDirectory(prefix="ks015_present_") as tmp:
        root = Path(tmp)
        peer_a = root / "a"
        peer_b = root / "b"
        peer_a.mkdir()
        peer_b.mkdir()
        write_file(peer_a / "alpha.txt", "alpha\n", TIMESTAMP_REPLACEMENT)
        (peer_a / "emptydir").mkdir()
        set_mtime(peer_a / "emptydir", TIMESTAMP_REPLACEMENT)

        result = run_sync(["--verbosity", "error", f"+{peer_a}", peer_b])
        check_success(failures, result, "present/copy/directory run")

        check(failures, (peer_b / "alpha.txt").read_text(encoding="utf-8") == "alpha\n", "015.13: copied file content should exist on destination")
        source_row = read_row(peer_a, "alpha.txt")
        dest_row = read_row(peer_b, "alpha.txt")
        dir_row = read_row(peer_b, "emptydir")
        check(failures, source_row is not None, "015.1-015.4: source listing should create alpha.txt snapshot row")
        if source_row:
            check(failures, source_row["mod_time"] == format_mtime(peer_a / "alpha.txt"), "015.1: listed source row should record current mod_time")
            check(failures, source_row["byte_size"] == 6, "015.2: listed source row should record current byte_size")
            check(failures, source_row["last_seen"] is not None, "015.3: listed source row should record last_seen")
            check(failures, source_row["deleted_time"] is None, "015.4: listed source row should clear deleted_time")
        check(failures, dest_row is not None, "015.13: destination copy should create alpha.txt snapshot row")
        if dest_row:
            check(failures, dest_row["mod_time"] == format_mtime(peer_a / "alpha.txt"), "015.8: destination row should record winning mod_time after copy")
            check(failures, dest_row["byte_size"] == 6, "015.9: destination row should record winning byte_size after copy")
            check(failures, dest_row["deleted_time"] is None, "015.10: destination row should have deleted_time NULL after copy")
            check(failures, dest_row["last_seen"] is not None, "015.13: completed copy should set destination last_seen")
        check(failures, dir_row is not None, "015.16-015.19: created directory should have a snapshot row")
        if dir_row:
            check(failures, dir_row["mod_time"] == format_mtime(peer_b / "emptydir"), "015.16: created directory row should record current mod_time")
            check(failures, dir_row["byte_size"] == -1, "015.17: created directory row should record byte_size -1")
            check(failures, dir_row["last_seen"] is not None, "015.18: created directory row should record last_seen")
            check(failures, dir_row["deleted_time"] is None, "015.19: created directory row should clear deleted_time")


def scenario_absence_and_file_displacement(failures):
    with tempfile.TemporaryDirectory(prefix="ks015_absence_") as tmp:
        root = Path(tmp)
        peer_a = root / "a"
        peer_b = root / "b"
        peer_a.mkdir()
        peer_b.mkdir()
        write_file(peer_b / "gone.txt", "gone\n", "2024-01-01_00-00-00_000000Z")
        write_file(peer_b / "tomb.txt", "tomb\n", "2024-01-01_00-00-00_000000Z")
        create_snapshot(
            peer_a,
            [
                ("gone.txt", "2024-01-01_00-00-00_000000Z", 5, OLD_LAST_SEEN, None),
                ("tomb.txt", "2024-01-01_00-00-00_000000Z", 5, OLDER_LAST_SEEN, OLD_LAST_SEEN),
            ],
        )
        create_snapshot(
            peer_b,
            [
                ("gone.txt", "2024-01-01_00-00-00_000000Z", 5, OLDER_LAST_SEEN, None),
                ("tomb.txt", "2024-01-01_00-00-00_000000Z", 5, OLDER_LAST_SEEN, None),
            ],
        )

        result = run_sync(["--verbosity", "error", peer_a, peer_b])
        check_success(failures, result, "absence/file displacement run")

        gone_a = read_row(peer_a, "gone.txt")
        tomb_a = read_row(peer_a, "tomb.txt")
        gone_b = read_row(peer_b, "gone.txt")
        check(failures, not (peer_b / "gone.txt").exists(), "015.21: losing live file should be displaced from peer")
        if gone_a:
            check(failures, gone_a["deleted_time"] == OLD_LAST_SEEN, "015.5: confirmed absent row should copy previous last_seen into deleted_time")
            check(failures, gone_a["last_seen"] == OLD_LAST_SEEN, "015.6: confirmed absent row should not change last_seen")
        else:
            failures.append("015.5-015.6: gone.txt row missing after confirmed absence")
        if tomb_a:
            check(failures, tomb_a["last_seen"] == OLDER_LAST_SEEN and tomb_a["deleted_time"] == OLD_LAST_SEEN, "015.7: existing tombstone row should not change")
        else:
            failures.append("015.7: tomb.txt tombstone row missing")
        if gone_b:
            check(failures, gone_b["deleted_time"] == OLDER_LAST_SEEN, "015.21: displaced file row should use previous last_seen as deleted_time")
        else:
            failures.append("015.21: displaced gone.txt row missing on peer_b")


def scenario_directory_displacement_cascade(failures):
    with tempfile.TemporaryDirectory(prefix="ks015_cascade_") as tmp:
        root = Path(tmp)
        peer_a = root / "a"
        peer_b = root / "b"
        peer_c = root / "c"
        for peer in (peer_a, peer_b, peer_c):
            peer.mkdir()
        for peer in (peer_a, peer_c):
            write_file(peer / "conflict" / "child.txt", "child\n", "2024-01-01_00-00-00_000000Z")
            write_file(peer / "conflict" / "old_tomb.txt", "old\n", "2024-01-01_00-00-00_000000Z")
            write_file(peer / "outside.txt", "outside\n", "2024-01-01_00-00-00_000000Z")
        write_file(peer_b / "conflict", "winning file\n", "2024-01-04_00-00-00_000000Z")

        create_snapshot(
            peer_a,
            [
                ("conflict", "2024-01-01_00-00-00_000000Z", -1, "2024-01-03_00-00-00_000000Z", None),
                ("conflict/child.txt", "2024-01-01_00-00-00_000000Z", 6, "2024-01-02_00-00-00_000000Z", None),
                ("conflict/old_tomb.txt", "2024-01-01_00-00-00_000000Z", 4, "2024-01-02_00-00-00_000000Z", "2024-01-02_00-00-00_000000Z"),
                ("outside.txt", "2024-01-01_00-00-00_000000Z", 8, "2024-01-02_00-00-00_000000Z", None),
            ],
        )
        create_snapshot(
            peer_b,
            [("conflict", "2024-01-04_00-00-00_000000Z", 13, "2024-01-04_00-00-01_000000Z", None)],
        )
        create_snapshot(
            peer_c,
            [
                ("conflict", "2024-01-01_00-00-00_000000Z", -1, "2024-01-05_00-00-00_000000Z", None),
                ("conflict/child.txt", "2024-01-01_00-00-00_000000Z", 6, "2024-01-02_00-00-00_000000Z", None),
            ],
        )

        result = run_sync(["--verbosity", "error", peer_a, peer_b, peer_c])
        check_success(failures, result, "directory cascade run")

        a_dir = read_row(peer_a, "conflict")
        a_child = read_row(peer_a, "conflict/child.txt")
        a_tomb = read_row(peer_a, "conflict/old_tomb.txt")
        a_outside = read_row(peer_a, "outside.txt")
        c_child = read_row(peer_c, "conflict/child.txt")
        b_child = read_row(peer_b, "conflict/child.txt")
        if a_dir and a_child:
            check(failures, a_dir["deleted_time"] == "2024-01-03_00-00-00_000000Z", "015.21: displaced directory row should use its previous last_seen")
            check(failures, a_child["deleted_time"] == "2024-01-03_00-00-00_000000Z", "015.23: descendant row should receive directory deletion estimate")
        else:
            failures.append("015.21/015.23: peer_a displaced directory or child row missing")
        if a_tomb:
            check(failures, a_tomb["deleted_time"] == "2024-01-02_00-00-00_000000Z", "015.24: already tombstoned descendant should not change")
        else:
            failures.append("015.24: existing tombstone descendant missing")
        if a_outside:
            check(failures, a_outside["deleted_time"] is None, "015.25: cascade should not change rows outside displaced subtree")
        else:
            failures.append("015.25: outside row missing")
        if c_child:
            check(failures, c_child["deleted_time"] == "2024-01-05_00-00-00_000000Z", "015.26: each peer cascade should use only its own snapshot database")
        else:
            failures.append("015.26: peer_c child row missing")
        check(failures, b_child is None, "015.26: peer_b snapshot should not receive descendant rows from another peer")


def scenario_snapshot_cleanup(failures):
    with tempfile.TemporaryDirectory(prefix="ks015_cleanup_") as tmp:
        root = Path(tmp)
        peer_a = root / "a"
        peer_b = root / "b"
        peer_a.mkdir()
        peer_b.mkdir()
        write_file(peer_b / "survivor.txt", "survives\n", "2024-01-04_00-00-00_000000Z")
        create_snapshot(
            peer_a,
            [
                ("old_tomb.txt", "2000-01-01_00-00-00_000000Z", 1, "2000-01-01_00-00-00_000000Z", OLD_TOMBSTONE),
                ("stale/orphan.txt", "2000-01-01_00-00-00_000000Z", 1, "2000-01-01_00-00-00_000000Z", None),
                ("survivor.txt", "2000-01-01_00-00-00_000000Z", 8, "2000-01-01_00-00-00_000000Z", OLD_TOMBSTONE),
            ],
        )
        create_snapshot(
            peer_b,
            [("survivor.txt", "2024-01-04_00-00-00_000000Z", 8, "2024-01-04_00-00-01_000000Z", None)],
        )

        result = run_sync(["--verbosity", "error", "--keep-del-days", "1", peer_a, peer_b])
        check_success(failures, result, "snapshot cleanup run")

        check(failures, read_row(peer_a, "old_tomb.txt") is None, "015.27: old tombstone row should be removed by cleanup")
        check(failures, read_row(peer_a, "stale/orphan.txt") is None, "015.29: old unreachable non-tombstone row should be removed by cleanup")
        check(failures, (peer_a / "survivor.txt").exists(), "015.28: sync decision should still copy live file even with eligible cleanup rows present")


def main():
    failures = []
    if not KITCHENSYNC_EXE.exists():
        failures.append(f"released executable does not exist: {KITCHENSYNC_EXE}")
    else:
        for scenario in (
            scenario_present_copy_and_directory,
            scenario_absence_and_file_displacement,
            scenario_directory_displacement_cascade,
            scenario_snapshot_cleanup,
        ):
            try:
                scenario(failures)
            except subprocess.TimeoutExpired as exc:
                failures.append(f"{scenario.__name__}: process timed out after {exc.timeout} seconds")
            except Exception as exc:
                failures.append(f"{scenario.__name__}: unexpected test error: {exc!r}")

    # not reasonably testable: 015.8, 015.9, 015.10, 015.11, 015.12
    #   The required state exists after enqueue but before copy completion; the
    #   released CLI exposes no stable pause point for inspecting that moment.
    # not reasonably testable: 015.14, 015.15
    #   Forcing process exit during a queued copy would depend on timing and file
    #   size rather than a specified, observable product control.
    # not reasonably testable: 015.20, 015.22
    #   Directory creation and displacement failure require intentionally broken
    #   filesystem permissions or transport faults, outside happy-path E2E setup.

    if failures:
        print("FAIL")
        for failure in failures:
            print(f"- {failure}")
        return 1
    print("PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
