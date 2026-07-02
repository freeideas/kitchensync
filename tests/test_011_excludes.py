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
from pathlib import Path


sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")


WORKSPACE_ROOT = Path("/home/ace/Desktop/prjx/kitchensync")
PRIMARY_EXE = WORKSPACE_ROOT / "released" / "kitchensync.exe"
FALLBACK_EXE = Path(__file__).resolve().parents[1] / "released" / "kitchensync.exe"


def released_exe() -> Path:
    if PRIMARY_EXE.exists():
        return PRIMARY_EXE
    return FALLBACK_EXE


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")


def read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def run_sync(args: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(released_exe()), *args],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=30,
        check=False,
    )


def add_failure(failures: list[str], condition: bool, message: str) -> None:
    if not condition:
        failures.append(message)


def assert_success(failures: list[str], result: subprocess.CompletedProcess[str], label: str) -> None:
    add_failure(
        failures,
        result.returncode == 0,
        f"{label}: expected exit 0, got {result.returncode}; stdout={result.stdout!r}; stderr={result.stderr!r}",
    )
    add_failure(failures, result.stderr == "", f"{label}: expected empty stderr, got {result.stderr!r}")


def action_lines(output: str) -> set[str]:
    return {
        line.strip()
        for line in output.splitlines()
        if line.startswith("C ") or line.startswith("X ")
    }


def snapshot_rows(peer: Path, basename: str) -> list[tuple[object, ...]]:
    db_path = peer / ".kitchensync" / "snapshot.db"
    if not db_path.exists():
        return []
    connection = sqlite3.connect(str(db_path))
    try:
        return connection.execute(
            """
            SELECT basename, mod_time, byte_size, last_seen, deleted_time
            FROM snapshot
            WHERE basename = ?
            ORDER BY id
            """,
            (basename,),
        ).fetchall()
    finally:
        connection.close()


