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
from datetime import datetime, timezone
from pathlib import Path


sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")


LITERAL_WORKSPACE_ROOT = Path("/home/ace/Desktop/prjx/kitchensync")
LITERAL_RELEASED_EXE = LITERAL_WORKSPACE_ROOT / "released" / "kitchensync.exe"
SCRIPT_WORKSPACE_ROOT = Path(__file__).resolve().parents[1]
WORKSPACE_ROOT = LITERAL_WORKSPACE_ROOT if LITERAL_WORKSPACE_ROOT.exists() else SCRIPT_WORKSPACE_ROOT
RELEASED_EXE = LITERAL_RELEASED_EXE if LITERAL_RELEASED_EXE.exists() else WORKSPACE_ROOT / "released" / "kitchensync.exe"

TS_FORMAT = "%Y-%m-%d_%H-%M-%S_%fZ"

# not reasonably testable: 015.8, 015.9, 015.10, 015.11, 015.12
# The destination row state before a queued copy completes is local working
# state and is not uploaded to a peer snapshot until the released process exits.
# not reasonably testable: 015.14, 015.15
# Observing a clean app exit before a queued copy finishes needs timing hooks or
# forced interruption of the product, outside the specified happy-path surface.
# not reasonably testable: 015.17, 015.19
# Failed directory creation and failed displacement require sabotaging peer
# filesystem operations, which the testing philosophy excludes.
# not reasonably testable: 015.25
# Cleanup not delaying sync decisions is a scheduling guarantee without a stable
# end-to-end observation from process exit, stdout, stderr, or final peer files.


def timestamp_text(year: int, month: int, day: int, hour: int = 0) -> str:
    return datetime(year, month, day, hour, tzinfo=timezone.utc).strftime(TS_FORMAT)


def parse_timestamp(value: str) -> datetime:
    return datetime.strptime(value, TS_FORMAT).replace(tzinfo=timezone.utc)


def set_file_time(path: Path, when: datetime) -> None:
    seconds = when.timestamp()
    os.utime(path, (seconds, seconds))


def peer_snapshot(peer: Path) -> Path:
    return peer / ".kitchensync" / "snapshot.db"


def connect_snapshot(peer: Path) -> sqlite3.Connection:
    con = sqlite3.connect(str(peer_snapshot(peer)))
    con.row_factory = sqlite3.Row
    return con


def rows_by_basename(peer: Path, basename: str) -> list[sqlite3.Row]:
    with connect_snapshot(peer) as con:
        return list(con.execute("SELECT * FROM snapshot WHERE basename = ?", (basename,)))


def one_row(peer: Path, basename: str) -> sqlite3.Row | None:
    rows = rows_by_basename(peer, basename)
    if len(rows) != 1:
        return None
    return rows[0]


def insert_snapshot_row(
    peer: Path,
    row_id: str,
    parent_id: str,
    basename: str,
    mod_time: str,
    byte_size: int,
    last_seen: str | None,
    deleted_time: str | None,
) -> None:
    with connect_snapshot(peer) as con:
        con.execute(
            """
            INSERT OR REPLACE INTO snapshot
                (id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            """,
            (row_id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time),
        )
        con.commit()


def run_kitchensync(args: list[str], failures: list[str], label: str, timeout: int = 45) -> subprocess.CompletedProcess[str] | None:
    try:
        result = subprocess.run(
            [str(RELEASED_EXE), *args],
            cwd=str(WORKSPACE_ROOT),
            text=True,
            encoding="utf-8",
            errors="replace",
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=timeout,
            shell=False,
        )
    except Exception as exc:
        failures.append(f"{label}: failed to launch or wait for KitchenSync: {exc}")
        return None

    if result.returncode != 0:
        failures.append(
            f"{label}: expected exit 0, got {result.returncode}; stdout={result.stdout!r}; stderr={result.stderr!r}"
        )
    if result.stderr != "":
        failures.append(f"{label}: expected empty stderr, got {result.stderr!r}")
    return result


def check(condition: bool, failures: list[str], message: str) -> None:
    if not condition:
        failures.append(message)


def assert_row_present(peer: Path, basename: str, failures: list[str], label: str) -> sqlite3.Row | None:
    row = one_row(peer, basename)
    if row is None:
        failures.append(f"{label}: expected exactly one snapshot row for {basename!r} in {peer}")
    return row


def check_present_file_row(
    peer: Path,
    file_path: Path,
    failures: list[str],
    label: str,
) -> sqlite3.Row | None:
    row = assert_row_present(peer, file_path.name, failures, label)
    if row is None:
        return None
    stat = file_path.stat()
    row_mtime = parse_timestamp(row["mod_time"])
    fs_mtime = datetime.fromtimestamp(stat.st_mtime, tz=timezone.utc)
    check(abs((row_mtime - fs_mtime).total_seconds()) <= 5, failures, f"{label}: mod_time does not match file mtime")
    check(row["byte_size"] == stat.st_size, failures, f"{label}: byte_size does not match file size")
    check(row["last_seen"] is not None, failures, f"{label}: last_seen should be set")
    check(row["deleted_time"] is None, failures, f"{label}: deleted_time should be NULL")
    return row


