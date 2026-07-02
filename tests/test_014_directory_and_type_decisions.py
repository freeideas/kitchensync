# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

from __future__ import annotations

import os
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import traceback
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

PRIME64_1 = 11400714785074694791
PRIME64_2 = 14029467366897019727
PRIME64_3 = 1609587929392839161
PRIME64_4 = 9650029242287828579
PRIME64_5 = 2870177450012600261
MASK64 = (1 << 64) - 1
BASE62 = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"


def xxh64(data: bytes) -> int:
    def rol(value: int, bits: int) -> int:
        return ((value << bits) | (value >> (64 - bits))) & MASK64

    def round_acc(acc: int, lane: int) -> int:
        acc = (acc + lane * PRIME64_2) & MASK64
        acc = rol(acc, 31)
        acc = (acc * PRIME64_1) & MASK64
        return acc

    def merge_round(acc: int, lane: int) -> int:
        acc ^= round_acc(0, lane)
        acc = (acc * PRIME64_1 + PRIME64_4) & MASK64
        return acc

    index = 0
    length = len(data)
    if length >= 32:
        v1 = (PRIME64_1 + PRIME64_2) & MASK64
        v2 = PRIME64_2
        v3 = 0
        v4 = (-PRIME64_1) & MASK64
        limit = length - 32
        while index <= limit:
            v1 = round_acc(v1, int.from_bytes(data[index : index + 8], "little"))
            index += 8
            v2 = round_acc(v2, int.from_bytes(data[index : index + 8], "little"))
            index += 8
            v3 = round_acc(v3, int.from_bytes(data[index : index + 8], "little"))
            index += 8
            v4 = round_acc(v4, int.from_bytes(data[index : index + 8], "little"))
            index += 8
        h64 = (
            rol(v1, 1)
            + rol(v2, 7)
            + rol(v3, 12)
            + rol(v4, 18)
        ) & MASK64
        h64 = merge_round(h64, v1)
        h64 = merge_round(h64, v2)
        h64 = merge_round(h64, v3)
        h64 = merge_round(h64, v4)
    else:
        h64 = PRIME64_5
    h64 = (h64 + length) & MASK64
    while index + 8 <= length:
        lane = int.from_bytes(data[index : index + 8], "little")
        h64 ^= round_acc(0, lane)
        h64 = (rol(h64, 27) * PRIME64_1 + PRIME64_4) & MASK64
        index += 8
    if index + 4 <= length:
        lane4 = int.from_bytes(data[index : index + 4], "little")
        h64 ^= (lane4 * PRIME64_1) & MASK64
        h64 = (rol(h64, 23) * PRIME64_2 + PRIME64_3) & MASK64
        index += 4
    while index < length:
        h64 ^= (data[index] * PRIME64_5) & MASK64
        h64 = (rol(h64, 11) * PRIME64_1) & MASK64
        index += 1
    h64 ^= h64 >> 33
    h64 = (h64 * PRIME64_2) & MASK64
    h64 ^= h64 >> 29
    h64 = (h64 * PRIME64_3) & MASK64
    h64 ^= h64 >> 32
    return h64 & MASK64


def base62_11(value: int) -> str:
    chars = []
    for _ in range(11):
        value, digit = divmod(value, 62)
        chars.append(BASE62[digit])
    return "".join(reversed(chars))


def path_id(relpath: str) -> str:
    return base62_11(xxh64(relpath.encode("utf-8")))


def parent_id(relpath: str) -> str:
    if "/" not in relpath:
        return path_id("/")
    return path_id(relpath.rsplit("/", 1)[0])


def ts(epoch: int) -> str:
    return datetime.fromtimestamp(epoch, timezone.utc).strftime(
        "%Y-%m-%d_%H-%M-%S_%fZ"
    )


def touch_file(path: Path, text: str, epoch: int) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")
    os.utime(path, (epoch, epoch))


def touch_dir(path: Path, epoch: int) -> None:
    path.mkdir(parents=True, exist_ok=True)
    os.utime(path, (epoch, epoch))


