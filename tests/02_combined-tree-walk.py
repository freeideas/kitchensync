#!/usr/bin/env -S uv run --script
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
import time
from datetime import UTC, datetime
from pathlib import Path
from typing import Any


PROJECT_DIR = Path(__file__).resolve().parents[1]
JAVA = PROJECT_DIR / "tools/compiler/jdk/bin/java"
JAR = PROJECT_DIR / "released/kitchensync.jar"
TEST_ROOT = PROJECT_DIR / ".aitc-test-02-combined-tree-walk"


def run_cli(*peers: Path | str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(JAVA), "-jar", str(JAR), *(str(peer) for peer in peers)],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=60,
        check=False,
    )


def reset_tree(path: Path) -> None:
    if not path.exists():
        return
    for root, dirs, files in os.walk(path, topdown=False):
        for name in files:
            file_path = Path(root) / name
            try:
                file_path.chmod(0o600)
            except OSError:
                pass
        for name in dirs:
            dir_path = Path(root) / name
            try:
                dir_path.chmod(0o700)
            except OSError:
                pass
    shutil.rmtree(path)


def write_file(path: Path, text: str, mtime: int) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="")
    os.utime(path, (mtime, mtime))


def snapshot_time(epoch_seconds: int) -> str:
    return datetime.fromtimestamp(epoch_seconds, UTC).strftime(
        "%Y-%m-%d_%H-%M-%S_%fZ"
    )


def user_paths(peer: Path) -> set[str]:
    paths: set[str] = set()
    for path in peer.rglob("*"):
        rel = path.relative_to(peer).as_posix()
        if rel == ".kitchensync" or rel.startswith(".kitchensync/"):
            continue
        paths.add(rel)
    return paths


def read_snapshot_rows(peer: Path) -> tuple[list[dict[str, Any]], str | None]:
    db = peer / ".kitchensync" / "snapshot.db"
    if not db.exists():
        return [], f"{db} does not exist"

    rows: list[dict[str, Any]] = []
    try:
        conn = sqlite3.connect(f"file:{db.as_posix()}?mode=ro", uri=True)
        conn.row_factory = sqlite3.Row
        with conn:
            table_names = [
                row[0]
                for row in conn.execute(
                    "SELECT name FROM sqlite_master WHERE type = 'table'"
                )
                if not row[0].startswith("sqlite_")
            ]
            for table in table_names:
                quoted = '"' + table.replace('"', '""') + '"'
                for row in conn.execute(f"SELECT * FROM {quoted}"):
                    item = dict(row)
                    item["__table__"] = table
                    rows.append(item)
    except sqlite3.Error as exc:
        return rows, f"could not read {db}: {exc}"
    finally:
        try:
            conn.close()
        except Exception:
            pass
    return rows, None


def row_matches(row: dict[str, Any], basename: str) -> bool:
    for key, value in row.items():
        if key == "__table__" or value is None:
            continue
        text = str(value).replace("\\", "/").strip("/")
        if text == basename or text.endswith("/" + basename):
            return True
    return False


def snapshot_row(
    failures: list[str], peer: Path, basename: str
) -> dict[str, Any] | None:
    rows, error = read_snapshot_rows(peer)
    if error is not None:
        failures.append(error)
        return None
    matches = [row for row in rows if row_matches(row, basename)]
    if not matches:
        sample = rows[:3]
        failures.append(
            f"{peer.name} snapshot has no row for {basename!r}; sample rows: {sample}"
        )
        return None
    if len(matches) > 1:
        failures.append(
            f"{peer.name} snapshot has multiple rows matching {basename!r}: {matches}"
        )
    return matches[0]


def field(row: dict[str, Any], *names: str) -> Any:
    wanted = {name.lower() for name in names}
    for key, value in row.items():
        if key.lower() in wanted:
            return value
    return None


def field_containing(row: dict[str, Any], *parts: str) -> Any:
    lowered = [part.lower() for part in parts]
    for key, value in row.items():
        name = key.lower()
        if all(part in name for part in lowered):
            return value
    return None


def last_seen(row: dict[str, Any]) -> Any:
    return field(row, "last_seen", "lastSeen") or field_containing(row, "last", "seen")


