#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

from __future__ import annotations

import os
import shutil
import sqlite3
import stat
import subprocess
import sys
from pathlib import Path


PROJECT_DIR = Path("/home/ace/Desktop/prjx/kitchensync")
JAVA = PROJECT_DIR / "tools/compiler/jdk/bin/java"
JAR = PROJECT_DIR / "released/kitchensync.jar"
WORK = PROJECT_DIR / "tests/.tmp/04_error-handling"
UNREACHABLE = "sftp://127.0.0.1:1/tmp/kitchensync-unreachable"
NO_DECISIONS = "No contributing peer reachable — cannot make sync decisions"


def make_writable(path: Path) -> None:
    if not path.exists() and not path.is_symlink():
        return
    paths = [path]
    if path.is_dir() and not path.is_symlink():
        paths.extend(p for p in path.rglob("*"))
    for item in reversed(paths):
        try:
            mode = item.lstat().st_mode
            item.chmod(mode | stat.S_IRUSR | stat.S_IWUSR | stat.S_IXUSR)
        except OSError:
            pass


def reset_work() -> None:
    make_writable(WORK)
    shutil.rmtree(WORK, ignore_errors=True)
    WORK.mkdir(parents=True)


def clean_case(name: str) -> Path:
    root = WORK / name
    make_writable(root)
    shutil.rmtree(root, ignore_errors=True)
    root.mkdir(parents=True)
    return root


def write_file(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")


def read_file(path: Path) -> str:
    return path.read_text(encoding="utf-8", errors="replace")


def run_ks(*peers: str, timeout: int = 60) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(JAVA), "-jar", str(JAR), "-vl", "error", "--ct", "1", *peers],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=timeout,
        check=False,
    )


def combined(result: subprocess.CompletedProcess[str]) -> str:
    return result.stdout + "\n" + result.stderr


def mentions_error(output: str) -> bool:
    lowered = output.lower()
    return any(
        word in lowered
        for word in (
            "error",
            "warn",
            "unreachable",
            "fail",
            "denied",
            "refused",
            "cannot",
        )
    )


def check(failures: list[str], condition: bool, message: str) -> None:
    if not condition:
        failures.append(message)


def sync_ok(failures: list[str], message: str, *peers: str) -> bool:
    result = run_ks(*peers)
    ok = result.returncode == 0
    check(
        failures,
        ok,
        f"{message}: expected exit 0, got {result.returncode}; output:\n{combined(result)}",
    )
    return ok


def snapshot_bytes(peer: Path) -> bytes:
    return (peer / ".kitchensync/snapshot.db").read_bytes()


def snapshot_rows(peer: Path, *basenames: str) -> list[tuple[str, str, int, str | None, str | None]]:
    placeholders = ",".join("?" for _ in basenames)
    with sqlite3.connect(peer / ".kitchensync/snapshot.db") as connection:
        return list(
            connection.execute(
                "select basename, mod_time, byte_size, last_seen, deleted_time "
                f"from snapshot where basename in ({placeholders}) order by basename",
                basenames,
            )
        )


def tmp_files(peer: Path) -> list[Path]:
    tmp = peer / ".kitchensync/TMP"
    if not tmp.exists():
        return []
    return [p for p in tmp.rglob("*") if p.is_file()]


def scenario_reachability(failures: list[str]) -> None:
    root = clean_case("reachability")

    source = root / "source"
    target = root / "target"
    source.mkdir()
    target.mkdir()
    write_file(source / "kept.txt", "reachable peers still sync\n")

    result = run_ks("+" + str(source), str(target), UNREACHABLE)
    output = combined(result)
    check(failures, result.returncode == 0, f"04.7: unreachable extra peer should not abort; output:\n{output}")
    check(failures, (target / "kept.txt").exists(), "04.7: run did not continue to sync the remaining reachable peer")
    check(failures, mentions_error(output), f"04.7: unreachable peer was not logged at error verbosity; output:\n{output}")

    result = run_ks("+" + str(source), UNREACHABLE)
    check(failures, result.returncode == 1, f"04.8: fewer than two reachable peers should exit 1; got {result.returncode}")

    result = run_ks("+" + UNREACHABLE, str(target))
    check(failures, result.returncode == 1, f"04.9: unreachable canon peer should exit 1; got {result.returncode}")

    subordinate_a = root / "subordinate-a"
    subordinate_b = root / "subordinate-b"
    subordinate_a.mkdir()
    subordinate_b.mkdir()
    write_file(subordinate_a / "history.txt", "history\n")
    if not sync_ok(failures, "no-contributor setup", "+" + str(subordinate_a), str(subordinate_b)):
        return
    result = run_ks("-" + str(subordinate_a), "-" + str(subordinate_b))
    output = combined(result)
    check(failures, result.returncode == 1, f"04.10: all reachable peers subordinate should exit 1; output:\n{output}")
    check(failures, NO_DECISIONS in output, f"04.10: missing required no-contributor message; output:\n{output}")