def scenario_listing_copy_and_directory_creation(root: Path, failures: list[str]) -> None:
    peer_a = root / "present_a"
    peer_b = root / "present_b"
    peer_a.mkdir()
    peer_b.mkdir()

    source_file = peer_a / "alpha.txt"
    source_file.write_text("alpha contents\n", encoding="utf-8", newline="\n")
    set_file_time(source_file, datetime(2020, 1, 2, 3, 4, 5, tzinfo=timezone.utc))
    (peer_a / "emptydir").mkdir()

    run_kitchensync([f"+{peer_a}", str(peer_b)], failures, "present/copy/directory")

    check((peer_b / "alpha.txt").read_text(encoding="utf-8") == "alpha contents\n", failures, "015.13: copied file content missing on destination")
    check((peer_b / "emptydir").is_dir(), failures, "015.16: destination directory was not created")

    check_present_file_row(peer_a, source_file, failures, "015.1-015.4 source listing")
    check_present_file_row(peer_b, peer_b / "alpha.txt", failures, "015.13 completed copy")

    dir_row = assert_row_present(peer_b, "emptydir", failures, "015.16 directory creation")
    if dir_row is not None:
        check(dir_row["byte_size"] == -1, failures, "015.16: directory snapshot byte_size should be -1")
        check(dir_row["last_seen"] is not None, failures, "015.16: directory last_seen should be set after creation")
        check(dir_row["deleted_time"] is None, failures, "015.16: directory deleted_time should be NULL")


def scenario_confirmed_absence_and_tombstone_idempotence(root: Path, failures: list[str]) -> None:
    peer_a = root / "absence_a"
    peer_b = root / "absence_b"
    peer_a.mkdir()
    peer_b.mkdir()

    file_a = peer_a / "gone.txt"
    file_a.write_text("delete me\n", encoding="utf-8", newline="\n")
    set_file_time(file_a, datetime(2020, 2, 1, tzinfo=timezone.utc))
    run_kitchensync([f"+{peer_a}", str(peer_b)], failures, "absence initial")

    before = assert_row_present(peer_b, "gone.txt", failures, "absence initial row")
    if before is None:
        return
    previous_last_seen = before["last_seen"]
    check(previous_last_seen is not None, failures, "absence setup: initial last_seen should be set")

    (peer_b / "gone.txt").unlink()
    run_kitchensync([str(peer_a), str(peer_b)], failures, "absence delete decision")

    tombstone = assert_row_present(peer_b, "gone.txt", failures, "015.5-015.6 tombstone")
    if tombstone is not None:
        check(tombstone["deleted_time"] == previous_last_seen, failures, "015.5: deleted_time should equal previous last_seen")
        check(tombstone["last_seen"] == previous_last_seen, failures, "015.6: last_seen should not change when absence is confirmed")

    run_kitchensync([str(peer_a), str(peer_b)], failures, "absence tombstone idempotence")
    after = assert_row_present(peer_b, "gone.txt", failures, "015.7 existing tombstone")
    if after is not None and tombstone is not None:
        check(dict(after) == dict(tombstone), failures, "015.7: existing tombstone row changed on repeated absence")

    peer_a_row = assert_row_present(peer_a, "gone.txt", failures, "015.18 displaced source")
    if peer_a_row is not None:
        check(peer_a_row["deleted_time"] == peer_a_row["last_seen"], failures, "015.18: displaced entry should copy last_seen to deleted_time")
        check(not (peer_a / "gone.txt").exists(), failures, "015.18: displaced source file still exists at original path")


