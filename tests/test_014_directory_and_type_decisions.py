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
from dataclasses import dataclass
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

WORKSPACE_ROOT = Path("/home/ace/Desktop/prjx/kitchensync")
KITCHENSYNC = WORKSPACE_ROOT / "released" / "kitchensync.exe"

TS_OLD = "2026-01-01_00-00-00_000000Z"
TS_MID = "2026-01-01_00-02-00_000000Z"
TS_NEW = "2026-01-01_00-04-00_000000Z"
TS_NEWER = "2026-01-01_00-06-00_000000Z"

EPOCH_OLD = 1_767_225_600
EPOCH_MID = EPOCH_OLD + 120
EPOCH_NEW = EPOCH_OLD + 240
EPOCH_NEWER = EPOCH_OLD + 360


@dataclass
class Check:
    name: str
    func: object


class FailureCollector:
    def __init__(self) -> None:
        self.failures: list[str] = []

    def check(self, condition: bool, message: str) -> None:
        if not condition:
            self.failures.append(message)

    def equal(self, actual: object, expected: object, message: str) -> None:
        if actual != expected:
            self.failures.append(f"{message}: expected {expected!r}, got {actual!r}")


def run_sync(failures: FailureCollector, *args: str) -> subprocess.CompletedProcess[str]:
    result = subprocess.run(
        [str(KITCHENSYNC), "--verbosity", "error", *args],
        cwd=str(WORKSPACE_ROOT),
        text=True,
        encoding="utf-8",
        errors="replace",
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=30,
        shell=False,
        check=False,
    )
    failures.equal(result.returncode, 0, f"sync exit code for {args}")
    failures.equal(result.stderr, "", f"sync stderr for {args}")
    failures.check(
        "sync complete" in result.stdout.splitlines(),
        f"sync stdout should contain final completion line for {args}; stdout={result.stdout!r}",
    )
    return result


def write_text(path: Path, text: str, mtime: int) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")
    os.utime(path, (mtime, mtime))


def make_dir(path: Path, mtime: int) -> None:
    path.mkdir(parents=True, exist_ok=True)
    os.utime(path, (mtime, mtime))


def read_text(path: Path) -> str | None:
    if not path.is_file():
        return None
    return path.read_text(encoding="utf-8")


def bak_entries(root: Path, basename: str) -> list[Path]:
    base = root / ".kitchensync" / "BAK"
    if not base.exists():
        return []
    return [path for path in base.rglob(basename) if path.name == basename]


def peer(tmp: Path, name: str) -> Path:
    path = tmp / name
    path.mkdir(parents=True, exist_ok=True)
    return path


def timestamp_for_epoch(epoch: int) -> str:
    if epoch == EPOCH_OLD:
        return TS_OLD
    if epoch == EPOCH_MID:
        return TS_MID
    if epoch == EPOCH_NEW:
        return TS_NEW
    if epoch == EPOCH_NEWER:
        return TS_NEWER
    raise ValueError(f"no fixture timestamp for epoch {epoch}")


XXH_PRIME64_1 = 11400714785074694791
XXH_PRIME64_2 = 14029467366897019727
XXH_PRIME64_3 = 1609587929392839161
XXH_PRIME64_4 = 9650029242287828579
XXH_PRIME64_5 = 2870177450012600261
MASK64 = (1 << 64) - 1
BASE62 = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"


def rotl64(value: int, count: int) -> int:
    return ((value << count) | (value >> (64 - count))) & MASK64


def read_u64(data: bytes, offset: int) -> int:
    return int.from_bytes(data[offset : offset + 8], "little")


def read_u32(data: bytes, offset: int) -> int:
    return int.from_bytes(data[offset : offset + 4], "little")


def xxh64_round(acc: int, lane: int) -> int:
    acc = (acc + lane * XXH_PRIME64_2) & MASK64
    acc = rotl64(acc, 31)
    return (acc * XXH_PRIME64_1) & MASK64


def xxh64_merge(acc: int, lane: int) -> int:
    acc ^= xxh64_round(0, lane)
    acc = (acc * XXH_PRIME64_1 + XXH_PRIME64_4) & MASK64
    return acc