def make_snapshot(peer: Path, rows: list[dict[str, object]]) -> None:
    db_path = peer / ".kitchensync" / "snapshot.db"
    db_path.parent.mkdir(parents=True, exist_ok=True)
    with sqlite3.connect(str(db_path)) as conn:
        conn.execute("PRAGMA journal_mode=DELETE")
        conn.execute(
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
        for row in rows:
            relpath = str(row["path"])
            conn.execute(
                """
                INSERT INTO snapshot
                (id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time)
                VALUES (?, ?, ?, ?, ?, ?, ?)
                """,
                (
                    path_id(relpath),
                    parent_id(relpath),
                    relpath.rsplit("/", 1)[-1],
                    row["mod_time"],
                    row["byte_size"],
                    row.get("last_seen"),
                    row.get("deleted_time"),
                ),
            )
        conn.commit()


def run_sync(*peers: str, extra_args: list[str] | None = None) -> subprocess.CompletedProcess[str]:
    args = [str(KITCHENSYNC_EXE)]
    if extra_args:
        args.extend(extra_args)
    args.extend(peers)
    return subprocess.run(
        args,
        cwd=str(WORKSPACE_ROOT),
        text=True,
        encoding="utf-8",
        errors="replace",
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=45,
        check=False,
    )


def assert_run_ok(result: subprocess.CompletedProcess[str], label: str) -> None:
    assert result.returncode == 0, (
        f"{label}: expected exit 0, got {result.returncode}\n"
        f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}"
    )
    assert result.stderr == "", f"{label}: expected empty stderr, got {result.stderr!r}"


def bak_matches(peer: Path, relative_parts: tuple[str, ...]) -> list[Path]:
    root = peer / ".kitchensync" / "BAK"
    if not root.exists():
        return []
    matches = []
    for timestamp_dir in root.iterdir():
        candidate = timestamp_dir.joinpath(*relative_parts)
        if candidate.exists():
            matches.append(candidate)
    return matches


def case_canon_directory_creation_and_deletion(base: Path) -> None:
    p1 = base / "canon"
    p2 = base / "receiver"
    touch_file(p1 / "Shared" / "note.txt", "canon\n", 1_735_000_000)
    result = run_sync("+" + str(p1), str(p2))
    assert_run_ok(result, "014.2 canon live directory")
    assert (p2 / "Shared").is_dir(), "014.2: canon live directory was not created"
    assert (p2 / "Shared" / "note.txt").read_text(encoding="utf-8") == "canon\n"

    shutil.rmtree(p1 / "Shared")
    result = run_sync("+" + str(p1), str(p2))
    assert_run_ok(result, "014.3 canon missing directory")
    assert not (p2 / "Shared").exists(), "014.3: canon-missing path still exists"


def case_all_live_directory_votes_create_missing(base: Path) -> None:
    p1 = base / "p1"
    p2 = base / "p2"
    p3 = base / "p3"
    touch_dir(p1 / "empty", 1_735_000_100)
    touch_dir(p2 / "empty", 1_735_000_200)
    make_snapshot(
        p1,
        [{"path": "empty", "mod_time": ts(1_735_000_100), "byte_size": -1, "last_seen": ts(1_735_000_100)}],
    )
    make_snapshot(
        p2,
        [{"path": "empty", "mod_time": ts(1_735_000_199), "byte_size": -1, "last_seen": ts(1_735_000_100)}],
    )
    make_snapshot(p3, [])
    result = run_sync(str(p1), str(p2), str(p3))
    assert_run_ok(result, "014.4 live directory votes")
    assert (p3 / "empty").is_dir(), "014.4: live directory was not created on peer with no row"