def deleted_time(row: dict[str, Any]) -> Any:
    return (
        field(row, "deleted_time", "deletedTime")
        if field(row, "deleted_time", "deletedTime") is not None
        else field_containing(row, "deleted")
    )


def byte_size(row: dict[str, Any]) -> Any:
    value = field(row, "byte_size", "byteSize")
    return value if value is not None else field_containing(row, "size")


def mod_time(row: dict[str, Any]) -> Any:
    value = field(row, "mod_time", "modTime", "mtime")
    return value if value is not None else field_containing(row, "mod")


def check(condition: bool, failures: list[str], message: str) -> None:
    if not condition:
        failures.append(message)


def check_run_ok(
    failures: list[str], result: subprocess.CompletedProcess[str], label: str
) -> None:
    if result.returncode != 0:
        failures.append(
            f"{label} exited {result.returncode}\nstdout:\n{result.stdout}\nstderr:\n{result.stderr}"
        )


def check_file_text(
    failures: list[str], path: Path, expected: str, label: str
) -> None:
    if not path.exists():
        failures.append(f"{label}: {path} does not exist")
        return
    actual = path.read_text(encoding="utf-8")
    if actual != expected:
        failures.append(f"{label}: {path} contains {actual!r}, expected {expected!r}")


def check_snapshot_present(
    failures: list[str], peer: Path, basename: str, label: str
) -> dict[str, Any] | None:
    row = snapshot_row(failures, peer, basename)
    if row is None:
        return None
    check(
        last_seen(row) is not None,
        failures,
        f"{label}: {peer.name} row for {basename} has no last_seen: {row}",
    )
    check(
        deleted_time(row) is None,
        failures,
        f"{label}: {peer.name} row for {basename} should not be deleted: {row}",
    )
    return row


def check_deleted_from_existing_last_seen(
    failures: list[str],
    peer: Path,
    basename: str,
    previous_last_seen: Any,
    label: str,
) -> tuple[Any, Any]:
    row = snapshot_row(failures, peer, basename)
    if row is None:
        return None, None
    current_last_seen = last_seen(row)
    current_deleted_time = deleted_time(row)
    check(
        current_last_seen == previous_last_seen,
        failures,
        f"{label}: confirming absence must not change {peer.name} last_seen for {basename}; before={previous_last_seen!r}, after={current_last_seen!r}, row={row}",
    )
    check(
        current_deleted_time == previous_last_seen,
        failures,
        f"{label}: {peer.name} deleted_time for {basename} should be the previous last_seen {previous_last_seen!r}, got {current_deleted_time!r}; row={row}",
    )
    return current_last_seen, current_deleted_time