def xxh64(data: bytes) -> int:
    index = 0
    length = len(data)
    if length >= 32:
        v1 = (XXH_PRIME64_1 + XXH_PRIME64_2) & MASK64
        v2 = XXH_PRIME64_2
        v3 = 0
        v4 = (-XXH_PRIME64_1) & MASK64
        limit = length - 32
        while index <= limit:
            v1 = xxh64_round(v1, read_u64(data, index))
            index += 8
            v2 = xxh64_round(v2, read_u64(data, index))
            index += 8
            v3 = xxh64_round(v3, read_u64(data, index))
            index += 8
            v4 = xxh64_round(v4, read_u64(data, index))
            index += 8
        acc = (
            rotl64(v1, 1)
            + rotl64(v2, 7)
            + rotl64(v3, 12)
            + rotl64(v4, 18)
        ) & MASK64
        acc = xxh64_merge(acc, v1)
        acc = xxh64_merge(acc, v2)
        acc = xxh64_merge(acc, v3)
        acc = xxh64_merge(acc, v4)
    else:
        acc = XXH_PRIME64_5

    acc = (acc + length) & MASK64
    while index + 8 <= length:
        lane = xxh64_round(0, read_u64(data, index))
        acc ^= lane
        acc = (rotl64(acc, 27) * XXH_PRIME64_1 + XXH_PRIME64_4) & MASK64
        index += 8
    if index + 4 <= length:
        acc ^= (read_u32(data, index) * XXH_PRIME64_1) & MASK64
        acc = (rotl64(acc, 23) * XXH_PRIME64_2 + XXH_PRIME64_3) & MASK64
        index += 4
    while index < length:
        acc ^= (data[index] * XXH_PRIME64_5) & MASK64
        acc = (rotl64(acc, 11) * XXH_PRIME64_1) & MASK64
        index += 1

    acc ^= acc >> 33
    acc = (acc * XXH_PRIME64_2) & MASK64
    acc ^= acc >> 29
    acc = (acc * XXH_PRIME64_3) & MASK64
    acc ^= acc >> 32
    return acc & MASK64


def base62_11(value: int) -> str:
    chars: list[str] = []
    for _ in range(11):
        value, rem = divmod(value, 62)
        chars.append(BASE62[rem])
    return "".join(reversed(chars))


def path_id(relpath: str) -> str:
    return base62_11(xxh64(relpath.encode("utf-8")))


def parent_id(relpath: str) -> str:
    parent = str(Path(relpath).parent).replace("\\", "/")
    if parent in ("", "."):
        return path_id("/")
    return path_id(parent)


def create_snapshot(root: Path, rows: list[tuple[str, int, str, str | None, str | None]]) -> None:
    db_path = root / ".kitchensync" / "snapshot.db"
    db_path.parent.mkdir(parents=True, exist_ok=True)
    if db_path.exists():
        db_path.unlink()
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
        conn.execute("CREATE INDEX idx_snapshot_parent_id ON snapshot(parent_id)")
        conn.execute("CREATE INDEX idx_snapshot_last_seen ON snapshot(last_seen)")
        conn.execute("CREATE INDEX idx_snapshot_deleted_time ON snapshot(deleted_time)")
        for relpath, byte_size, mod_time, last_seen, deleted_time in rows:
            normalized = relpath.replace("\\", "/")
            basename = normalized.rsplit("/", 1)[-1]
            conn.execute(
                """
                INSERT INTO snapshot
                (id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time)
                VALUES (?, ?, ?, ?, ?, ?, ?)
                """,
                (
                    path_id(normalized),
                    parent_id(normalized),
                    basename,
                    mod_time,
                    byte_size,
                    last_seen,
                    deleted_time,
                ),
            )


def seed_snapshot_from_live(root: Path, deleted: dict[str, str | None] | None = None) -> None:
    rows: list[tuple[str, int, str, str | None, str | None]] = []
    deleted = deleted or {}
    for path in sorted(root.rglob("*")):
        if ".kitchensync" in path.parts:
            continue
        relpath = path.relative_to(root).as_posix()
        if path.is_dir():
            rows.append((relpath, -1, timestamp_for_epoch(EPOCH_OLD), TS_OLD, deleted.get(relpath)))
        elif path.is_file():
            size = path.stat().st_size
            rows.append((relpath, size, timestamp_for_epoch(int(path.stat().st_mtime)), TS_OLD, deleted.get(relpath)))
    for relpath, deleted_time in deleted.items():
        if not (root / relpath).exists() and not any(row[0] == relpath for row in rows):
            rows.append((relpath, -1, TS_OLD, TS_OLD, deleted_time))
    create_snapshot(root, rows)