def case_empty_directory_deletion_ignores_directory_mtime(base: Path) -> None:
    live = base / "live"
    absent = base / "absent"
    target = base / "target"
    touch_dir(live / "empty", 1_735_001_000)
    make_snapshot(
        live,
        [{"path": "empty", "mod_time": ts(1_735_000_000), "byte_size": -1, "last_seen": ts(1_735_000_000)}],
    )
    make_snapshot(
        absent,
        [{
            "path": "empty",
            "mod_time": ts(1_735_000_000),
            "byte_size": -1,
            "last_seen": ts(1_735_000_200),
            "deleted_time": ts(1_735_000_200),
        }],
    )
    make_snapshot(target, [])
    result = run_sync(str(live), str(absent), str(target))
    assert_run_ok(result, "014.1 empty directory deletion")
    assert not (live / "empty").exists(), "014.1/014.14: empty live directory survived by directory mtime"
    assert not (target / "empty").exists(), "014.15: deletion winner recreated a missing directory"
    assert bak_matches(live, ("empty",)), "014.13: displaced directory was not moved to BAK"


def case_directory_deletion_uses_last_seen_when_deleted_time_absent(base: Path) -> None:
    live = base / "live"
    absent = base / "absent"
    touch_dir(live / "gone")
    make_snapshot(
        live,
        [{"path": "gone", "mod_time": ts(1_735_000_000), "byte_size": -1, "last_seen": ts(1_735_000_000)}],
    )
    make_snapshot(
        absent,
        [{"path": "gone", "mod_time": ts(1_735_000_000), "byte_size": -1, "last_seen": ts(1_735_000_400)}],
    )
    result = run_sync(str(live), str(absent))
    assert_run_ok(result, "014.8 directory deletion last_seen")
    assert not (live / "gone").exists(), "014.8: absent row with no deleted_time did not delete empty directory"


def case_directory_survives_and_recurses(base: Path) -> None:
    live = base / "live"
    absent = base / "absent"
    touch_file(live / "dir" / "old.txt", "old\n", 1_735_000_100)
    touch_file(live / "dir" / "new.txt", "new\n", 1_735_000_300)
    touch_dir(live / "dir" / "subdir", 1_735_000_400)
    make_snapshot(
        live,
        [
            {"path": "dir", "mod_time": ts(1_735_000_000), "byte_size": -1, "last_seen": ts(1_735_000_000)},
            {"path": "dir/old.txt", "mod_time": ts(1_735_000_100), "byte_size": 4, "last_seen": ts(1_735_000_100)},
            {"path": "dir/new.txt", "mod_time": ts(1_735_000_200), "byte_size": 4, "last_seen": ts(1_735_000_200)},
            {"path": "dir/subdir", "mod_time": ts(1_735_000_400), "byte_size": -1, "last_seen": ts(1_735_000_400)},
        ],
    )
    make_snapshot(
        absent,
        [
            {
                "path": "dir",
                "mod_time": ts(1_735_000_000),
                "byte_size": -1,
                "last_seen": ts(1_735_000_200),
                "deleted_time": ts(1_735_000_200),
            },
            {
                "path": "dir/old.txt",
                "mod_time": ts(1_735_000_100),
                "byte_size": 4,
                "last_seen": ts(1_735_000_200),
                "deleted_time": ts(1_735_000_200),
            },
        ],
    )
    result = run_sync(str(live), str(absent))
    assert_run_ok(result, "014.17 surviving directory")
    assert (absent / "dir").is_dir(), "014.17: directory with newer file evidence did not survive"
    assert (absent / "dir" / "new.txt").read_text(encoding="utf-8") == "new\n", (
        "014.18/014.19: sync did not recurse and propagate newer child file"
    )
    assert not (live / "dir" / "old.txt").exists(), "014.20: older child file was not removed during recursion"
    assert (live / "dir" / "subdir").is_dir(), (
        "014.10: child directory mtime appears to have been treated as file survival evidence"
    )


