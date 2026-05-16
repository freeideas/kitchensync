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
import time
from datetime import UTC, datetime
from pathlib import Path
from typing import Any


PROJECT_DIR = Path("/home/ace/Desktop/prjx/kitchensync")
JAVA = Path("/home/ace/Desktop/prjx/kitchensync/tools/compiler/jdk/bin/java")
JAR = Path("/home/ace/Desktop/prjx/kitchensync/released/kitchensync.jar")
WORK = PROJECT_DIR / ".tmp_test_03_decision_rules"


def run_cli(*peers: Path | str) -> tuple[bool, str]:
    args = [str(peer) for peer in peers]
    try:
        result = subprocess.run(
            [str(JAVA), "-jar", str(JAR), *args],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=60,
            check=False,
        )
    except Exception as exc:
        return False, f"failed to launch kitchensync: {exc}"
    if result.returncode == 0:
        return True, ""
    return (
        False,
        "kitchensync exited "
        f"{result.returncode}\nstdout:\n{result.stdout}\nstderr:\n{result.stderr}",
    )


def reset_dir(path: Path) -> None:
    if path.exists():
        shutil.rmtree(path)
    path.mkdir(parents=True)


def write_file(path: Path, text: str, mtime: float) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="")
    os.utime(path, (mtime, mtime))


def read_file(path: Path) -> str | None:
    if not path.exists():
        return None
    return path.read_text(encoding="utf-8")


def stat_state(path: Path) -> tuple[int, int] | None:
    if not path.exists():
        return None
    st = path.stat()
    return (round(st.st_mtime), st.st_size)


def db_path(peer: Path) -> Path:
    return peer / ".kitchensync" / "snapshot.db"


def snapshot_row(peer: Path, basename: str) -> tuple[str, dict[str, Any]]:
    db = db_path(peer)
    with sqlite3.connect(db) as conn:
        conn.row_factory = sqlite3.Row
        tables = [
            row[0]
            for row in conn.execute(
                "select name from sqlite_master where type = 'table'"
            )
        ]
        for table in tables:
            columns = [
                row[1]
                for row in conn.execute(f'pragma table_info("{table}")')
            ]
            if not {"deleted_time", "mod_time", "byte_size"}.issubset(columns):
                continue
            rows = conn.execute(f'select rowid, * from "{table}"').fetchall()
            for row in rows:
                values = dict(row)
                if any(value == basename for value in values.values()):
                    return table, values
    raise AssertionError(f"no snapshot row found for {basename} in {db}")


def db_time_from_seconds(seconds: float) -> str:
    return datetime.fromtimestamp(seconds, UTC).strftime("%Y-%m-%d_%H-%M-%S_%fZ")


def update_snapshot(peer: Path, basename: str, **updates: Any) -> None:
    table, row = snapshot_row(peer, basename)
    assignments = ", ".join(f'"{column}" = ?' for column in updates)
    values = [*updates.values(), row["rowid"]]
    with sqlite3.connect(db_path(peer)) as conn:
        conn.execute(
            f'update "{table}" set {assignments} where rowid = ?',
            values,
        )
        conn.commit()


def expect(failures: list[str], condition: bool, message: str) -> None:
    if not condition:
        failures.append(message)


def synced_peer_set(name: str, files: dict[str, tuple[str, float]]) -> list[Path]:
    root = WORK / name
    peers = [root / "a", root / "b", root / "c"]
    for peer in peers:
        reset_dir(peer)
    for rel, (text, mtime) in files.items():
        write_file(peers[0] / rel, text, mtime)
    ok, detail = run_cli("+" + str(peers[0]), peers[1], peers[2])
    if not ok:
        raise AssertionError(f"initial sync failed for {name}: {detail}")
    return peers


def scenario_agreement_newest_tolerance_size(failures: list[str]) -> None:
    root = WORK / "agreement_newest_tolerance_size"
    a, b, c = root / "a", root / "b", root / "c"
    for peer in (a, b, c):
        reset_dir(peer)

    base = time.time() - 300
    write_file(a / "same.txt", "same", base)
    write_file(a / "newer.txt", "old", base)
    write_file(a / "tie-size.txt", "small", base)
    ok, detail = run_cli("+" + str(a), b, c)
    expect(failures, ok, f"03 setup initial sync failed: {detail}")
    if not ok:
        return

    write_file(b / "same.txt", "diff", base)
    before = stat_state(b / "same.txt")
    write_file(a / "newer.txt", "winner", base + 80)
    write_file(b / "tie-size.txt", "larger", base + 2)
    ok, detail = run_cli(a, b, c)
    expect(failures, ok, f"03.1/03.2/03.6/03.7 sync failed: {detail}")
    if not ok:
        return

    expect(
        failures,
        stat_state(b / "same.txt") == before and read_file(b / "same.txt") == "diff",
        "03.1: agreeing peers should not receive an unnecessary copy",
    )
    expect(
        failures,
        read_file(b / "newer.txt") == "winner" and read_file(c / "newer.txt") == "winner",
        "03.2: newest mod_time should propagate to older peers",
    )
    expect(
        failures,
        read_file(a / "tie-size.txt") == "larger" and read_file(c / "tie-size.txt") == "larger",
        "03.6/03.7: within the 5 second mod_time tie, larger byte_size should win",
    )