def scenario_directory_displacement_cascade(root: Path, failures: list[str]) -> None:
    peer_a = root / "cascade_a"
    peer_b = root / "cascade_b"
    peer_c = root / "cascade_c"
    for peer in (peer_a, peer_b, peer_c):
        peer.mkdir()

    folder = peer_a / "folder"
    folder.mkdir()
    live = folder / "live.txt"
    old = folder / "old.txt"
    outside = peer_a / "outside.txt"
    live.write_text("live\n", encoding="utf-8", newline="\n")
    old.write_text("old\n", encoding="utf-8", newline="\n")
    outside.write_text("outside\n", encoding="utf-8", newline="\n")
    for path in (live, old, outside):
        set_file_time(path, datetime(2020, 3, 1, tzinfo=timezone.utc))

    run_kitchensync([f"+{peer_a}", str(peer_b), str(peer_c)], failures, "cascade initial")

    old.unlink()
    run_kitchensync([f"+{peer_a}", str(peer_b), str(peer_c)], failures, "cascade create existing tombstone")
    old_b_tombstone = assert_row_present(peer_b, "old.txt", failures, "015.21 setup tombstone")
    old_c_tombstone = assert_row_present(peer_c, "old.txt", failures, "015.23 setup tombstone")
    outside_b_before = assert_row_present(peer_b, "outside.txt", failures, "015.22 setup outside")
    outside_c_before = assert_row_present(peer_c, "outside.txt", failures, "015.23 setup outside")

    shutil.rmtree(folder)
    run_kitchensync([f"+{peer_a}", str(peer_b), str(peer_c)], failures, "cascade directory displacement")

    for peer, old_before, outside_before, peer_label in (
        (peer_b, old_b_tombstone, outside_b_before, "peer B"),
        (peer_c, old_c_tombstone, outside_c_before, "peer C"),
    ):
        folder_row = assert_row_present(peer, "folder", failures, f"015.20 {peer_label} folder")
        live_row = assert_row_present(peer, "live.txt", failures, f"015.20 {peer_label} descendant")
        if folder_row is not None and live_row is not None:
            check(folder_row["deleted_time"] is not None, failures, f"015.20 {peer_label}: folder deleted_time should be set")
            check(live_row["deleted_time"] == folder_row["deleted_time"], failures, f"015.20 {peer_label}: descendant should inherit directory deletion estimate")
        old_after = assert_row_present(peer, "old.txt", failures, f"015.21 {peer_label} existing tombstone")
        if old_before is not None and old_after is not None:
            check(dict(old_after) == dict(old_before), failures, f"015.21 {peer_label}: existing tombstone changed during cascade")
        outside_after = assert_row_present(peer, "outside.txt", failures, f"015.22 {peer_label} outside row")
        if outside_before is not None and outside_after is not None:
            check(outside_after["deleted_time"] == outside_before["deleted_time"], failures, f"015.22 {peer_label}: outside row was tombstoned by cascade")
            check(outside_after["byte_size"] == outside_before["byte_size"], failures, f"015.22 {peer_label}: outside byte_size changed unexpectedly")
        check(not (peer / "folder").exists(), failures, f"015.18/015.23 {peer_label}: displaced folder still exists at original path")

    a_folder = assert_row_present(peer_a, "folder", failures, "015.23 canon peer absence row")
    if a_folder is not None:
        check(a_folder["deleted_time"] is not None, failures, "015.23: canon peer should record its own absence, not receive another peer cascade")


def scenario_cleanup_old_rows(root: Path, failures: list[str]) -> None:
    peer_a = root / "cleanup_a"
    peer_b = root / "cleanup_b"
    peer_a.mkdir()
    peer_b.mkdir()
    keep = peer_a / "keep.txt"
    keep.write_text("keep\n", encoding="utf-8", newline="\n")
    set_file_time(keep, datetime(2020, 4, 1, tzinfo=timezone.utc))

    run_kitchensync([f"+{peer_a}", str(peer_b)], failures, "cleanup initial")

    ancient = timestamp_text(2000, 1, 1)
    insert_snapshot_row(
        peer_b,
        "oldTomb0001",
        "rootSent001",
        "ancient-tombstone.txt",
        ancient,
        10,
        ancient,
        ancient,
    )
    insert_snapshot_row(
        peer_b,
        "oldOrph0001",
        "missing00001",
        "ancient-orphan.txt",
        ancient,
        11,
        ancient,
        None,
    )

    run_kitchensync(["--keep-del-days", "1", f"+{peer_a}", str(peer_b)], failures, "cleanup old rows")

    check(rows_by_basename(peer_b, "ancient-tombstone.txt") == [], failures, "015.24: old tombstone row was not cleaned up")
    check(rows_by_basename(peer_b, "ancient-orphan.txt") == [], failures, "015.26: old orphan non-tombstone row was not cleaned up")
    check((peer_b / "keep.txt").exists(), failures, "cleanup scenario: normal sync decision did not proceed")


def main() -> int:
    failures: list[str] = []
    check(RELEASED_EXE.exists(), failures, f"released executable does not exist: {RELEASED_EXE}")

    with tempfile.TemporaryDirectory(prefix="kitchensync-015-") as temp_name:
        root = Path(temp_name)
        for scenario in (
            scenario_listing_copy_and_directory_creation,
            scenario_confirmed_absence_and_tombstone_idempotence,
            scenario_directory_displacement_cascade,
            scenario_cleanup_old_rows,
        ):
            try:
                scenario(root, failures)
            except Exception as exc:
                failures.append(f"{scenario.__name__}: unexpected exception: {exc}")

    if failures:
        for failure in failures:
            print(f"FAIL: {failure}")
        return 1

    print("test_015_snapshot_row_updates_and_cleanup passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