def main() -> int:
    failures: list[str] = []
    reset_tree(TEST_ROOT)
    a = TEST_ROOT / "peer-a-canon"
    b = TEST_ROOT / "peer-b"
    c = TEST_ROOT / "peer-c"
    for peer in (a, b, c):
        peer.mkdir(parents=True, exist_ok=True)

    # The public CLI exposes completed sync state, not in-walk scheduling.
    # Exact shared-directory visit count (02.26), exact parallel listing,
    # pre-order timing (02.29), subtree recursion peer membership (02.30),
    # pre-operation snapshot writes (02.31), and the enqueue-before-copy half of
    # 02.47 are not reasonably testable here without instrumentation or induced
    # mid-run failure.

    t0 = int(time.time()) - 200
    write_file(a / "RootOnly_02.txt", "from canon root", t0)
    write_file(a / "SharedDir_02" / "CanonChild_02.txt", "canon child", t0 + 1)
    write_file(a / "KeepDir_02" / "KeepChild_02.txt", "keep child", t0 + 2)
    write_file(a / "MixedCaseName_02.TxT", "case must survive", t0 + 3)
    write_file(a / "DeleteVote_02.txt", "delete vote target", t0 + 4)
    write_file(a / "EverywhereGone_02.txt", "later absent everywhere", t0 + 5)

    check_run_ok(failures, run_cli("+" + str(a), b, c), "initial canon seed sync")

    root_a_previous = last_seen(snapshot_row(failures, a, "RootOnly_02.txt") or {})
    time.sleep(1.1)

    write_file(b / "PeerBOnly_02.txt", "from peer b", t0 + 6)
    write_file(b / "SharedDir_02" / "PeerBChild_02.txt", "peer b child", t0 + 7)

    check_run_ok(failures, run_cli(a, b, c), "combined-tree sync with contributing peers")

    expected_after_initial = {
        "RootOnly_02.txt",
        "PeerBOnly_02.txt",
        "MixedCaseName_02.TxT",
        "DeleteVote_02.txt",
        "EverywhereGone_02.txt",
        "SharedDir_02",
        "SharedDir_02/CanonChild_02.txt",
        "SharedDir_02/PeerBChild_02.txt",
        "KeepDir_02",
        "KeepDir_02/KeepChild_02.txt",
    }
    for peer in (a, b, c):
        check(
            expected_after_initial <= user_paths(peer),
            failures,
            f"{peer.name} did not receive the unioned combined tree; missing {sorted(expected_after_initial - user_paths(peer))}",
        )

    check_file_text(
        failures,
        b / "MixedCaseName_02.TxT",
        "case must survive",
        "mixed-case file copy",
    )
    check(
        not (b / "mixedcasename_02.txt").exists(),
        failures,
        "destination peer contains a case-normalized variant of MixedCaseName_02.TxT",
    )
    check_file_text(
        failures,
        c / "SharedDir_02" / "CanonChild_02.txt",
        "canon child",
        "directory created inline before subtree copy",
    )
    check_file_text(
        failures,
        c / "SharedDir_02" / "PeerBChild_02.txt",
        "peer b child",
        "combined shared directory visited once with all peers' children",
    )
    check(
        not (c / "shareddir_02").exists(),
        failures,
        "destination peer contains a case-normalized variant of SharedDir_02",
    )

    root_copy_row = check_snapshot_present(
        failures, b, "RootOnly_02.txt", "copied root file snapshot"
    )
    mixed_copy_row = check_snapshot_present(
        failures, b, "MixedCaseName_02.TxT", "mixed-case copied file snapshot"
    )
    shared_dir_row = check_snapshot_present(
        failures, c, "SharedDir_02", "inline-created directory snapshot"
    )
    check_snapshot_present(
        failures, a, "SharedDir_02", "present source directory snapshot"
    )
    check_snapshot_present(
        failures, b, "PeerBChild_02.txt", "present peer-b child snapshot"
    )
    root_a_current = last_seen(snapshot_row(failures, a, "RootOnly_02.txt") or {})
    check(
        root_a_previous is not None and root_a_current != root_a_previous,
        failures,
        f"confirmed-present source row should refresh last_seen during the later walk; before={root_a_previous!r}, after={root_a_current!r}",
    )
    if root_copy_row is not None:
        expected_mod_time = snapshot_time(t0)
        check(
            byte_size(root_copy_row) == len("from canon root"),
            failures,
            f"copied file snapshot should record byte_size={len('from canon root')}: {root_copy_row}",
        )
        check(
            mod_time(root_copy_row) == expected_mod_time,
            failures,
            f"copied file snapshot should record the winning file mod_time={expected_mod_time}: {root_copy_row}",
        )
    if mixed_copy_row is not None and shared_dir_row is not None:
        check(
            last_seen(mixed_copy_row) is not None and last_seen(shared_dir_row) is not None,
            failures,
            "copied file and created directory rows should both get last_seen during the sync",
        )

    delete_b_previous = last_seen(snapshot_row(failures, b, "DeleteVote_02.txt") or {})
    delete_c_previous = last_seen(snapshot_row(failures, c, "DeleteVote_02.txt") or {})
    gone_b_previous = last_seen(snapshot_row(failures, b, "EverywhereGone_02.txt") or {})
    gone_c_previous = last_seen(snapshot_row(failures, c, "EverywhereGone_02.txt") or {})

    for target in (a / "DeleteVote_02.txt", c / "DeleteVote_02.txt"):
        if target.exists():
            target.unlink()
    for peer in (a, b, c):
        target = peer / "EverywhereGone_02.txt"
        if target.exists():
            target.unlink()

    check_run_ok(
        failures,
        run_cli("+" + str(a), b, c),
        "canon deletion and confirmed absence sync",
    )
    check(
        not (b / "DeleteVote_02.txt").exists(),
        failures,
        "canon-lost live entry DeleteVote_02.txt still exists on peer b after deletion decision",
    )
    check(
        not (c / "DeleteVote_02.txt").exists(),
        failures,
        "confirmed-absent DeleteVote_02.txt was recreated on peer c",
    )
    delete_b_last, delete_b_deleted = check_deleted_from_existing_last_seen(
        failures,
        b,
        "DeleteVote_02.txt",
        delete_b_previous,
        "live peer losing a canon deletion decision",
    )
    delete_c_last, delete_c_deleted = check_deleted_from_existing_last_seen(
        failures,
        c,
        "DeleteVote_02.txt",
        delete_c_previous,
        "absent peer with existing non-deleted row",
    )
    gone_b_row = snapshot_row(failures, b, "EverywhereGone_02.txt")
    gone_c_row = snapshot_row(failures, c, "EverywhereGone_02.txt")
    gone_b_last = gone_b_deleted = None
    gone_c_last = gone_c_deleted = None
    if gone_b_row is not None:
        gone_b_last = last_seen(gone_b_row)
        gone_b_deleted = deleted_time(gone_b_row)
        check(
            (gone_b_last, gone_b_deleted) == (gone_b_previous, None),
            failures,
            f"row absent from every live listing should not be visited or tombstoned on peer b: before={(gone_b_previous, None)!r}, after={(gone_b_last, gone_b_deleted)!r}, row={gone_b_row}",
        )
    if gone_c_row is not None:
        gone_c_last = last_seen(gone_c_row)
        gone_c_deleted = deleted_time(gone_c_row)
        check(
            (gone_c_last, gone_c_deleted) == (gone_c_previous, None),
            failures,
            f"row absent from every live listing should not be visited or tombstoned on peer c: before={(gone_c_previous, None)!r}, after={(gone_c_last, gone_c_deleted)!r}, row={gone_c_row}",
        )

    write_file(b / "DeleteVote_02.txt", "recreated on peer b", t0 + 8)

    check_run_ok(
        failures,
        run_cli("+" + str(a), b, c),
        "repeat sync with an already-deleted absent peer row",
    )
    check(
        not (b / "DeleteVote_02.txt").exists(),
        failures,
        "canon-lost recreated DeleteVote_02.txt still exists on peer b after repeat deletion decision",
    )
    check(
        not any((peer / "EverywhereGone_02.txt").exists() for peer in (a, b, c)),
        failures,
        "snapshot rows absent from every live peer listing recreated EverywhereGone_02.txt",
    )
    repeat_delete_c = snapshot_row(failures, c, "DeleteVote_02.txt")
    repeat_c = snapshot_row(failures, c, "EverywhereGone_02.txt")
    if repeat_delete_c is not None:
        check(
            (last_seen(repeat_delete_c), deleted_time(repeat_delete_c))
            == (delete_c_last, delete_c_deleted),
            failures,
            f"already-deleted absent row for DeleteVote_02.txt changed while the entry was visited from peer b: before={(delete_c_last, delete_c_deleted)!r}, after={(last_seen(repeat_delete_c), deleted_time(repeat_delete_c))!r}, row={repeat_delete_c}",
        )
    if repeat_c is not None:
        check(
            (last_seen(repeat_c), deleted_time(repeat_c))
            == (gone_c_last, gone_c_deleted),
            failures,
            f"already-deleted row for EverywhereGone_02.txt changed on repeat sync: before={(gone_c_last, gone_c_deleted)!r}, after={(last_seen(repeat_c), deleted_time(repeat_c))!r}, row={repeat_c}",
        )
    check(
        (delete_c_last, delete_c_deleted) == (delete_c_previous, delete_c_previous),
        failures,
        "confirmed absent DeleteVote_02.txt on peer c should set deleted_time from existing last_seen and leave last_seen unchanged",
    )
    check(
        (gone_b_last, gone_b_deleted) == (gone_b_previous, None),
        failures,
        "EverywhereGone_02.txt absent on all live peers should not create a walk entry or update its orphaned snapshot row",
    )

    if failures:
        print("FAIL")
        for index, failure in enumerate(failures, 1):
            print(f"\n{index}. {failure}")
        return 1
    print("PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