def scenario_new_absent_and_no_contributor(failures: list[str]) -> None:
    root = WORK / "new_absent_and_no_contributor"
    a, b, c, sub = root / "a", root / "b", root / "c", root / "sub"
    for peer in (a, b, c, sub):
        reset_dir(peer)

    base = time.time() - 200
    write_file(a / "seed.txt", "seed", base)
    ok, detail = run_cli("+" + str(a), b)
    expect(failures, ok, f"03 setup seed sync failed: {detail}")
    if not ok:
        return

    write_file(a / "new.txt", "new", base + 20)
    write_file(sub / "sub-only.txt", "ignored", base + 30)
    ok, detail = run_cli(a, b, c, "-" + str(sub))
    expect(failures, ok, f"03.3/03.8/03.110 sync failed: {detail}")
    if not ok:
        return

    expect(
        failures,
        read_file(b / "new.txt") == "new"
        and read_file(c / "new.txt") == "new"
        and read_file(sub / "new.txt") == "new",
        "03.3/03.110: decided existing files should copy to absent peers with no snapshot row",
    )
    expect(
        failures,
        not (a / "sub-only.txt").exists() and not (b / "sub-only.txt").exists(),
        "03.8: entries present only on a subordinate peer should not be copied to contributors",
    )


def scenario_no_snapshot_live_peer_does_not_vote(failures: list[str]) -> None:
    root = WORK / "no_snapshot_live_peer_does_not_vote"
    a, b, c = root / "a", root / "b", root / "c"
    for peer in (a, b, c):
        reset_dir(peer)

    base = time.time() - 200
    write_file(a / "nonvoter.txt", "tracked", base)
    ok, detail = run_cli("+" + str(a), b)
    expect(failures, ok, f"03.110 setup sync failed: {detail}")
    if not ok:
        return

    write_file(c / "nonvoter.txt", "untracked-newer", base + 80)
    ok, detail = run_cli(a, b, c)
    expect(failures, ok, f"03.110 sync failed: {detail}")
    if not ok:
        return

    expect(
        failures,
        read_file(a / "nonvoter.txt") == "tracked"
        and read_file(b / "nonvoter.txt") == "tracked",
        "03.110: a contributing peer with no snapshot row should not vote on the winner",
    )


def scenario_deletion_timing(failures: list[str]) -> None:
    old_a, old_b, _ = synced_peer_set(
        "deletion_more_than_five_seconds",
        {"gone.txt": ("old", time.time() - 600)},
    )
    (old_b / "gone.txt").unlink()
    update_snapshot(
        old_b,
        "gone.txt",
        deleted_time=db_time_from_seconds(time.time()),
    )
    ok, detail = run_cli(old_a, old_b)
    expect(failures, ok, f"03.4 sync failed: {detail}")
    if ok:
        expect(
            failures,
            not (old_a / "gone.txt").exists() and not (old_b / "gone.txt").exists(),
            "03.4: deletion more than 5 seconds after surviving mod_time should displace live copies",
        )

    future_a, future_b, _ = synced_peer_set(
        "deletion_not_more_than_five_seconds",
        {"kept.txt": ("future", time.time() + 120)},
    )
    (future_b / "kept.txt").unlink()
    update_snapshot(
        future_b,
        "kept.txt",
        deleted_time=db_time_from_seconds(time.time()),
    )
    ok, detail = run_cli(future_a, future_b)
    expect(failures, ok, f"03.14 sync failed: {detail}")
    if ok:
        expect(
            failures,
            read_file(future_b / "kept.txt") == "future",
            "03.14: tombstone not more than 5 seconds after live mod_time should receive the file",
        )


