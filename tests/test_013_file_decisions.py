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
from datetime import UTC, datetime
from pathlib import Path


sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")


WORKSPACE_ROOT = Path("/home/ace/Desktop/prjx/kitchensync")
PRIMARY_EXE = WORKSPACE_ROOT / "released" / "kitchensync.exe"
if not PRIMARY_EXE.exists():
    PRIMARY_EXE = Path(__file__).resolve().parents[1] / "released" / "kitchensync.exe"

BASE_TIME = 1_700_000_000


@dataclass
class CheckState:
    failures: list[str]

    def check(self, condition: bool, message: str) -> None:
        if not condition:
            self.failures.append(message)


def ts(seconds: int) -> str:
    return datetime.fromtimestamp(seconds, UTC).strftime("%Y-%m-%d_%H-%M-%S_%fZ")


def write_file(path: Path, data: bytes, mtime: int) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)
    os.utime(path, (mtime, mtime))


def remove_path(path: Path) -> None:
    if path.is_dir():
        shutil.rmtree(path)
    elif path.exists():
        path.unlink()


def read_bytes(path: Path) -> bytes | None:
    if not path.exists():
        return None
    return path.read_bytes()


def run_ks(ctx: CheckState, args: list[str], label: str, expect: int | None = 0) -> subprocess.CompletedProcess[str]:
    try:
        proc = subprocess.run(
            [str(PRIMARY_EXE), *args],
            cwd=str(WORKSPACE_ROOT if WORKSPACE_ROOT.exists() else Path(__file__).resolve().parents[1]),
            text=True,
            encoding="utf-8",
            errors="replace",
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=30,
            shell=False,
        )
    except Exception as exc:  # pragma: no cover - reported as test failure
        ctx.failures.append(f"{label}: process failed to run: {exc}")
        return subprocess.CompletedProcess([str(PRIMARY_EXE), *args], 127, "", str(exc))
    if expect is not None:
        ctx.check(proc.returncode == expect, f"{label}: expected exit {expect}, got {proc.returncode}; stdout={proc.stdout!r}")
    ctx.check(proc.stderr == "", f"{label}: stderr should be empty, got {proc.stderr!r}")
    return proc


def snapshot_db(peer: Path) -> Path:
    return peer / ".kitchensync" / "snapshot.db"


def update_snapshot(peer: Path, basename: str, **columns: object) -> None:
    db = snapshot_db(peer)
    assignments = ", ".join(f"{name} = ?" for name in columns)
    values = list(columns.values())
    with sqlite3.connect(str(db)) as conn:
        cur = conn.execute(f"UPDATE snapshot SET {assignments} WHERE basename = ?", [*values, basename])
        if cur.rowcount != 1:
            raise AssertionError(f"expected one snapshot row for {basename!r} in {db}, updated {cur.rowcount}")


def seed(ctx: CheckState, root: Path, names: list[str], files: dict[str, bytes]) -> list[Path]:
    peers = [root / name for name in names]
    for peer in peers:
        peer.mkdir(parents=True, exist_ok=True)
    write_file(peers[0] / "anchor.txt", b"anchor", BASE_TIME)
    for name, data in files.items():
        write_file(peers[0] / name, data, BASE_TIME)
    run_ks(ctx, ["+" + str(peers[0]), *[str(peer) for peer in peers[1:]]], f"seed {names}", 0)
    return peers


def assert_bak_has(ctx: CheckState, peer: Path, basename: str, label: str) -> None:
    bak = peer / ".kitchensync" / "BAK"
    found = bak.exists() and any(path.name == basename for path in bak.rglob(basename))
    ctx.check(found, f"{label}: expected displaced {basename!r} under {bak}")