def scenario_snapshot_download_failure(failures: list[str]) -> None:
    root = clean_case("snapshot-download")
    canon = root / "canon"
    peer = root / "peer"
    canon.mkdir()
    peer.mkdir()
    write_file(canon / "seed.txt", "seed\n")
    if not sync_ok(failures, "snapshot-download setup", "+" + str(canon), str(peer)):
        return

    snap = peer / ".kitchensync/snapshot.db"
    before = snap.read_bytes()
    snap.chmod(0)
    try:
        result = run_ks("+" + str(canon), str(peer))
        output = combined(result)
        check(failures, result.returncode == 1, f"04.17: unreadable snapshot should re-evaluate reachable count and exit 1; output:\n{output}")
        check(failures, mentions_error(output), f"04.17: snapshot-download failure was not logged; output:\n{output}")
    finally:
        snap.chmod(stat.S_IRUSR | stat.S_IWUSR)

    check(failures, snap.read_bytes() == before, "04.16/04.17: unreachable peer snapshot.db changed during the run")


def scenario_list_dir_failures(failures: list[str]) -> None:
    root = clean_case("list-dir")
    p1 = root / "p1"
    p2 = root / "p2"
    p3 = root / "p3"
    for peer in (p1, p2, p3):
        peer.mkdir()
    write_file(p1 / "sub/old.txt", "old\n")
    if not sync_ok(failures, "list-dir setup", "+" + str(p1), str(p2), str(p3)):
        return

    write_file(p1 / "sub/new.txt", "new from p1\n")
    before_p2_subtree_rows = snapshot_rows(p2, "old.txt", "new.txt")
    (p2 / "sub").chmod(0)
    try:
        result = run_ks(str(p1), str(p2), str(p3))
        output = combined(result)
        check(failures, result.returncode == 0, f"04.11: one peer list_dir failure should not abort; output:\n{output}")
        check(failures, mentions_error(output), f"04.11: list_dir failure was not logged; output:\n{output}")
        check(failures, (p3 / "sub/new.txt").exists(), "04.11: accessible peer did not receive decision for subtree")
    finally:
        (p2 / "sub").chmod(stat.S_IRWXU)

    check(failures, not (p2 / "sub/new.txt").exists(), "04.11: failed-listing peer was not excluded from the affected subtree")
    check(failures, snapshot_rows(p2, "old.txt", "new.txt") == before_p2_subtree_rows, "04.20: failed-listing peer snapshot rows changed for the affected subtree")

    write_file(p3 / "blocked/keep.txt", "subordinate file must remain\n")
    for peer in (p1, p2):
        write_file(peer / "blocked/contributor.txt", "unlistable\n")
        (peer / "blocked").chmod(0)
    try:
        result = run_ks(str(p1), str(p2), "-" + str(p3))
        output = combined(result)
        check(failures, result.returncode == 0, f"04.19: all contributing peers failing one directory should skip it, not abort; output:\n{output}")
        check(failures, (p3 / "blocked/keep.txt").exists(), "04.19: subordinate subtree file was displaced even though no contributor could list the directory")
    finally:
        for peer in (p1, p2):
            (peer / "blocked").chmod(stat.S_IRWXU)


def scenario_transfer_and_tmp_failures(failures: list[str]) -> None:
    root = clean_case("transfer")
    p1 = root / "p1"
    p2 = root / "p2"
    p3 = root / "p3"
    for peer in (p1, p2, p3):
        peer.mkdir()
    write_file(p1 / "seed.txt", "seed\n")
    if not sync_ok(failures, "transfer setup", "+" + str(p1), str(p2), str(p3)):
        return

    shutil.rmtree(p2 / ".kitchensync/TMP", ignore_errors=True)
    write_file(p2 / ".kitchensync/TMP", "not a directory\n")
    write_file(p1 / "fail-a.txt", "tmp staging should fail on p2\n")
    write_file(p1 / "fail-b.txt", "other peer transfers should continue\n")

    result = run_ks(str(p1), str(p2), str(p3))
    output = combined(result)
    check(failures, result.returncode == 0, f"04.12/04.21: transfer failure should not abort the run; output:\n{output}")
    check(failures, mentions_error(output), f"04.12/04.21: TMP/transfer failure was not logged; output:\n{output}")
    check(failures, not (p2 / "fail-a.txt").exists(), "04.21: file was copied despite TMP staging failure")
    check(failures, (p3 / "fail-a.txt").exists() and (p3 / "fail-b.txt").exists(), "04.12: other transfers did not continue after one peer transfer failed")