def case_newest_directory_deletion_displaces_whole_subtree(base: Path) -> None:
    live = base / "live"
    old_delete = base / "old_delete"
    new_delete = base / "new_delete"
    touch_file(live / "tree" / "nested" / "file.txt", "keep?\n", 1_735_000_300)
    make_snapshot(
        live,
        [
            {"path": "tree", "mod_time": ts(1_735_000_000), "byte_size": -1, "last_seen": ts(1_735_000_000)},
            {"path": "tree/nested", "mod_time": ts(1_735_000_000), "byte_size": -1, "last_seen": ts(1_735_000_000)},
            {"path": "tree/nested/file.txt", "mod_time": ts(1_735_000_300), "byte_size": 6, "last_seen": ts(1_735_000_300)},
        ],
    )
    delete_rows = [
        {
            "path": "tree",
            "mod_time": ts(1_735_000_000),
            "byte_size": -1,
            "last_seen": ts(1_735_000_000),
            "deleted_time": ts(1_735_000_100),
        }
    ]
    make_snapshot(old_delete, delete_rows)
    make_snapshot(
        new_delete,
        [{
            "path": "tree",
            "mod_time": ts(1_735_000_000),
            "byte_size": -1,
            "last_seen": ts(1_735_000_000),
            "deleted_time": ts(1_735_000_400),
        }],
    )
    result = run_sync(str(live), str(old_delete), str(new_delete))
    assert_run_ok(result, "014.12 newest directory deletion")
    assert not (live / "tree").exists(), "014.12/014.13: newest deletion estimate did not win"
    assert bak_matches(live, ("tree", "nested", "file.txt")), (
        "014.24: directory was not displaced as one subtree before child visits"
    )


def case_absent_snapshot_rows_delete_subordinate_directory(base: Path) -> None:
    p1 = base / "p1"
    p2 = base / "p2"
    sub = base / "sub"
    touch_dir(sub / "orphan")
    make_snapshot(
        p1,
        [{
            "path": "orphan",
            "mod_time": ts(1_735_000_000),
            "byte_size": -1,
            "last_seen": ts(1_735_000_000),
            "deleted_time": ts(1_735_000_000),
        }],
    )
    make_snapshot(p2, [])
    make_snapshot(sub, [])
    result = run_sync(str(p1), str(p2), "-" + str(sub))
    assert_run_ok(result, "014.22 absent snapshot rows")
    assert not (sub / "orphan").exists(), "014.22: subordinate directory survived absent snapshot decision"

    p3 = base / "p3"
    p4 = base / "p4"
    sub2 = base / "sub2"
    touch_dir(sub2 / "never")
    make_snapshot(p3, [])
    make_snapshot(p4, [])
    make_snapshot(sub2, [])
    result = run_sync(str(p3), str(p4), "-" + str(sub2))
    assert_run_ok(result, "014.23 no contributing live or snapshot row")
    assert not (sub2 / "never").exists(), "014.23: subordinate no-opinion directory was not displaced"


def case_canon_type_conflicts(base: Path) -> None:
    p1 = base / "canon_file"
    p2 = base / "dir_loser"
    touch_file(p1 / "item", "file wins\n", 1_735_000_500)
    touch_file(p2 / "item" / "nested.txt", "directory loses\n", 1_735_000_400)
    result = run_sync("+" + str(p1), str(p2))
    assert_run_ok(result, "014.25 canon file type conflict")
    assert (p2 / "item").is_file(), "014.25: canon file did not replace directory"
    assert (p2 / "item").read_text(encoding="utf-8") == "file wins\n"

    p3 = base / "canon_dir"
    p4 = base / "file_loser"
    touch_file(p3 / "item" / "inside.txt", "directory wins\n", 1_735_000_600)
    touch_file(p4 / "item", "file loses\n", 1_735_000_700)
    result = run_sync("+" + str(p3), str(p4))
    assert_run_ok(result, "014.26 canon directory type conflict")
    assert (p4 / "item").is_dir(), "014.26: canon directory did not replace file"
    assert (p4 / "item" / "inside.txt").read_text(encoding="utf-8") == "directory wins\n"

    p5 = base / "canon_missing"
    p6 = base / "path_present"
    p5.mkdir(parents=True, exist_ok=True)
    touch_file(p6 / "gone" / "nested.txt", "remove\n", 1_735_000_800)
    result = run_sync("+" + str(p5), str(p6))
    assert_run_ok(result, "014.27 canon missing type conflict")
    assert not (p6 / "gone").exists(), "014.27: missing canon path did not displace active path"