def assert_no_entry(failures: FailureCollector, root: Path, relpath: str, message: str) -> None:
    failures.check(not (root / relpath).exists(), message)


def check_canon_directory_and_type_rules(failures: FailureCollector) -> None:
    with tempfile.TemporaryDirectory(prefix="ks014_canon_") as raw:
        tmp = Path(raw)
        canon = peer(tmp, "canon")
        other = peer(tmp, "other")
        subordinate = peer(tmp, "subordinate")

        make_dir(canon / "SharedDir", EPOCH_NEW)
        write_text(canon / "SharedDir" / "CanonOnly.txt", "from canon\n", EPOCH_NEW)
        write_text(canon / "FileWins", "canon file\n", EPOCH_NEW)
        make_dir(other / "FileWins", EPOCH_OLD)
        write_text(other / "FileWins" / "old.txt", "old directory\n", EPOCH_OLD)
        make_dir(canon / "DirWins", EPOCH_NEW)
        write_text(canon / "DirWins" / "inside.txt", "inside\n", EPOCH_NEW)
        write_text(other / "DirWins", "old file\n", EPOCH_OLD)
        write_text(other / "CanonMissing", "remove me\n", EPOCH_OLD)
        write_text(subordinate / "CanonMissing", "remove me too\n", EPOCH_OLD)
        write_text(canon / "MiXeDCase.TXT", "case kept\n", EPOCH_NEWER)

        run_sync(failures, f"+{canon}", str(other), f"-{subordinate}")

        for target in (other, subordinate):
            failures.check((target / "SharedDir").is_dir(), "014.2 canon live directory should exist on every active peer")
            failures.equal(read_text(target / "SharedDir" / "CanonOnly.txt"), "from canon\n", "014.18 canon-created directory should be recursed into")
            assert_no_entry(failures, target, "CanonMissing", "014.3 and 014.28 canon missing path should be absent on active peers")
            failures.equal(read_text(target / "FileWins"), "canon file\n", "014.25 and 014.26 canon file should replace peer directory")
            failures.check((target / "DirWins").is_dir(), "014.27 canon directory should replace peer file")
            failures.equal(read_text(target / "DirWins" / "inside.txt"), "inside\n", "014.27 canon directory contents should sync")
            failures.check((target / "MiXeDCase.TXT").is_file(), "014.36 synced filename should preserve source case")
            failures.check(not (target / "mixedcase.txt").exists(), "014.36 sync should not invent a different filename case")

        failures.check(bak_entries(other, "FileWins"), "014.25 canon file should displace losing directory to BAK")
        failures.check(bak_entries(other, "DirWins"), "014.27 canon directory should displace losing file to BAK")
        failures.check(bak_entries(other, "CanonMissing"), "014.28 missing canon path should displace active peer path to BAK")


def check_all_live_directory_votes_and_subordinate_cleanup(failures: FailureCollector) -> None:
    with tempfile.TemporaryDirectory(prefix="ks014_live_votes_") as raw:
        tmp = Path(raw)
        a = peer(tmp, "a")
        b = peer(tmp, "b")
        sub = peer(tmp, "sub")

        make_dir(a / "Everywhere", EPOCH_OLD)
        write_text(a / "Everywhere" / "a.txt", "a\n", EPOCH_OLD)
        make_dir(b / "Everywhere", EPOCH_NEWER)
        os.utime(b / "Everywhere", (EPOCH_NEWER, EPOCH_NEWER))
        make_dir(sub / "OnlySub", EPOCH_OLD)
        write_text(sub / "OnlySub" / "sub.txt", "sub\n", EPOCH_OLD)
        seed_snapshot_from_live(a)
        seed_snapshot_from_live(b)
        seed_snapshot_from_live(sub)

        run_sync(failures, str(a), str(b), f"-{sub}")

        failures.check((a / "Everywhere").is_dir(), "014.1 directory mtime differences should not delete a live directory")
        failures.check((b / "Everywhere").is_dir(), "014.4 directory live on all contributing voters should survive")
        failures.check((sub / "Everywhere").is_dir(), "014.4 surviving directory should be created on subordinate peer")
        failures.equal(read_text(b / "Everywhere" / "a.txt"), "a\n", "014.18 surviving directory should recurse into child files")
        assert_no_entry(failures, sub, "OnlySub", "014.23 subordinate-only directory should be displaced")
        failures.check(bak_entries(sub, "OnlySub"), "014.23 subordinate-only directory should move to BAK")