def startup_errors(ctx: CheckState, root: Path) -> None:
    first_a = root / "first-a"
    first_b = root / "first-b"
    first_a.mkdir()
    first_b.mkdir()
    proc = run_ks(ctx, [str(first_a), str(first_b)], "first sync without canon", 1)
    ctx.check(
        "First sync? Mark the authoritative peer with a leading +" in proc.stdout,
        "013.13: first sync without canon should print the required guidance",
    )

    reachable = root / "reachable"
    reachable.mkdir()
    missing_canon = root / "missing-canon"
    run_ks(ctx, ["--dry-run", "+" + str(missing_canon), str(reachable)], "unreachable canon", 1)

    sub_a, sub_b = seed(ctx, root / "no-contrib", ["sub-a", "sub-b"], {})
    proc = run_ks(ctx, ["-" + str(sub_a), "-" + str(sub_b)], "no contributing peer", 1)
    ctx.check(
        "No contributing peer reachable - cannot make sync decisions" in proc.stdout,
        "013.15: all-subordinate run should print the no-contributing-peer error",
    )


def canon_rules(ctx: CheckState, root: Path) -> None:
    canon, other = seed(ctx, root, ["canon", "other"], {"canon-wins.txt": b"canon", "canon-deletes.txt": b"keep"})

    write_file(other / "canon-wins.txt", b"other-newer-and-larger", BASE_TIME + 100)
    run_ks(ctx, ["+" + str(canon), str(other)], "canon file wins", 0)
    ctx.check(read_bytes(other / "canon-wins.txt") == b"canon", "013.9/013.11: canon file should overwrite a newer peer file")

    remove_path(canon / "canon-deletes.txt")
    write_file(other / "canon-deletes.txt", b"other survives only if canon is ignored", BASE_TIME + 200)
    run_ks(ctx, ["+" + str(canon), str(other)], "canon deletion wins", 0)
    ctx.check(not (other / "canon-deletes.txt").exists(), "013.10/013.11: canon absence should delete the peer file")
    assert_bak_has(ctx, other, "canon-deletes.txt", "013.10")


def unchanged_and_subordinate_targets(ctx: CheckState, root: Path) -> None:
    p1, p2 = seed(ctx, root, ["p1", "p2"], {"same.txt": b"same"})
    p3 = root / "new-peer"
    p3.mkdir()
    proc = run_ks(ctx, [str(p1), str(p2), str(p3)], "matching unchanged files copy to new peer", 0)
    ctx.check(read_bytes(p3 / "same.txt") == b"same", "013.19/013.21: matching unchanged file should be copied to a peer that lacks it")
    ctx.check("C same.txt" in proc.stdout, "013.21: missing active peer should cause a copy progress line")
    ctx.check(read_bytes(p1 / "same.txt") == b"same" and read_bytes(p2 / "same.txt") == b"same", "013.20: matching contributing peers should not be changed")


