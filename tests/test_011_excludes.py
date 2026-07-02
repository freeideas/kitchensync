# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

from __future__ import annotations

import sqlite3
import subprocess
import sys
import tempfile
from pathlib import Path


sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")


WORKSPACE_ROOT = Path("/home/ace/Desktop/prjx/kitchensync")
KITCHENSYNC_EXE = WORKSPACE_ROOT / "released" / "kitchensync.exe"


# not reasonably testable: 011.6
# The project testing guidelines say tests must not create symlinks.
# not reasonably testable: 011.7
# The project testing guidelines say tests must not create symlinks.
# not reasonably testable: 011.8
# Portable creation of special files is not available on both Windows and Linux.


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")


def run_sync(args: list[str], failures: list[str], label: str) -> subprocess.CompletedProcess[str] | None:
    try:
        completed = subprocess.run(
            [str(KITCHENSYNC_EXE), *args],
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
    except subprocess.TimeoutExpired as exc:
        failures.append(f"{label}: KitchenSync timed out after {exc.timeout} seconds")
        return None

    if completed.returncode != 0:
        failures.append(
            f"{label}: expected exit code 0, got {completed.returncode}; "
            f"stdout={completed.stdout!r}; stderr={completed.stderr!r}"
        )
    if completed.stderr != "":
        failures.append(f"{label}: expected empty stderr, got {completed.stderr!r}")
    if "sync complete" not in completed.stdout.splitlines():
        failures.append(f"{label}: stdout did not contain a complete sync line: {completed.stdout!r}")
    return completed


def expect(condition: bool, failures: list[str], message: str) -> None:
    if not condition:
        failures.append(message)


def read_text_or_none(path: Path, failures: list[str], label: str) -> str | None:
    try:
        return path.read_text(encoding="utf-8")
    except OSError as exc:
        failures.append(f"{label}: could not read {path}: {exc}")
        return None


def snapshot_rows(peer: Path, basename: str, failures: list[str], label: str) -> list[tuple[str, int, str | None, str | None]]:
    db_path = peer / ".kitchensync" / "snapshot.db"
    if not db_path.exists():
        failures.append(f"{label}: missing snapshot database at {db_path}")
        return []

    try:
        with sqlite3.connect(str(db_path)) as db:
            rows = db.execute(
                """
                SELECT mod_time, byte_size, last_seen, deleted_time
                FROM snapshot
                WHERE basename = ?
                ORDER BY id
                """,
                (basename,),
            ).fetchall()
    except sqlite3.Error as exc:
        failures.append(f"{label}: could not read snapshot database {db_path}: {exc}")
        return []

    return [(str(row[0]), int(row[1]), row[2], row[3]) for row in rows]


def expect_no_snapshot_basename(peer: Path, basename: str, failures: list[str], label: str) -> None:
    rows = snapshot_rows(peer, basename, failures, label)
    expect(rows == [], failures, f"{label}: excluded basename {basename!r} was written to the snapshot: {rows!r}")


def command_line_excludes_case(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks-011-cli-") as tmp_name:
        tmp = Path(tmp_name)
        canon = tmp / "canon"
        other = tmp / "other"
        canon.mkdir()
        other.mkdir()

        write_text(canon / "include.txt", "included from canon\n")
        write_text(canon / "skip-file.txt", "excluded file from canon\n")
        write_text(canon / "skip-dir" / "skip-nested.txt", "excluded directory child\n")
        write_text(other / "keep-file.txt", "excluded preexisting file on other\n")
        write_text(other / "keep-dir" / "keep-nested.txt", "excluded preexisting directory child\n")
        write_text(canon / "type-conflict", "excluded canon file must not displace other directory\n")
        write_text(other / "type-conflict" / "conflict-child.txt", "other directory survives\n")

        run_sync(
            [
                "+" + str(canon),
                str(other),
                "-x",
                "skip-file.txt",
                "-x",
                "skip-dir",
                "-x",
                "keep-file.txt",
                "-x",
                "keep-dir",
                "-x",
                "type-conflict",
            ],
            failures,
            "command-line excludes",
        )

        expect(read_text_or_none(other / "include.txt", failures, "011.1") == "included from canon\n", failures, "011.1: non-excluded file was not copied")
        expect((canon / "skip-file.txt").is_file(), failures, "011.2: excluded source file was not left in place")
        expect(not (other / "skip-file.txt").exists(), failures, "011.2/011.12: excluded file was copied to another peer")
        expect((canon / "skip-dir" / "skip-nested.txt").is_file(), failures, "011.3: excluded source directory child was not left in place")
        expect(not (other / "skip-dir").exists(), failures, "011.3/011.11/011.12: excluded directory was copied or recursed into")
        expect(read_text_or_none(other / "keep-file.txt", failures, "011.13") == "excluded preexisting file on other\n", failures, "011.13: excluded preexisting file was deleted")
        expect(read_text_or_none(other / "keep-dir" / "keep-nested.txt", failures, "011.13") == "excluded preexisting directory child\n", failures, "011.13: excluded preexisting directory was deleted")
        expect((other / "type-conflict").is_dir(), failures, "011.14: excluded conflicting directory was displaced")
        expect((other / "type-conflict" / "conflict-child.txt").is_file(), failures, "011.14: excluded conflicting directory contents were displaced")

        for peer, peer_label in ((canon, "canon"), (other, "other")):
            for basename in (
                "skip-file.txt",
                "skip-dir",
                "skip-nested.txt",
                "keep-file.txt",
                "keep-dir",
                "keep-nested.txt",
                "type-conflict",
                "conflict-child.txt",
            ):
                expect_no_snapshot_basename(peer, basename, failures, f"011.10/011.16 command-line excludes {peer_label}")


def built_in_excludes_case(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks-011-builtins-") as tmp_name:
        tmp = Path(tmp_name)
        canon = tmp / "canon"
        other = tmp / "other"
        canon.mkdir()
        other.mkdir()

        write_text(canon / "ordinary.txt", "ordinary file\n")
        write_text(canon / ".git" / "canon-git.txt", "canon git metadata\n")
        write_text(other / ".git" / "other-git.txt", "other git metadata\n")
        write_text(canon / ".kitchensync" / "canon-meta.txt", "canon kitchensync metadata\n")
        write_text(other / ".kitchensync" / "other-meta.txt", "other kitchensync metadata\n")

        run_sync(
            ["+" + str(canon), str(other), "-x", "not-present-command-exclude.txt"],
            failures,
            "built-in excludes",
        )

        expect(read_text_or_none(other / "ordinary.txt", failures, "built-in excludes") == "ordinary file\n", failures, "built-in excludes: ordinary file was not copied")
        expect(not (other / ".git" / "canon-git.txt").exists(), failures, "011.5/011.9/011.12: .git content was copied")
        expect(read_text_or_none(other / ".git" / "other-git.txt", failures, "011.5") == "other git metadata\n", failures, "011.5/011.13: existing .git content was deleted")
        expect(not (other / ".kitchensync" / "canon-meta.txt").exists(), failures, "011.4/011.9/011.12: .kitchensync user content was copied")
        expect(read_text_or_none(other / ".kitchensync" / "other-meta.txt", failures, "011.4") == "other kitchensync metadata\n", failures, "011.4/011.13: existing .kitchensync content was deleted")

        for peer, peer_label in ((canon, "canon"), (other, "other")):
            for basename in ("canon-git.txt", "other-git.txt", "canon-meta.txt", "other-meta.txt"):
                expect_no_snapshot_basename(peer, basename, failures, f"011.4/011.5/011.16 built-ins {peer_label}")


def snapshot_not_consulted_or_updated_case(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks-011-snapshot-") as tmp_name:
        tmp = Path(tmp_name)
        canon = tmp / "canon"
        other = tmp / "other"
        canon.mkdir()
        other.mkdir()

        target = "snapshot-target.txt"
        write_text(canon / target, "initial content\n")

        run_sync(["+" + str(canon), str(other)], failures, "snapshot setup")
        before_canon = snapshot_rows(canon, target, failures, "snapshot setup canon")
        before_other = snapshot_rows(other, target, failures, "snapshot setup other")
        expect(len(before_canon) == 1, failures, f"snapshot setup: expected one canon row for {target}, got {before_canon!r}")
        expect(len(before_other) == 1, failures, f"snapshot setup: expected one other row for {target}, got {before_other!r}")

        (canon / target).unlink()
        write_text(other / target, "other peer keeps excluded content\n")

        run_sync(["+" + str(canon), str(other), "-x", target], failures, "snapshot excluded run")
        after_canon = snapshot_rows(canon, target, failures, "snapshot excluded canon")
        after_other = snapshot_rows(other, target, failures, "snapshot excluded other")

        expect(not (canon / target).exists(), failures, "011.13: excluded absent canon file was recreated from another peer")
        expect(read_text_or_none(other / target, failures, "011.15") == "other peer keeps excluded content\n", failures, "011.15: excluded path snapshot was consulted and changed peer content")
        expect(after_canon == before_canon, failures, f"011.16: excluded canon snapshot row changed from {before_canon!r} to {after_canon!r}")
        expect(after_other == before_other, failures, f"011.16: excluded other snapshot row changed from {before_other!r} to {after_other!r}")


def main() -> int:
    failures: list[str] = []

    if not KITCHENSYNC_EXE.exists():
        failures.append(f"released executable does not exist: {KITCHENSYNC_EXE}")
    else:
        command_line_excludes_case(failures)
        built_in_excludes_case(failures)
        snapshot_not_consulted_or_updated_case(failures)

    if failures:
        print("FAIL")
        for failure in failures:
            print(f"- {failure}")
        return 1

    print("PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
