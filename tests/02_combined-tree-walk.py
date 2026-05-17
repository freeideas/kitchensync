#!/usr/bin/env uvrun
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
import time
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

PROJECT_DIR = Path("C:/Users/human/Desktop/prjx/kitchensync")
JAVA = Path("C:/Users/human/Desktop/prjx/kitchensync/tools/compiler/jdk/bin/java.exe")
JAR = Path("C:/Users/human/Desktop/prjx/kitchensync/released/kitchensync.jar")
WORK = PROJECT_DIR / "tmp" / "02_combined_tree_walk"


def run_sync(*peers: Path | str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(JAVA), "-jar", str(JAR), "-vl", "error", *(str(peer) for peer in peers)],
        cwd=PROJECT_DIR,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=60,
        check=False,
    )


def reset(path: Path) -> None:
    if path.exists():
        for root, dirs, files in os.walk(path, topdown=False):
            for name in files:
                try:
                    (Path(root) / name).chmod(0o600)
                except OSError:
                    pass
            for name in dirs:
                try:
                    (Path(root) / name).chmod(0o700)
                except OSError:
                    pass
        shutil.rmtree(path)
    path.mkdir(parents=True, exist_ok=True)


def write_file(path: Path, text: str, mtime: float) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="")
    os.utime(path, (mtime, mtime))


def read_file(path: Path) -> str | None:
    if not path.is_file():
        return None
    return path.read_text(encoding="utf-8")


def user_paths(peer: Path) -> set[str]:
    found: set[str] = set()
    for path in peer.rglob("*"):
        rel = path.relative_to(peer).as_posix()
        if rel == ".kitchensync" or rel.startswith(".kitchensync/"):
            continue
        found.add(rel)
    return found


def child_names(path: Path) -> set[str]:
    if not path.is_dir():
        return set()
    return {child.name for child in path.iterdir()}


def snapshot_db(peer: Path) -> Path:
    return peer / ".kitchensync" / "snapshot.db"


def db_time(seconds: float) -> str:
    return datetime.fromtimestamp(seconds, UTC).strftime("%Y-%m-%d_%H-%M-%S_%fZ")


def snapshot_rows(peer: Path) -> list[dict[str, Any]]:
    db = snapshot_db(peer)
    rows: list[dict[str, Any]] = []
    with tempfile.TemporaryDirectory(
        prefix="snapshot-read-", dir=WORK, ignore_cleanup_errors=True
    ) as temp:
        copy = Path(temp) / "snapshot.db"
        shutil.copy2(db, copy)
        with sqlite3.connect(f"file:{copy.as_posix()}?mode=ro&immutable=1", uri=True) as conn:
            conn.row_factory = sqlite3.Row
            tables = [
                row[0]
                for row in conn.execute(
                    "select name from sqlite_master where type = 'table'"
                )
                if not row[0].startswith("sqlite_")
            ]
            for table in tables:
                quoted = '"' + table.replace('"', '""') + '"'
                for row in conn.execute(f"select rowid, * from {quoted}"):
                    item = dict(row)
                    item["__table__"] = table
                    rows.append(item)
    return rows


def row_for(peer: Path, basename: str) -> dict[str, Any] | None:
    matches: list[dict[str, Any]] = []
    for row in snapshot_rows(peer):
        for key, value in row.items():
            if key == "__table__" or value is None:
                continue
            text = str(value).replace("\\", "/").strip("/")
            if text == basename or text.endswith("/" + basename):
                matches.append(row)
                break
    if len(matches) == 1:
        return matches[0]
    return None


def value(row: dict[str, Any] | None, *names: str) -> Any:
    if row is None:
        return None
    wanted = {name.lower() for name in names}
    for key, current in row.items():
        if key.lower() in wanted:
            return current
    return None


def last_seen(row: dict[str, Any] | None) -> Any:
    return value(row, "last_seen", "lastSeen")


def deleted_time(row: dict[str, Any] | None) -> Any:
    return value(row, "deleted_time", "deletedTime")


def mod_time(row: dict[str, Any] | None) -> Any:
    return value(row, "mod_time", "modTime", "mtime")


def byte_size(row: dict[str, Any] | None) -> Any:
    return value(row, "byte_size", "byteSize")


def check(failures: list[str], condition: bool, message: str) -> None:
    if not condition:
        failures.append(message)


