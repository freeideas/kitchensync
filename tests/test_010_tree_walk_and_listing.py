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
import tempfile
from pathlib import Path


sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")


WORKSPACE_ROOT = Path("/home/ace/Desktop/prjx/kitchensync")
KITCHENSYNC = WORKSPACE_ROOT / "released" / "kitchensync.exe"


class CheckFailure(Exception):
    pass


def record(failures: list[str], label: str, func) -> None:
    try:
        func()
    except CheckFailure as exc:
        failures.append(f"{label}: {exc}")
    except Exception as exc:  # noqa: BLE001 - collect all end-to-end failures
        failures.append(f"{label}: unexpected {type(exc).__name__}: {exc}")


def require(condition: bool, message: str) -> None:
    if not condition:
        raise CheckFailure(message)


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")


def read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def run_sync(args: list[Path | str], cwd: Path) -> subprocess.CompletedProcess[str]:
    command = [str(KITCHENSYNC), *[str(arg) for arg in args]]
    return subprocess.run(
        command,
        cwd=str(cwd),
        text=True,
        encoding="utf-8",
        errors="replace",
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=30,
        check=False,
    )


def assert_success(result: subprocess.CompletedProcess[str], context: str) -> None:
    require(
        result.returncode == 0,
        f"{context} exited {result.returncode}; stdout={result.stdout!r}; stderr={result.stderr!r}",
    )
    require(result.stderr == "", f"{context} wrote to stderr: {result.stderr!r}")
    require(
        "sync complete" in result.stdout.splitlines(),
        f"{context} did not print completion line; stdout={result.stdout!r}",
    )


def progress_lines(result: subprocess.CompletedProcess[str]) -> list[str]:
    return [
        line
        for line in result.stdout.splitlines()
        if len(line) > 2 and line[1] == " " and line[0] in {"C", "X"}
    ]


def line_index(lines: list[str], needle: str) -> int:
    try:
        return lines.index(needle)
    except ValueError as exc:
        raise CheckFailure(f"missing progress line {needle!r}; got {lines!r}") from exc


def snapshot_rows(peer: Path) -> list[tuple[str, int, str | None, str | None]]:
    db = peer / ".kitchensync" / "snapshot.db"
    require(db.exists(), f"snapshot database does not exist at {db}")
    with sqlite3.connect(str(db)) as conn:
        return list(
            conn.execute(
                "SELECT basename, byte_size, last_seen, deleted_time "
                "FROM snapshot ORDER BY basename, byte_size, last_seen, deleted_time"
            )
        )


def has_snapshot_basename(peer: Path, basename: str) -> bool:
    return any(row[0] == basename for row in snapshot_rows(peer))


def make_unreadable(path: Path) -> int | None:
    if os.name == "nt":
        return None
    if hasattr(os, "geteuid") and os.geteuid() == 0:
        return None
    original = stat.S_IMODE(path.stat().st_mode)
    path.chmod(0)
    return original


def restore_mode(path: Path, mode: int | None) -> None:
    if mode is not None:
        path.chmod(mode)


def test_combined_tree_walk_and_order(tmp: Path, failures: list[str]) -> None:
    peer_a = tmp / "walk" / "peer_a"
    peer_b = tmp / "walk" / "peer_b"
    peer_sub = tmp / "walk" / "peer_sub"
    peer_a.mkdir(parents=True)
    peer_b.mkdir(parents=True)
    peer_sub.mkdir(parents=True)

    write_text(peer_a / "seed.txt", "seed\n")
    first = run_sync([f"+{peer_a}", peer_b, peer_sub], tmp)
    record(failures, "010 setup first sync", lambda: assert_success(first, "first sync"))
    if first.returncode != 0:
        return

    write_text(peer_a / "Alpha.txt", "from a\n")
    write_text(peer_a / "dir_from_a" / "a_child.txt", "child from a\n")
    write_text(peer_b / "beta.txt", "from b\n")
    write_text(peer_b / "DirFromB" / "b_child.txt", "child from b\n")
    write_text(peer_sub / "sub_only.txt", "remove from subordinate\n")
    write_text(peer_sub / "zz_subdir" / "old.txt", "remove dir from subordinate\n")

    second = run_sync(["--verbosity", "info", peer_a, peer_b, f"-{peer_sub}"], tmp)

    def check_result() -> None:
        assert_success(second, "combined-tree sync")
        lines = progress_lines(second)

        alpha = line_index(lines, "C Alpha.txt")
        beta = line_index(lines, "C beta.txt")
        sub_file = line_index(lines, "X sub_only.txt")
        sub_dir = line_index(lines, "X zz_subdir")
        a_child = line_index(lines, "C dir_from_a/a_child.txt")
        b_child = line_index(lines, "C DirFromB/b_child.txt")

        require(alpha < beta, f"root files were not processed in case-insensitive order: {lines!r}")
        require(beta < sub_file < sub_dir, f"root entries were not processed before recursion: {lines!r}")
        require(
            sub_dir < a_child and sub_dir < b_child,
            f"child directories were recursed before all root entries finished: {lines!r}",
        )

        for peer in (peer_a, peer_b, peer_sub):
            require(read_text(peer / "Alpha.txt") == "from a\n", f"{peer} is missing Alpha.txt")
            require(read_text(peer / "beta.txt") == "from b\n", f"{peer} is missing beta.txt")
            require(
                read_text(peer / "dir_from_a" / "a_child.txt") == "child from a\n",
                f"{peer} is missing dir_from_a/a_child.txt",
            )
            require(
                read_text(peer / "DirFromB" / "b_child.txt") == "child from b\n",
                f"{peer} is missing DirFromB/b_child.txt",
            )

        require(not (peer_sub / "sub_only.txt").exists(), "subordinate-only file was not displaced")
        require(not (peer_sub / "zz_subdir").exists(), "subordinate-only directory was not displaced")

    record(failures, "010.1-010.9 combined traversal", check_result)