def modified_new_and_tie_rules(ctx: CheckState, root: Path) -> None:
    p1, p2, p3 = seed(ctx, root, ["p1", "p2", "p3"], {"size.txt": b"old", "time.txt": b"old", "large.txt": b"aaaa", "tie.txt": b"1111", "near.txt": b"1111", "behind.txt": b"old"})

    write_file(p1 / "size.txt", b"larger", BASE_TIME)
    run_ks(ctx, [str(p1), str(p2), str(p3)], "modified by byte size", 0)
    ctx.check(read_bytes(p2 / "size.txt") == b"larger" and read_bytes(p3 / "size.txt") == b"larger", "013.2: different byte size should be treated as modified and propagated")

    write_file(p2 / "time.txt", b"newer-time", BASE_TIME + 100)
    run_ks(ctx, [str(p1), str(p2), str(p3)], "modified by mtime", 0)
    ctx.check(read_bytes(p1 / "time.txt") == b"newer-time" and read_bytes(p3 / "time.txt") == b"newer-time", "013.3/013.22/013.43: newest modified file more than 5 seconds newer should win")

    write_file(p1 / "brand-new.txt", b"new file", BASE_TIME + 120)
    run_ks(ctx, [str(p1), str(p2), str(p3)], "new file propagation", 0)
    ctx.check(read_bytes(p2 / "brand-new.txt") == b"new file" and read_bytes(p3 / "brand-new.txt") == b"new file", "013.5/013.23/013.24: newest new file should be copied to peers with no row")

    write_file(p1 / "large.txt", b"small", BASE_TIME + 130)
    write_file(p2 / "large.txt", b"much-larger", BASE_TIME + 130)
    run_ks(ctx, [str(p1), str(p2), str(p3)], "larger tied file wins", 0)
    ctx.check(read_bytes(p1 / "large.txt") == b"much-larger", "013.33: larger byte size should win when modification times are tied")

    write_file(p1 / "tie.txt", b"AAAA", BASE_TIME + 140)
    write_file(p2 / "tie.txt", b"BBBB", BASE_TIME + 140)
    remove_path(p3 / "tie.txt")
    update_snapshot(p3, "tie.txt", last_seen=None, deleted_time=None)
    run_ks(ctx, [str(p1), str(p2), str(p3)], "exact tie keeps peer data", 0)
    ctx.check(read_bytes(p1 / "tie.txt") == b"AAAA" and read_bytes(p2 / "tie.txt") == b"BBBB", "013.35/013.36: equal size and tied mtime should not copy between tied peers")
    ctx.check(read_bytes(p3 / "tie.txt") in {b"AAAA", b"BBBB"}, "013.37: a peer needing an exactly tied file should receive it from one tied source")

    write_file(p1 / "near.txt", b"AAAA", BASE_TIME + 160)
    write_file(p2 / "near.txt", b"BBBB", BASE_TIME + 157)
    run_ks(ctx, [str(p1), str(p2)], "within five seconds avoids copy", 0)
    ctx.check(read_bytes(p2 / "near.txt") == b"BBBB", "013.41/013.44: same size within 5 seconds of the max mtime should be treated as already matching")

    write_file(p1 / "behind.txt", b"new", BASE_TIME + 200)
    write_file(p2 / "behind.txt", b"older-but-larger", BASE_TIME + 194)
    run_ks(ctx, [str(p1), str(p2)], "older than tolerance loses", 0)
    ctx.check(read_bytes(p2 / "behind.txt") == b"new", "013.45: a file more than 5 seconds behind the max mtime should lose despite larger size")


def deleted_and_absent_rules(ctx: CheckState, root: Path) -> None:
    p1, p2 = seed(ctx, root, ["p1", "p2"], {"deleted.txt": b"live", "near-delete.txt": b"live", "unconfirmed-delete.txt": b"live", "null-last-seen.txt": b"live", "recent-last-seen.txt": b"live"})

    remove_path(p1 / "deleted.txt")
    update_snapshot(p1, "deleted.txt", deleted_time=ts(BASE_TIME + 20))
    run_ks(ctx, [str(p1), str(p2)], "deleted estimate wins", 0)
    ctx.check(not (p2 / "deleted.txt").exists(), "013.6/013.25/013.26/013.27: deletion estimate newer by more than 5 seconds should delete existing files")
    assert_bak_has(ctx, p2, "deleted.txt", "013.27")

    remove_path(p1 / "near-delete.txt")
    update_snapshot(p1, "near-delete.txt", deleted_time=ts(BASE_TIME + 3))
    run_ks(ctx, [str(p1), str(p2)], "existing wins near deletion", 0)
    ctx.check(read_bytes(p1 / "near-delete.txt") == b"live", "013.28/013.34: existing file should win when deletion is not more than 5 seconds newer")

    remove_path(p1 / "unconfirmed-delete.txt")
    update_snapshot(p1, "unconfirmed-delete.txt", last_seen=ts(BASE_TIME + 30), deleted_time=None)
    run_ks(ctx, [str(p1), str(p2)], "absent unconfirmed last_seen deletion", 0)
    ctx.check(not (p2 / "unconfirmed-delete.txt").exists(), "013.7/013.29: absent-unconfirmed with last_seen more than 5 seconds newer should vote deletion")

    remove_path(p1 / "null-last-seen.txt")
    update_snapshot(p1, "null-last-seen.txt", last_seen=None, deleted_time=None)
    run_ks(ctx, [str(p1), str(p2)], "absent unconfirmed null last_seen", 0)
    ctx.check(read_bytes(p1 / "null-last-seen.txt") == b"live", "013.30/013.32: NULL last_seen should not vote deletion and should receive the file")

    remove_path(p1 / "recent-last-seen.txt")
    update_snapshot(p1, "recent-last-seen.txt", last_seen=ts(BASE_TIME + 3), deleted_time=None)
    run_ks(ctx, [str(p1), str(p2)], "absent unconfirmed within tolerance", 0)
    ctx.check(read_bytes(p1 / "recent-last-seen.txt") == b"live", "013.31/013.32: last_seen within 5 seconds should not vote deletion and should receive the file")