def check_live_directory_votes_despite_snapshot_and_no_row_abstention(failures: FailureCollector) -> None:
    with tempfile.TemporaryDirectory(prefix="ks014_snapshot_votes_") as raw:
        tmp = Path(raw)
        live = peer(tmp, "live")
        no_row = peer(tmp, "no_row")
        target = peer(tmp, "target")

        make_dir(live / "RowDiffers", EPOCH_NEW)
        write_text(live / "RowDiffers" / "fresh.txt", "fresh\n", EPOCH_NEW)
        create_snapshot(live, [("RowDiffers", -1, TS_OLD, TS_OLD, TS_OLD)])
        create_snapshot(no_row, [("unrelated.txt", 1, TS_OLD, TS_OLD, None)])
        create_snapshot(target, [("unrelated.txt", 1, TS_OLD, TS_OLD, None)])

        run_sync(failures, str(live), str(no_row), str(target))

        failures.check((no_row / "RowDiffers").is_dir(), "014.5 live directory should vote for existence even when snapshot row differs")
        failures.check((target / "RowDiffers").is_dir(), "014.6 no-row absent peer should not vote against live directory")
        failures.equal(read_text(target / "RowDiffers" / "fresh.txt"), "fresh\n", "014.18 live directory survival should recurse")


def check_directory_deletion_wins_with_deleted_time_and_whole_displacement(failures: FailureCollector) -> None:
    with tempfile.TemporaryDirectory(prefix="ks014_delete_deleted_time_") as raw:
        tmp = Path(raw)
        live = peer(tmp, "live")
        absent_deleted = peer(tmp, "absent_deleted")
        absent_old = peer(tmp, "absent_old")
        target = peer(tmp, "target")

        make_dir(live / "DeletedWins", EPOCH_OLD)
        write_text(live / "DeletedWins" / "old.txt", "old\n", EPOCH_OLD)
        make_dir(target / "DeletedWins", EPOCH_OLD)
        write_text(target / "DeletedWins" / "target.txt", "target\n", EPOCH_OLD)
        create_snapshot(live, [("DeletedWins", -1, TS_OLD, TS_OLD, None), ("DeletedWins/old.txt", 4, TS_OLD, TS_OLD, None)])
        create_snapshot(target, [("DeletedWins", -1, TS_OLD, TS_OLD, None), ("DeletedWins/target.txt", 7, TS_OLD, TS_OLD, None)])
        create_snapshot(absent_deleted, [("DeletedWins", -1, TS_OLD, TS_OLD, TS_NEWER)])
        create_snapshot(absent_old, [("DeletedWins", -1, TS_OLD, TS_OLD, TS_MID)])

        run_sync(failures, str(live), str(absent_deleted), str(absent_old), str(target))

        for root in (live, target):
            assert_no_entry(failures, root, "DeletedWins", "014.7, 014.12, and 014.13 newest deleted_time should delete live directory")
            failures.check(bak_entries(root, "DeletedWins"), "014.24 directory displacement should move the whole directory to BAK")
        assert_no_entry(failures, absent_deleted, "DeletedWins", "014.15 deletion winner should not recreate missing peer directory")
        assert_no_entry(failures, absent_old, "DeletedWins", "014.15 deletion winner should not recreate any absent peer directory")


def check_directory_deletion_wins_with_last_seen_and_no_files(failures: FailureCollector) -> None:
    with tempfile.TemporaryDirectory(prefix="ks014_delete_last_seen_") as raw:
        tmp = Path(raw)
        live_empty = peer(tmp, "live_empty")
        absent = peer(tmp, "absent")
        target = peer(tmp, "target")

        make_dir(live_empty / "EmptyTree" / "child", EPOCH_NEWER)
        make_dir(target / "EmptyTree", EPOCH_NEWER)
        create_snapshot(live_empty, [("EmptyTree", -1, TS_OLD, TS_OLD, None), ("EmptyTree/child", -1, TS_OLD, TS_OLD, None)])
        create_snapshot(target, [("EmptyTree", -1, TS_OLD, TS_OLD, None)])
        create_snapshot(absent, [("EmptyTree", -1, TS_OLD, TS_NEW, None)])

        run_sync(failures, str(live_empty), str(absent), str(target))

        assert_no_entry(failures, live_empty, "EmptyTree", "014.8 and 014.14 last_seen deletion should win when live subtree has no files")
        assert_no_entry(failures, target, "EmptyTree", "014.11 directories under live tree should not provide survival evidence")
        failures.check(bak_entries(live_empty, "EmptyTree"), "014.10 and 014.11 empty live directory tree should be displaced")