def test_command_line_and_builtin_excludes(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks-011-excludes-") as tmp_name:
        root = Path(tmp_name)
        peer_a = root / "peer-a"
        peer_b = root / "peer-b"
        peer_a.mkdir()
        peer_b.mkdir()

        write_text(peer_a / "copy-me.txt", "copy me\n")
        write_text(peer_a / "skip-file.txt", "canon skip file\n")
        write_text(peer_a / "skip-dir" / "inside.txt", "canon skip directory\n")
        write_text(peer_a / "blocked" / "inside.txt", "canon directory should not displace peer file\n")
        write_text(peer_a / ".git" / "source-only.txt", "git metadata from canon\n")
        write_text(peer_a / ".kitchensync" / "source-only.txt", "sync metadata from canon\n")

        write_text(peer_b / "skip-file.txt", "peer file must remain\n")
        write_text(peer_b / "skip-dir" / "inside.txt", "peer directory must remain\n")
        write_text(peer_b / "blocked", "peer file must not be displaced\n")
        write_text(peer_b / ".git" / "peer-only.txt", "peer git metadata must remain\n")
        write_text(peer_b / ".kitchensync" / "peer-only.txt", "peer sync metadata must remain\n")

        result = run_sync(
            [
                f"+{peer_a}",
                str(peer_b),
                "-x",
                "skip-file.txt",
                "-x",
                "skip-dir",
                "-x",
                "blocked",
            ]
        )
        assert_success(failures, result, "command-line and built-in excludes")

        actions = action_lines(result.stdout)
        add_failure(
            failures,
            (peer_b / "copy-me.txt").exists() and read_text(peer_b / "copy-me.txt") == "copy me\n",
            "011.1: a non-excluded file should still be copied during the run",
        )
        add_failure(
            failures,
            read_text(peer_b / "skip-file.txt") == "peer file must remain\n",
            "011.1/011.2/011.13/011.14: excluded file path should not be copied over or deleted",
        )
        add_failure(
            failures,
            not (peer_b / "skip-file.txt" / "inside.txt").exists(),
            "011.2: a file exclude should apply only to that file path, not an invented subtree",
        )
        add_failure(
            failures,
            read_text(peer_b / "skip-dir" / "inside.txt") == "peer directory must remain\n",
            "011.1/011.3/011.12/011.13/011.14: excluded directory descendants should not be copied or deleted",
        )
        add_failure(
            failures,
            (peer_b / "blocked").is_file() and read_text(peer_b / "blocked") == "peer file must not be displaced\n",
            "011.10/011.15: an excluded type-conflict path should be omitted from decisions and not displaced",
        )
        add_failure(
            failures,
            not (peer_b / ".git" / "source-only.txt").exists()
            and (peer_b / ".git" / "peer-only.txt").exists(),
            "011.5/011.9/011.13/011.14: .git directories should stay excluded even when -x is supplied",
        )
        add_failure(
            failures,
            not (peer_b / ".kitchensync" / "source-only.txt").exists()
            and (peer_b / ".kitchensync" / "peer-only.txt").exists(),
            "011.4/011.9/011.13/011.14: .kitchensync directories should stay excluded even when -x is supplied",
        )
        for relpath in ("skip-file.txt", "skip-dir", "skip-dir/inside.txt", "blocked"):
            add_failure(
                failures,
                f"C {relpath}" not in actions and f"X {relpath}" not in actions,
                f"011.10/011.13/011.14/011.15: excluded path {relpath!r} should not have progress actions",
            )


def test_excluded_snapshot_rows_are_not_consulted_or_updated(failures: list[str]) -> None:
    tracked_name = "tracked_unique_011.txt"
    with tempfile.TemporaryDirectory(prefix="ks-011-snapshot-") as tmp_name:
        root = Path(tmp_name)
        peer_a = root / "peer-a"
        peer_b = root / "peer-b"
        peer_a.mkdir()
        peer_b.mkdir()

        write_text(peer_a / tracked_name, "original\n")
        initial = run_sync([f"+{peer_a}", str(peer_b)])
        assert_success(failures, initial, "initial snapshot setup")
        add_failure(
            failures,
            (peer_b / tracked_name).exists(),
            "snapshot setup: tracked file should copy before the exclude check",
        )

        before_a = snapshot_rows(peer_a, tracked_name)
        before_b = snapshot_rows(peer_b, tracked_name)
        add_failure(failures, len(before_a) == 1, "snapshot setup: peer A should have one tracked snapshot row")
        add_failure(failures, len(before_b) == 1, "snapshot setup: peer B should have one tracked snapshot row")

        os.remove(peer_a / tracked_name)
        excluded = run_sync([f"+{peer_a}", str(peer_b), "-x", tracked_name])
        assert_success(failures, excluded, "excluded snapshot run")

        after_a = snapshot_rows(peer_a, tracked_name)
        after_b = snapshot_rows(peer_b, tracked_name)
        add_failure(
            failures,
            (peer_b / tracked_name).exists() and read_text(peer_b / tracked_name) == "original\n",
            "011.10/011.14/011.16: excluded path with snapshot history should not be deleted by canon absence",
        )
        add_failure(
            failures,
            before_a == after_a and before_b == after_b,
            "011.17: snapshot rows for an excluded path should not be updated during the run",
        )
        add_failure(
            failures,
            f"X {tracked_name}" not in action_lines(excluded.stdout),
            "011.16: excluded snapshot history should not produce a delete action",
        )


def main() -> int:
    failures: list[str] = []

    if not released_exe().exists():
        failures.append(f"released executable does not exist: {released_exe()}")
    else:
        test_command_line_and_builtin_excludes(failures)
        test_excluded_snapshot_rows_are_not_consulted_or_updated(failures)

    # not reasonably testable: 011.6
    # not reasonably testable: 011.7
    # not reasonably testable: 011.8
    # not reasonably testable: 011.11

    if failures:
        print("FAIL")
        for failure in failures:
            print(f"- {failure}")
        return 1

    print("PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