def no_row_and_subordinate_cleanup(ctx: CheckState, root: Path) -> None:
    p1, p2 = seed(ctx, root, ["p1", "p2"], {})
    sub = root / "sub"
    sub.mkdir()
    write_file(sub / "sub-only.txt", b"subordinate data", BASE_TIME + 50)
    run_ks(ctx, [str(p1), str(p2), "-" + str(sub)], "subordinate-only no-row file", 0)
    ctx.check(not (sub / "sub-only.txt").exists(), "013.8/013.17/013.38/013.40: subordinate-only file with no contributing vote should be displaced")
    ctx.check(not (p1 / "sub-only.txt").exists() and not (p2 / "sub-only.txt").exists(), "013.39: no copy should be selected when all contributing peers are absent with no row")
    assert_bak_has(ctx, sub, "sub-only.txt", "013.40")


def tolerance_classification(ctx: CheckState, root: Path) -> None:
    p1, p2 = seed(ctx, root, ["p1", "p2"], {"within.txt": b"AAAA"})
    write_file(p1 / "within.txt", b"BBBB", BASE_TIME + 3)
    run_ks(ctx, [str(p1), str(p2)], "snapshot mtime within tolerance", 0)
    ctx.check(read_bytes(p1 / "within.txt") == b"BBBB", "013.1/013.42: live mtime within 5 seconds and same size should be treated as matching, so bytes are not used to force a copy")


def live_file_with_tombstone_row(ctx: CheckState, root: Path) -> None:
    p1, p2 = seed(ctx, root, ["p1", "p2"], {"resurrected.txt": b"old"})
    update_snapshot(p1, "resurrected.txt", deleted_time=ts(BASE_TIME + 10))
    write_file(p1 / "resurrected.txt", b"resurrected", BASE_TIME + 40)
    remove_path(p2 / "resurrected.txt")
    update_snapshot(p2, "resurrected.txt", deleted_time=ts(BASE_TIME + 5))
    run_ks(ctx, [str(p1), str(p2)], "live file with tombstone row", 0)
    ctx.check(read_bytes(p2 / "resurrected.txt") == b"resurrected", "013.4: live file with non-NULL deleted_time should be treated as modified")


def main() -> int:
    ctx = CheckState([])
    scenarios = [
        startup_errors,
        canon_rules,
        unchanged_and_subordinate_targets,
        modified_new_and_tie_rules,
        deleted_and_absent_rules,
        no_row_and_subordinate_cleanup,
        tolerance_classification,
        live_file_with_tombstone_row,
    ]
    with tempfile.TemporaryDirectory(prefix="kitchensync-013-") as tmp:
        base = Path(tmp)
        for scenario in scenarios:
            try:
                scenario(ctx, base / scenario.__name__)
            except Exception as exc:
                ctx.failures.append(f"{scenario.__name__}: unexpected exception: {exc}")

    if ctx.failures:
        print("test_013_file_decisions.py failures:")
        for failure in ctx.failures:
            print(f"- {failure}")
        return 1
    print("test_013_file_decisions.py passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