def check_directory_survives_and_recurses_by_child_file_rules(failures: FailureCollector) -> None:
    with tempfile.TemporaryDirectory(prefix="ks014_survival_") as raw:
        tmp = Path(raw)
        live = peer(tmp, "live")
        absent = peer(tmp, "absent")
        target = peer(tmp, "target")

        make_dir(live / "Survives", EPOCH_OLD)
        write_text(live / "Survives" / "new.txt", "new\n", EPOCH_NEWER)
        write_text(live / "Survives" / "old.txt", "old\n", EPOCH_OLD)
        create_snapshot(live, [
            ("Survives", -1, TS_OLD, TS_OLD, None),
            ("Survives/new.txt", 4, TS_NEWER, TS_OLD, None),
            ("Survives/old.txt", 4, TS_OLD, TS_OLD, None),
        ])
        create_snapshot(absent, [
            ("Survives", -1, TS_OLD, TS_NEW, None),
            ("Survives/new.txt", 4, TS_OLD, TS_NEW, None),
            ("Survives/old.txt", 4, TS_OLD, TS_NEW, None),
        ])
        create_snapshot(target, [("seed.txt", 5, TS_OLD, TS_OLD, None)])

        run_sync(failures, str(live), str(absent), str(target))

        for root in (live, absent, target):
            failures.check((root / "Survives").is_dir(), "014.17 directory should survive when file evidence is within tolerance/newer")
            failures.equal(read_text(root / "Survives" / "new.txt"), "new\n", "014.19 newer child content should propagate when directory survives")
            assert_no_entry(failures, root, "Survives/old.txt", "014.20 older child content should be removed entry by entry")
        failures.check(bak_entries(live, "old.txt"), "014.20 older child file should be displaced, not whole directory")


def check_all_snapshot_absent_directory_displacement(failures: FailureCollector) -> None:
    with tempfile.TemporaryDirectory(prefix="ks014_all_absent_") as raw:
        tmp = Path(raw)
        a = peer(tmp, "a")
        b = peer(tmp, "b")
        sub = peer(tmp, "sub")

        make_dir(sub / "GoneEverywhere", EPOCH_OLD)
        write_text(sub / "GoneEverywhere" / "sub.txt", "sub\n", EPOCH_OLD)
        create_snapshot(a, [("GoneEverywhere", -1, TS_OLD, TS_OLD, TS_NEW)])
        create_snapshot(b, [("GoneEverywhere", -1, TS_OLD, TS_MID, None)])
        create_snapshot(sub, [("GoneEverywhere", -1, TS_OLD, TS_OLD, None), ("GoneEverywhere/sub.txt", 4, TS_OLD, TS_OLD, None)])

        run_sync(failures, str(a), str(b), f"-{sub}")

        assert_no_entry(failures, sub, "GoneEverywhere", "014.22 absent snapshot-only directory should displace active peer copy")
        failures.check(bak_entries(sub, "GoneEverywhere"), "014.22 displaced snapshot-only directory should be recoverable in BAK")