def check_run(failures: list[str], result: subprocess.CompletedProcess[str], label: str) -> bool:
    if result.returncode == 0:
        return True
    failures.append(
        f"{label}: kitchensync exited {result.returncode}\n"
        f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}"
    )
    return False


def check_live_row(
    failures: list[str], peer: Path, basename: str, req: str
) -> dict[str, Any] | None:
    row = row_for(peer, basename)
    check(failures, row is not None, f"{req}: {peer.name} has no snapshot row for {basename}")
    check(
        failures,
        last_seen(row) is not None,
        f"{req}: {peer.name} row for {basename} should have last_seen set: {row}",
    )
    check(
        failures,
        deleted_time(row) is None,
        f"{req}: {peer.name} row for {basename} should be live: {row}",
    )
    return row


def check_deleted_from_previous_last_seen(
    failures: list[str],
    peer: Path,
    basename: str,
    previous_last_seen: Any,
    req: str,
) -> tuple[Any, Any]:
    row = row_for(peer, basename)
    current_last_seen = last_seen(row)
    current_deleted_time = deleted_time(row)
    check(failures, row is not None, f"{req}: {peer.name} has no snapshot row for {basename}")
    check(
        failures,
        current_last_seen == previous_last_seen,
        f"{req}: confirming {basename} absent must not change {peer.name} last_seen; "
        f"before={previous_last_seen!r}, after={current_last_seen!r}, row={row}",
    )
    check(
        failures,
        current_deleted_time == previous_last_seen,
        f"{req}: {peer.name} deleted_time for {basename} should be existing last_seen; "
        f"before={previous_last_seen!r}, after={current_deleted_time!r}, row={row}",
    )
    return current_last_seen, current_deleted_time