def test_non_canon_listing_failure(tmp: Path, failures: list[str]) -> None:
    peer_a = tmp / "non_canon_failure" / "peer_a"
    peer_b = tmp / "non_canon_failure" / "peer_b"
    peer_a.mkdir(parents=True)
    peer_b.mkdir(parents=True)
    write_text(peer_a / "shared" / "before.txt", "before\n")

    first = run_sync([f"+{peer_a}", peer_b], tmp)
    record(failures, "010 non-canon failure setup", lambda: assert_success(first, "first sync"))
    if first.returncode != 0:
        return

    write_text(peer_a / "shared" / "new_from_a.txt", "new\n")
    marker = peer_b / "shared" / "local_marker.txt"
    write_text(marker, "must remain\n")
    original_rows = snapshot_rows(peer_b)
    mode = make_unreadable(peer_b / "shared")
    if mode is None:
        failures.append(
            "010.10-010.15 not reasonably testable: this host cannot make a "
            "local directory listing fail portably"
        )
        return

    try:
        result = run_sync(["--verbosity", "error", "--retries-list", "2", peer_a, peer_b], tmp)
    finally:
        restore_mode(peer_b / "shared", mode)

    def check_result() -> None:
        assert_success(result, "non-canon listing failure sync")
        lower_stdout = result.stdout.lower()
        require("listing" in lower_stdout, f"listing failure was not logged: {result.stdout!r}")
        require("shared" in result.stdout, f"failed path was not logged: {result.stdout!r}")
        require(marker.exists(), "failed peer subtree was modified after listing failure")
        require(
            not (peer_b / "shared" / "new_from_a.txt").exists(),
            "file was copied into a peer subtree after that peer listing failed",
        )
        require(
            not has_snapshot_basename(peer_b, "new_from_a.txt"),
            "failed peer snapshot gained a row below the failed subtree",
        )
        require(
            snapshot_rows(peer_b) == original_rows,
            "failed peer snapshot changed even though only its listed subtree was involved",
        )

    record(failures, "010.10-010.15 non-canon listing failure", check_result)


def test_canon_listing_failure_skips_all_peers(tmp: Path, failures: list[str]) -> None:
    peer_a = tmp / "canon_failure" / "peer_a"
    peer_b = tmp / "canon_failure" / "peer_b"
    peer_a.mkdir(parents=True)
    peer_b.mkdir(parents=True)
    write_text(peer_a / "shared" / "before.txt", "before\n")

    first = run_sync([f"+{peer_a}", peer_b], tmp)
    record(failures, "010 canon failure setup", lambda: assert_success(first, "first sync"))
    if first.returncode != 0:
        return

    write_text(peer_b / "shared" / "extra_on_b.txt", "must not be displaced\n")
    original_a_rows = snapshot_rows(peer_a)
    original_b_rows = snapshot_rows(peer_b)
    mode = make_unreadable(peer_a / "shared")
    if mode is None:
        failures.append(
            "010.16-010.17 not reasonably testable: this host cannot make a "
            "local canon directory listing fail portably"
        )
        return

    try:
        result = run_sync(["--verbosity", "error", "--retries-list", "1", f"+{peer_a}", peer_b], tmp)
    finally:
        restore_mode(peer_a / "shared", mode)

    def check_result() -> None:
        assert_success(result, "canon listing failure sync")
        lower_stdout = result.stdout.lower()
        require("listing" in lower_stdout, f"canon listing failure was not logged: {result.stdout!r}")
        require("shared" in result.stdout, f"canon failed path was not logged: {result.stdout!r}")
        require(
            (peer_b / "shared" / "extra_on_b.txt").exists(),
            "non-canon peer was modified under a canon failed subtree",
        )
        require(snapshot_rows(peer_a) == original_a_rows, "canon snapshot changed below failed subtree")
        require(snapshot_rows(peer_b) == original_b_rows, "other peer snapshot changed below canon failed subtree")

    record(failures, "010.16-010.17 canon listing failure", check_result)