def scenario_missing_file_last_seen(failures: list[str]) -> None:
    recopy_a, recopy_b, _ = synced_peer_set(
        "missing_recopy",
        {"recopy.txt": ("recopy", time.time() + 120)},
    )
    (recopy_b / "recopy.txt").unlink()
    ok, detail = run_cli(recopy_a, recopy_b)
    expect(failures, ok, f"03.5 sync failed: {detail}")
    if ok:
        expect(
            failures,
            read_file(recopy_b / "recopy.txt") == "recopy",
            "03.5: absent file with last_seen not exceeding max mod_time by more than 5 seconds should be re-copied",
        )

    delete_a, delete_b, _ = synced_peer_set(
        "missing_displace",
        {"displace.txt": ("displace", time.time() - 600)},
    )
    (delete_b / "displace.txt").unlink()
    ok, detail = run_cli(delete_a, delete_b)
    expect(failures, ok, f"03.18 sync failed: {detail}")
    if ok:
        expect(
            failures,
            not (delete_a / "displace.txt").exists() and not (delete_b / "displace.txt").exists(),
            "03.18: absent file with last_seen more than 5 seconds after max mod_time should displace live copies",
        )


def scenario_multiple_deletions(failures: list[str]) -> None:
    a, b, c = synced_peer_set(
        "multiple_deletions",
        {"multi-delete.txt": ("survivor", time.time() - 10)},
    )
    survivor_mtime = time.time() - 10
    os.utime(a / "multi-delete.txt", (survivor_mtime, survivor_mtime))
    (b / "multi-delete.txt").unlink()
    (c / "multi-delete.txt").unlink()

    update_snapshot(
        b,
        "multi-delete.txt",
        deleted_time=db_time_from_seconds(survivor_mtime - 20),
    )
    update_snapshot(
        c,
        "multi-delete.txt",
        deleted_time=db_time_from_seconds(survivor_mtime + 20),
    )
    ok, detail = run_cli(a, b, c)
    expect(failures, ok, f"03.85 sync failed: {detail}")
    if ok:
        expect(
            failures,
            not (a / "multi-delete.txt").exists(),
            "03.85: most recent deletion estimate should decide against a surviving older file",
        )


def scenario_resurrection_and_matching_destination(failures: list[str]) -> None:
    a, b, _ = synced_peer_set(
        "resurrection",
        {"rise.txt": ("old", time.time() - 100)},
    )
    (b / "rise.txt").unlink()
    update_snapshot(
        b,
        "rise.txt",
        deleted_time=db_time_from_seconds(time.time() - 50),
    )
    write_file(b / "rise.txt", "resurrected", time.time() + 30)
    ok, detail = run_cli(a, b)
    expect(failures, ok, f"03.91/03.19 sync failed: {detail}")
    if ok:
        _, after = snapshot_row(b, "rise.txt")
        expect(
            failures,
            read_file(a / "rise.txt") == "resurrected",
            "03.91: live file with a tombstoned snapshot row should be treated as modified",
        )
        expect(
            failures,
            after["deleted_time"] is None,
            "03.19: resurrection should clear deleted_time in the updated snapshot row",
        )

    root = WORK / "matching_destination"
    src, seeded, dst = root / "src", root / "seeded", root / "dst"
    for peer in (src, seeded, dst):
        reset_dir(peer)
    mtime = time.time() - 60
    write_file(src / "same-state.txt", "AAAA", mtime)
    ok, detail = run_cli("+" + str(src), seeded)
    expect(failures, ok, f"03.92 setup sync failed: {detail}")
    if not ok:
        return
    write_file(dst / "same-state.txt", "BBBB", mtime + 2)
    ok, detail = run_cli(src, dst)
    expect(failures, ok, f"03.92 sync failed: {detail}")
    if ok:
        expect(
            failures,
            read_file(dst / "same-state.txt") == "BBBB",
            "03.92: destination with matching byte_size and mod_time tolerance should not be copied over",
        )
        expect(
            failures,
            snapshot_row(dst, "same-state.txt")[1]["deleted_time"] is None,
            "03.92: matching destination should still get a live snapshot row",
        )


def main() -> int:
    reset_dir(WORK)
    failures: list[str] = []
    scenarios = [
        scenario_agreement_newest_tolerance_size,
        scenario_new_absent_and_no_contributor,
        scenario_no_snapshot_live_peer_does_not_vote,
        scenario_deletion_timing,
        scenario_missing_file_last_seen,
        scenario_multiple_deletions,
        scenario_resurrection_and_matching_destination,
    ]
    try:
        for scenario in scenarios:
            try:
                scenario(failures)
            except Exception as exc:
                failures.append(f"{scenario.__name__} raised {exc!r}")
    finally:
        shutil.rmtree(WORK, ignore_errors=True)

    if failures:
        print("FAIL: 03_decision-rules")
        for failure in failures:
            print(f"- {failure}")
        return 1
    print("PASS: 03_decision-rules")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