def main() -> int:
    failures: list[str] = []
    reset(WORK)
    peer_a = WORK / "peer-a"
    peer_b = WORK / "peer-b"
    peer_c = WORK / "peer-c"
    for peer in (peer_a, peer_b, peer_c):
        peer.mkdir(parents=True, exist_ok=True)

    base = int(time.time()) - 600
    write_file(peer_a / "RootOnly_02.txt", "root from a", base)
    write_file(peer_a / "SharedDir_02" / "CanonChild_02.txt", "child from a", base + 1)
    write_file(peer_a / "MixedCase_02.TxT", "case preserved", base + 2)
    write_file(peer_a / "DeleteVote_02.txt", "delete me later", base + 3)
    write_file(peer_a / "GoneEverywhere_02.txt", "orphan snapshot row", base + 4)

    if not check_run(failures, run_sync("+" + str(peer_a), peer_b, peer_c), "setup canon sync"):
        print("FAIL tests/02_combined-tree-walk.py")
        for failure in failures:
            print(f"- {failure}")
        return 1

    root_a_last_seen_before = last_seen(row_for(peer_a, "RootOnly_02.txt"))
    copied_root_b = check_live_row(failures, peer_b, "RootOnly_02.txt", "02.28/02.47")
    created_dir_c = check_live_row(failures, peer_c, "SharedDir_02", "02.37")
    check(
        failures,
        byte_size(copied_root_b) == len("root from a")
        and mod_time(copied_root_b) == db_time(base),
        f"02.47: completed file copy should record winning byte_size and mod_time; "
        f"row={copied_root_b}",
    )

    time.sleep(1.1)
    write_file(peer_b / "PeerBOnly_02.txt", "root from b", base + 20)
    write_file(peer_b / "SharedDir_02" / "PeerBChild_02.txt", "child from b", base + 21)

    check_run(failures, run_sync(peer_a, peer_b, peer_c), "unioned combined-tree sync")

    # 02.26 exact per-directory visit count, 02.29 pre-order, 02.30 recursion
    # peer-set filtering for peers that do not keep a directory, 02.31
    # decision-before-operation timing, and the pre-completion last_seen part of
    # 02.47 require observing intra-run traversal/operation sequencing. The
    # released CLI exposes only completed filesystem and snapshot outcomes, so
    # those parts are not reasonably testable here.
    expected = {
        "RootOnly_02.txt",
        "PeerBOnly_02.txt",
        "SharedDir_02",
        "SharedDir_02/CanonChild_02.txt",
        "SharedDir_02/PeerBChild_02.txt",
        "MixedCase_02.TxT",
        "DeleteVote_02.txt",
        "GoneEverywhere_02.txt",
    }
    for peer in (peer_a, peer_b, peer_c):
        paths = user_paths(peer)
        check(
            failures,
            expected <= paths,
            f"02.27: {peer.name} is missing unioned paths "
            f"{sorted(expected - paths)} after one recursive combined-tree walk",
        )

    check(
        failures,
        read_file(peer_c / "SharedDir_02" / "CanonChild_02.txt") == "child from a"
        and read_file(peer_c / "SharedDir_02" / "PeerBChild_02.txt") == "child from b",
        "02.27: shared directory should include entries unioned from all reachable peers",
    )
    check(
        failures,
        read_file(peer_b / "MixedCase_02.TxT") == "case preserved"
        and "mixedcase_02.txt" not in child_names(peer_b),
        "02.48: destination basename spelling should exactly preserve source case",
    )

    check_live_row(failures, peer_a, "RootOnly_02.txt", "02.34")
    check_live_row(failures, peer_b, "PeerBChild_02.txt", "02.34")

    check(
        failures,
        last_seen(row_for(peer_a, "RootOnly_02.txt")) != root_a_last_seen_before,
        "02.34: confirming a present source entry during a later traversal should refresh last_seen",
    )
    check(
        failures,
        last_seen(copied_root_b) is not None and last_seen(created_dir_c) is not None,
        "02.28/02.37: copied files and inline-created directories should have last_seen after success",
    )

    delete_b_last_seen_before = last_seen(row_for(peer_b, "DeleteVote_02.txt"))
    delete_c_last_seen_before = last_seen(row_for(peer_c, "DeleteVote_02.txt"))
    orphan_b_state_before = (
        last_seen(row_for(peer_b, "GoneEverywhere_02.txt")),
        deleted_time(row_for(peer_b, "GoneEverywhere_02.txt")),
    )

    for path in (peer_a / "DeleteVote_02.txt", peer_c / "DeleteVote_02.txt"):
        if path.exists():
            path.unlink()
    for peer in (peer_a, peer_b, peer_c):
        path = peer / "GoneEverywhere_02.txt"
        if path.exists():
            path.unlink()

    check_run(failures, run_sync("+" + str(peer_a), peer_b, peer_c), "canon deletion sync")

    check(
        failures,
        not (peer_b / "DeleteVote_02.txt").exists(),
        "02.39: canon deletion decision should remove a peer's still-live copy",
    )
    delete_b_state = check_deleted_from_previous_last_seen(
        failures,
        peer_b,
        "DeleteVote_02.txt",
        delete_b_last_seen_before,
        "02.39/02.55",
    )
    delete_c_state = check_deleted_from_previous_last_seen(
        failures,
        peer_c,
        "DeleteVote_02.txt",
        delete_c_last_seen_before,
        "02.35/02.55",
    )
    orphan_b_state_after = (
        last_seen(row_for(peer_b, "GoneEverywhere_02.txt")),
        deleted_time(row_for(peer_b, "GoneEverywhere_02.txt")),
    )
    check(
        failures,
        orphan_b_state_after == orphan_b_state_before,
        f"02.44: snapshot rows absent from every live peer listing should not create walk entries; "
        f"before={orphan_b_state_before!r}, after={orphan_b_state_after!r}",
    )

    write_file(peer_b / "DeleteVote_02.txt", "live loser again", base + 30)
    check_run(
        failures,
        run_sync("+" + str(peer_a), peer_b, peer_c),
        "already-deleted absent-row sync",
    )
    delete_c_state_after = (
        last_seen(row_for(peer_c, "DeleteVote_02.txt")),
        deleted_time(row_for(peer_c, "DeleteVote_02.txt")),
    )
    check(
        failures,
        delete_c_state_after == delete_c_state,
        f"02.36: already-deleted absent row should be left unchanged; "
        f"before={delete_c_state!r}, after={delete_c_state_after!r}",
    )

    if failures:
        print("FAIL tests/02_combined-tree-walk.py")
        for failure in failures:
            print(f"- {failure}")
        return 1

    print("PASS tests/02_combined-tree-walk.py")
    return 0


if __name__ == "__main__":
    sys.exit(main())