def test_all_contributing_listing_failure(tmp: Path, failures: list[str]) -> None:
    peer_a = tmp / "all_failure" / "peer_a"
    peer_b = tmp / "all_failure" / "peer_b"
    peer_sub = tmp / "all_failure" / "peer_sub"
    peer_a.mkdir(parents=True)
    peer_b.mkdir(parents=True)
    peer_sub.mkdir(parents=True)
    write_text(peer_a / "shared" / "before.txt", "before\n")

    first = run_sync([f"+{peer_a}", peer_b, peer_sub], tmp)
    record(failures, "010 all contributing failure setup", lambda: assert_success(first, "first sync"))
    if first.returncode != 0:
        return

    write_text(peer_sub / "shared" / "subordinate_extra.txt", "must remain\n")
    mode_a = make_unreadable(peer_a / "shared")
    mode_b = make_unreadable(peer_b / "shared")
    if mode_a is None or mode_b is None:
        restore_mode(peer_a / "shared", mode_a)
        restore_mode(peer_b / "shared", mode_b)
        failures.append(
            "010.18-010.19 not reasonably testable: this host cannot make all "
            "local contributing directory listings fail portably"
        )
        return

    try:
        result = run_sync(["--verbosity", "error", "--retries-list", "1", peer_a, peer_b, f"-{peer_sub}"], tmp)
    finally:
        restore_mode(peer_a / "shared", mode_a)
        restore_mode(peer_b / "shared", mode_b)

    def check_result() -> None:
        assert_success(result, "all contributing listing failure sync")
        require(
            (peer_sub / "shared" / "subordinate_extra.txt").exists(),
            "subordinate file was displaced when all contributing peers failed listing",
        )

    record(failures, "010.18-010.19 all contributing listing failure", check_result)


def test_later_run_reincludes_failed_peer(tmp: Path, failures: list[str]) -> None:
    peer_a = tmp / "later_reinclude" / "peer_a"
    peer_b = tmp / "later_reinclude" / "peer_b"
    peer_a.mkdir(parents=True)
    peer_b.mkdir(parents=True)
    write_text(peer_a / "shared" / "before.txt", "before\n")

    first = run_sync([f"+{peer_a}", peer_b], tmp)
    record(failures, "010 later run setup", lambda: assert_success(first, "first sync"))
    if first.returncode != 0:
        return

    write_text(peer_a / "shared" / "after_failure.txt", "later\n")
    mode = make_unreadable(peer_b / "shared")
    if mode is None:
        failures.append(
            "010.20 not reasonably testable: this host cannot make a local "
            "directory listing fail and then succeed portably"
        )
        return

    try:
        failed_run = run_sync(["--verbosity", "error", "--retries-list", "1", peer_a, peer_b], tmp)
    finally:
        restore_mode(peer_b / "shared", mode)

    later_run = run_sync(["--verbosity", "error", peer_a, peer_b], tmp)

    def check_result() -> None:
        assert_success(failed_run, "listing failure run")
        require(
            not (peer_b / "shared" / "after_failure.txt").exists(),
            "failed run copied into the failed subtree",
        )
        assert_success(later_run, "later successful run")
        require(
            read_text(peer_b / "shared" / "after_failure.txt") == "later\n",
            "peer excluded by a previous listing failure did not participate on a later run",
        )

    record(failures, "010.20 later run after listing failure", check_result)


def main() -> int:
    failures: list[str] = []
    require(KITCHENSYNC.exists(), f"released executable does not exist: {KITCHENSYNC}")

    with tempfile.TemporaryDirectory(prefix="kitchensync-010-") as temp_name:
        tmp = Path(temp_name)
        test_combined_tree_walk_and_order(tmp, failures)
        test_non_canon_listing_failure(tmp, failures)
        test_canon_listing_failure_skips_all_peers(tmp, failures)
        test_all_contributing_listing_failure(tmp, failures)
        test_later_run_reincludes_failed_peer(tmp, failures)

    # not reasonably testable: 010.2 requires observing operation start ordering
    # inside concurrent directory listing operations, not just final process output.
    # not reasonably testable: 010.10 retry counts are not externally countable for
    # a local permission-denied listing without instrumenting the transport.
    # not reasonably testable: 010.21 requires forcing survival-evidence listing
    # failure inside directory conflict resolution without a specified test fixture.

    if failures:
        print("FAILURES:")
        for failure in failures:
            print(f"- {failure}")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