def check_type_conflict_without_canon_and_subordinate_rules(failures: FailureCollector) -> None:
    with tempfile.TemporaryDirectory(prefix="ks014_type_conflict_") as raw:
        tmp = Path(raw)
        file_old = peer(tmp, "file_old")
        file_new = peer(tmp, "file_new")
        directory = peer(tmp, "directory")
        sub_file = peer(tmp, "sub_file")
        sub_dir = peer(tmp, "sub_dir")

        write_text(file_old / "Conflict", "old file\n", EPOCH_OLD)
        write_text(file_new / "Conflict", "new file\n", EPOCH_NEWER)
        make_dir(directory / "Conflict", EPOCH_MID)
        write_text(directory / "Conflict" / "inside.txt", "directory loses\n", EPOCH_MID)
        write_text(sub_file / "OnlySubFile", "subordinate should not make file win\n", EPOCH_NEWER)
        make_dir(directory / "OnlySubFile", EPOCH_OLD)
        write_text(directory / "OnlySubFile" / "dir.txt", "contributing directory\n", EPOCH_OLD)
        make_dir(sub_dir / "Conflict", EPOCH_OLD)
        write_text(sub_dir / "Conflict" / "sub.txt", "sub loses\n", EPOCH_OLD)

        seed_snapshot_from_live(file_old)
        seed_snapshot_from_live(file_new)
        seed_snapshot_from_live(directory)
        seed_snapshot_from_live(sub_file)
        seed_snapshot_from_live(sub_dir)

        run_sync(failures, str(file_old), str(file_new), str(directory), f"-{sub_file}", f"-{sub_dir}")

        for root in (file_old, file_new, directory, sub_file, sub_dir):
            failures.equal(read_text(root / "Conflict"), "new file\n", "014.29, 014.31, and 014.32 file winner should sync to all active peers")
        failures.check(bak_entries(directory, "Conflict"), "014.30 losing contributing directory should be displaced to BAK")
        failures.check(bak_entries(sub_dir, "Conflict"), "014.34 subordinate losing directory type should be displaced to BAK")
        failures.check((directory / "OnlySubFile").is_dir(), "014.33 subordinate file should not beat contributing directory")
        failures.check((sub_file / "OnlySubFile").is_dir(), "014.35 subordinate wrong type should be replaced with winning directory")
        failures.check(bak_entries(sub_file, "OnlySubFile"), "014.34 subordinate losing file type should be displaced to BAK")


def check_canon_directory_makes_subordinate_file_directory(failures: FailureCollector) -> None:
    with tempfile.TemporaryDirectory(prefix="ks014_canon_subordinate_") as raw:
        tmp = Path(raw)
        canon = peer(tmp, "canon")
        normal = peer(tmp, "normal")
        sub = peer(tmp, "sub")

        make_dir(canon / "CanonDir", EPOCH_NEW)
        write_text(canon / "CanonDir" / "content.txt", "content\n", EPOCH_NEW)
        write_text(sub / "CanonDir", "wrong type\n", EPOCH_OLD)

        run_sync(failures, f"+{canon}", str(normal), f"-{sub}")

        failures.check((sub / "CanonDir").is_dir(), "014.35 subordinate path should be replaced with canon winning directory")
        failures.equal(read_text(sub / "CanonDir" / "content.txt"), "content\n", "014.27 canon directory should sync after replacing subordinate file")
        failures.check(bak_entries(sub, "CanonDir"), "014.34 subordinate losing file should be displaced to BAK")


def main() -> int:
    failures = FailureCollector()
    failures.check(KITCHENSYNC.is_file(), f"released executable should exist at {KITCHENSYNC}")

    checks = [
        Check("canon directory/type/case rules", check_canon_directory_and_type_rules),
        Check("all-live directory votes and subordinate cleanup", check_all_live_directory_votes_and_subordinate_cleanup),
        Check("live vote despite snapshot and no-row abstention", check_live_directory_votes_despite_snapshot_and_no_row_abstention),
        Check("directory deletion by deleted_time", check_directory_deletion_wins_with_deleted_time_and_whole_displacement),
        Check("directory deletion by last_seen with no file evidence", check_directory_deletion_wins_with_last_seen_and_no_files),
        Check("directory survival and child file rules", check_directory_survives_and_recurses_by_child_file_rules),
        Check("snapshot-only absent directory displacement", check_all_snapshot_absent_directory_displacement),
        Check("type conflict without canon", check_type_conflict_without_canon_and_subordinate_rules),
        Check("canon directory replaces subordinate file", check_canon_directory_makes_subordinate_file_directory),
    ]

    for check in checks:
        try:
            check.func(failures)
        except Exception as exc:
            failures.failures.append(f"{check.name}: unexpected exception: {exc!r}")

    # not reasonably testable: 014.21 requires forcing repeated listing failure
    # for a live subtree after startup without sabotaging the local filesystem or
    # probing an SFTP helper protocol in this authoring job.

    if failures.failures:
        print("FAIL")
        for index, failure in enumerate(failures.failures, start=1):
            print(f"{index}. {failure}")
        return 1
    print("PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
