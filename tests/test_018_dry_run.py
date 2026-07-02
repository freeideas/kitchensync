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
from pathlib import Path


sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")


LITERAL_WORKSPACE_ROOT = Path("/home/ace/Desktop/prjx/kitchensync")
WORKSPACE_ROOT = (
    LITERAL_WORKSPACE_ROOT
    if LITERAL_WORKSPACE_ROOT.exists()
    else Path(__file__).resolve().parents[1]
)
KITCHENSYNC_EXE = WORKSPACE_ROOT / "released" / "kitchensync.exe"


def file_url(path: Path) -> str:
    return path.resolve().as_uri()


def run_kitchensync(args: list[str], timeout: float = 20.0) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(KITCHENSYNC_EXE), *args],
        cwd=str(WORKSPACE_ROOT),
        text=True,
        encoding="utf-8",
        errors="replace",
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=timeout,
        check=False,
    )


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")


def make_snapshot_db(path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with sqlite3.connect(str(path)) as conn:
        conn.execute("PRAGMA journal_mode=DELETE")
        conn.execute(
            """
            CREATE TABLE snapshot (
                id TEXT PRIMARY KEY,
                parent_id TEXT NOT NULL,
                basename TEXT NOT NULL,
                mod_time TEXT NOT NULL,
                byte_size INTEGER NOT NULL,
                last_seen TEXT,
                deleted_time TEXT
            )
            """
        )
        conn.execute("CREATE INDEX snapshot_parent_id ON snapshot(parent_id)")
        conn.execute("CREATE INDEX snapshot_last_seen ON snapshot(last_seen)")
        conn.execute("CREATE INDEX snapshot_deleted_time ON snapshot(deleted_time)")
        conn.execute(
            """
            INSERT INTO snapshot
                (id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time)
            VALUES
                ('dryrun00001', 'root0000000', 'historic.txt',
                 '2024-01-01_00-00-00_000001Z', 8,
                 '2024-01-01_00-00-00_000002Z', NULL)
            """
        )


def tree_state(root: Path) -> dict[str, tuple[str, object]]:
    state: dict[str, tuple[str, object]] = {}
    for path in sorted(root.rglob("*")):
        rel = path.relative_to(root).as_posix()
        stat = path.stat()
        if path.is_dir():
            state[rel] = ("dir", stat.st_mtime_ns)
        else:
            state[rel] = ("file", path.read_bytes(), stat.st_mtime_ns)
    return state


def require(condition: bool, message: str, failures: list[str]) -> None:
    if not condition:
        failures.append(message)


def require_lines_contain(
    lines: list[str], prefix: str, relpath: str, failures: list[str]
) -> None:
    wanted = f"{prefix} {relpath}"
    require(
        wanted in lines,
        f"expected stdout progress line {wanted!r}; got {lines!r}",
        failures,
    )


def check_planning_is_read_only(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks-018-dry-run-") as tmp_name:
        tmp = Path(tmp_name)
        source = tmp / "source"
        dest = tmp / "dest"
        source.mkdir()
        dest.mkdir()

        write_text(source / "copy_me.txt", "copy source\n")
        write_text(source / "replace_me.txt", "replacement content\n")
        write_text(dest / "replace_me.txt", "old destination content\n")
        write_text(dest / "delete_me.txt", "canon does not have this\n")

        make_snapshot_db(source / ".kitchensync" / "snapshot.db")
        make_snapshot_db(dest / ".kitchensync" / "snapshot.db")
        write_text(dest / ".kitchensync" / "SWAP" / "snapshot.db" / "old", "old snapshot\n")
        write_text(dest / ".kitchensync" / "SWAP" / "snapshot.db" / "new", "new snapshot\n")
        write_text(dest / ".kitchensync" / "SWAP" / "delete_me.txt" / "old", "old swap\n")
        write_text(dest / ".kitchensync" / "TMP" / "1999-01-01_00-00-00_000001Z" / "tmp", "tmp\n")
        write_text(dest / ".kitchensync" / "BAK" / "1999-01-01_00-00-00_000001Z" / "old", "bak\n")

        before_source = tree_state(source)
        before_dest = tree_state(dest)

        result = run_kitchensync(
            [
                "--dry-run",
                "--verbosity",
                "trace",
                "--max-copies",
                "1",
                "--retries-copy",
                "1",
                f"+{file_url(source)}",
                file_url(dest),
            ]
        )

        lines = [line.strip() for line in result.stdout.splitlines() if line.strip()]
        progress_lines = [line for line in lines if len(line) > 2 and line[1] == " "]

        require(result.returncode == 0, f"dry-run planning exited {result.returncode}", failures)
        require(result.stderr == "", f"stderr should be empty, got {result.stderr!r}", failures)
        require("dry run" in result.stdout.lower(), "stdout should contain 'dry run'", failures)
        require_lines_contain(progress_lines, "C", "copy_me.txt", failures)
        require_lines_contain(progress_lines, "C", "replace_me.txt", failures)
        require_lines_contain(progress_lines, "X", "delete_me.txt", failures)
        require(
            "copy-slots active=" in result.stdout,
            "trace dry-run copy work should acquire copy slots",
            failures,
        )
        require(tree_state(source) == before_source, "dry-run changed the source peer tree", failures)
        require(tree_state(dest) == before_dest, "dry-run changed the destination peer tree", failures)


def check_missing_peer_root_is_not_created(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks-018-missing-root-") as tmp_name:
        tmp = Path(tmp_name)
        existing = tmp / "existing"
        missing = tmp / "missing-parent" / "missing-peer"
        existing.mkdir()
        write_text(existing / "file.txt", "content\n")

        result = run_kitchensync(["--dry-run", f"+{file_url(existing)}", file_url(missing)])

        require(result.returncode != 0, "dry-run with one missing peer root should fail", failures)
        require(result.stderr == "", f"stderr should be empty, got {result.stderr!r}", failures)
        require(not missing.exists(), "dry-run created a missing peer root", failures)
        require(
            not missing.parent.exists(),
            "dry-run created a missing peer root parent directory",
            failures,
        )


def check_snapshotless_reachable_peer_is_read_only(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks-018-snapshotless-") as tmp_name:
        tmp = Path(tmp_name)
        source = tmp / "source"
        empty_dest = tmp / "empty-dest"
        source.mkdir()
        empty_dest.mkdir()
        write_text(source / "new_file.txt", "new content\n")

        before_dest = tree_state(empty_dest)

        result = run_kitchensync(["--dry-run", f"+{file_url(source)}", file_url(empty_dest)])

        require(result.returncode == 0, f"snapshotless dry-run exited {result.returncode}", failures)
        require(result.stderr == "", f"stderr should be empty, got {result.stderr!r}", failures)
        require(
            tree_state(empty_dest) == before_dest,
            "dry-run created peer files or metadata for a snapshotless peer",
            failures,
        )


def main() -> int:
    failures: list[str] = []

    if not KITCHENSYNC_EXE.exists():
        failures.append(f"released executable does not exist: {KITCHENSYNC_EXE}")
    else:
        check_planning_is_read_only(failures)
        check_missing_peer_root_is_not_created(failures)
        check_snapshotless_reachable_peer_is_read_only(failures)

    # not reasonably testable: 018.6 local temporary snapshot database creation is
    # internal temp state; the peer-visible requirement is covered by preserving
    # the snapshotless peer.
    # not reasonably testable: 018.9 local temporary snapshot database updates are
    # internal temp state and are not uploaded in dry-run.
    # not reasonably testable: 018.12 source reads are not separately observable
    # from the released process without sabotaging the source during the run.
    # not reasonably testable: 018.13 retry counts require a deterministic copy
    # read failure, which is not available through the portable file peer surface.
    # not reasonably testable: sftp:// dry-run non-mutation is the same transport
    # contract as file:// for these bullets; this test covers file:// peers.

    if failures:
        print("FAIL")
        for failure in failures:
            print(f"- {failure}")
        return 1

    print("PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