def scenario_displacement_failure(failures: list[str]) -> None:
    root = clean_case("displacement")
    p1 = root / "p1"
    p2 = root / "p2"
    p1.mkdir()
    p2.mkdir()
    write_file(p1 / "replace.txt", "old\n")
    if not sync_ok(failures, "displacement setup", "+" + str(p1), str(p2)):
        return

    before_tmp = set(tmp_files(p2))
    write_file(p1 / "replace.txt", "new\n")
    os.utime(p1 / "replace.txt", (1_800_000_000, 1_800_000_000))
    (p2 / ".kitchensync/TMP").mkdir(parents=True, exist_ok=True)
    (p2 / ".kitchensync/BAK").mkdir(parents=True, exist_ok=True)
    p2.chmod(stat.S_IRUSR | stat.S_IXUSR)
    try:
        result = run_ks(str(p1), str(p2))
        output = combined(result)
        check(failures, result.returncode == 0, f"04.13/04.15: displacement failure should not abort; output:\n{output}")
        check(failures, mentions_error(output), f"04.13/04.15: displacement failure was not logged; output:\n{output}")
        check(failures, read_file(p2 / "replace.txt") == "old\n", "04.13/04.15: failed displacement did not leave the existing file in place")
    finally:
        p2.chmod(stat.S_IRWXU)

    after_tmp = set(tmp_files(p2))
    check(failures, after_tmp == before_tmp, "04.15: failed copy sequence left a TMP staging file behind")


def scenario_snapshot_upload_failure(failures: list[str]) -> None:
    root = clean_case("snapshot-upload")
    p1 = root / "p1"
    p2 = root / "p2"
    p1.mkdir()
    p2.mkdir()
    write_file(p1 / "seed.txt", "seed\n")
    if not sync_ok(failures, "snapshot-upload setup", "+" + str(p1), str(p2)):
        return

    before = snapshot_bytes(p2)
    write_file(p1 / "later.txt", "forces snapshot update\n")
    tmp = p2 / ".kitchensync/TMP"
    tmp.mkdir(parents=True, exist_ok=True)
    tmp.chmod(stat.S_IRWXU)
    (p2 / ".kitchensync").chmod(stat.S_IRUSR | stat.S_IXUSR)
    try:
        result = run_ks(str(p1), str(p2))
        output = combined(result)
        check(failures, result.returncode == 0, f"04.18: snapshot-upload failure should complete normally; output:\n{output}")
        check(failures, mentions_error(output), f"04.18: snapshot-upload failure was not logged; output:\n{output}")
    finally:
        (p2 / ".kitchensync").chmod(stat.S_IRWXU)
        tmp.chmod(stat.S_IRWXU)

    check(failures, snapshot_bytes(p2) == before, "04.18: existing snapshot.db changed after upload failure")
    check(failures, len(tmp_files(p2)) > 0, "04.18: failed snapshot upload did not retain a staging file under .kitchensync/TMP")


def scenario_set_mod_time_recovery(failures: list[str]) -> None:
    root = clean_case("set-mod-time")
    p1 = root / "p1"
    p2 = root / "p2"
    p1.mkdir()
    p2.mkdir()
    write_file(p1 / "time.txt", "copy remains even if timestamp application fails\n")

    too_old = -22_089_888_000
    try:
        os.utime(p1 / "time.txt", (too_old, too_old))
    except (OverflowError, OSError):
        failures.append("04.14: host filesystem cannot create the required out-of-range source timestamp trigger")
        return

    result = run_ks("+" + str(p1), str(p2))
    output = combined(result)
    check(failures, result.returncode == 0, f"04.14: set_mod_time failure should not abort; output:\n{output}")
    check(failures, (p2 / "time.txt").exists(), "04.14: copied file was undone after set_mod_time failure")
    if mentions_error(output):
        second = run_ks("+" + str(p1), str(p2))
        check(failures, second.returncode == 0, f"04.14: next run should complete after timestamp discrepancy; output:\n{combined(second)}")


def main() -> int:
    failures: list[str] = []
    reset_work()
    try:
        scenario_reachability(failures)
        scenario_snapshot_download_failure(failures)
        scenario_list_dir_failures(failures)
        scenario_transfer_and_tmp_failures(failures)
        scenario_displacement_failure(failures)
        scenario_snapshot_upload_failure(failures)
        scenario_set_mod_time_recovery(failures)
    finally:
        make_writable(WORK)

    if failures:
        print("FAILURES:")
        for failure in failures:
            print(f"- {failure}")
        return 1

    print("04_error-handling: all checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