def case_bidirectional_type_conflicts(base: Path) -> None:
    file_peer = base / "file_peer"
    dir_peer = base / "dir_peer"
    sub = base / "subordinate"
    touch_file(file_peer / "same", "contributing file\n", 1_735_001_000)
    touch_file(dir_peer / "same" / "inside.txt", "contributing directory\n", 1_735_001_100)
    touch_file(sub / "same" / "inside.txt", "subordinate directory\n", 1_735_001_200)
    make_snapshot(file_peer, [{"path": "same", "mod_time": ts(1_735_001_000), "byte_size": 18, "last_seen": ts(1_735_001_000)}])
    make_snapshot(dir_peer, [{"path": "same", "mod_time": ts(1_735_001_100), "byte_size": -1, "last_seen": ts(1_735_001_100)}])
    make_snapshot(sub, [])
    result = run_sync(str(file_peer), str(dir_peer), "-" + str(sub))
    assert_run_ok(result, "014.28 contributing file beats directory")
    assert (dir_peer / "same").is_file(), "014.28: contributing file did not beat contributing directory"
    assert (sub / "same").is_file(), "014.31: subordinate losing type was not replaced"
    assert (dir_peer / "same").read_text(encoding="utf-8") == "contributing file\n"

    dir_only = base / "dir_only"
    no_opinion = base / "no_opinion"
    sub_file = base / "sub_file"
    touch_file(dir_only / "path" / "inside.txt", "directory outcome\n", 1_735_001_300)
    touch_file(sub_file / "path", "subordinate file must not win\n", 1_735_001_400)
    make_snapshot(dir_only, [{"path": "path", "mod_time": ts(1_735_001_300), "byte_size": -1, "last_seen": ts(1_735_001_300)}])
    make_snapshot(no_opinion, [])
    make_snapshot(sub_file, [])
    result = run_sync(str(dir_only), str(no_opinion), "-" + str(sub_file))
    assert_run_ok(result, "014.30 subordinate file ignored")
    assert (sub_file / "path").is_dir(), "014.30: subordinate file made file type win over contributing directory"
    assert (sub_file / "path" / "inside.txt").read_text(encoding="utf-8") == "directory outcome\n"


def case_case_preservation(base: Path) -> None:
    p1 = base / "source"
    p2 = base / "dest"
    touch_file(p1 / "CamelCase.TXT", "case\n", 1_735_002_000)
    result = run_sync("+" + str(p1), str(p2))
    assert_run_ok(result, "014.32 filename case")
    names = {entry.name for entry in p2.iterdir() if entry.name != ".kitchensync"}
    assert "CamelCase.TXT" in names, f"014.32: synced names did not preserve exact case: {sorted(names)}"
    assert "camelcase.txt" not in names, "014.32: destination used a different filename case"


def main() -> int:
    failures: list[str] = []
    scenarios = [
        case_canon_directory_creation_and_deletion,
        case_all_live_directory_votes_create_missing,
        case_empty_directory_deletion_ignores_directory_mtime,
        case_directory_deletion_uses_last_seen_when_deleted_time_absent,
        case_directory_survives_and_recurses,
        case_newest_directory_deletion_displaces_whole_subtree,
        case_absent_snapshot_rows_delete_subordinate_directory,
        case_canon_type_conflicts,
        case_bidirectional_type_conflicts,
        case_case_preservation,
    ]

    # not reasonably testable: 014.21 requires forcing repeatable listing
    # failure for one subtree through the released local-file peer surface
    # without relying on host-specific permissions.

    with tempfile.TemporaryDirectory(prefix="kitchensync_014_") as temp_name:
        base = Path(temp_name)
        for scenario in scenarios:
            scenario_base = base / scenario.__name__
            scenario_base.mkdir(parents=True, exist_ok=True)
            try:
                scenario(scenario_base)
            except Exception:
                failures.append(f"{scenario.__name__} failed:\n{traceback.format_exc()}")

    if failures:
        print("\n\n".join(failures))
        return 1
    print("test_014_directory_and_type_decisions: all checks passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
