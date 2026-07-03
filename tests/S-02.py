from __future__ import annotations

import datetime
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile


REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
EXECUTABLE = REPO_ROOT / "released" / "kitchensync.exe"
CANON_BYTES = b"canon\n"
CANON_MTIME = datetime.datetime(
    2024, 1, 1, 12, 0, 0, tzinfo=datetime.timezone.utc
)
CANON_MTIME_NS = int(CANON_MTIME.timestamp() * 1_000_000_000)


def fail(message: str) -> None:
    raise AssertionError(message)


def require_equal(actual: object, expected: object, label: str) -> None:
    if actual != expected:
        fail(f"{label}: expected {expected!r}, got {actual!r}")


def require_exists(path: pathlib.Path, label: str) -> None:
    if not path.exists():
        fail(f"{label}: missing at {path}")


def require_no_user_files(path: pathlib.Path) -> None:
    user_entries = [entry.name for entry in path.iterdir() if entry.name != ".kitchensync"]
    if user_entries:
        fail(f"B should have no user files before sync, found {sorted(user_entries)!r}")


def write_file_with_mtime(path: pathlib.Path, content: bytes, mtime_ns: int) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(content)
    os.utime(path, ns=(mtime_ns, mtime_ns))


def assert_file_bytes_and_mtime(path: pathlib.Path) -> None:
    require_exists(path, "synced file")
    if not path.is_file():
        fail(f"synced file is not a regular file: {path}")
    require_equal(path.read_bytes(), CANON_BYTES, "synced file bytes")
    require_equal(path.stat().st_mtime_ns, CANON_MTIME_NS, "synced file mtime ns")


def run_scenario() -> None:
    if not EXECUTABLE.is_file():
        fail(f"missing released executable: {EXECUTABLE}")

    scratch_parent = pathlib.Path(tempfile.gettempdir()) / "kitchensync-S-02"
    if scratch_parent.exists():
        shutil.rmtree(scratch_parent)
    scratch_parent.mkdir(parents=True)

    with tempfile.TemporaryDirectory(dir=scratch_parent, prefix="run-") as temp_name:
        root = pathlib.Path(temp_name)
        peer_a = root / "A"
        peer_b = root / "B"
        write_file_with_mtime(peer_a / "album" / "one.txt", CANON_BYTES, CANON_MTIME_NS)
        peer_b.mkdir()

        require_no_user_files(peer_b)
        if (peer_a / ".kitchensync" / "snapshot.db").exists():
            fail("A unexpectedly has .kitchensync/snapshot.db before sync")
        if (peer_b / ".kitchensync" / "snapshot.db").exists():
            fail("B unexpectedly has .kitchensync/snapshot.db before sync")

        completed = subprocess.run(
            [str(EXECUTABLE), "--verbosity", "error", "+A", "B"],
            cwd=root,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=20,
            check=False,
        )

        require_equal(completed.returncode, 0, "exit code")
        require_equal(completed.stdout, b"sync complete\n", "stdout")
        require_equal(completed.stderr, b"", "stderr")
        assert_file_bytes_and_mtime(peer_b / "album" / "one.txt")
        require_exists(peer_a / ".kitchensync" / "snapshot.db", "A snapshot")
        require_exists(peer_b / ".kitchensync" / "snapshot.db", "B snapshot")


def main() -> int:
    try:
        run_scenario()
    except subprocess.TimeoutExpired as exc:
        print(f"FAIL: process timed out after {exc.timeout} seconds", file=sys.stderr)
        return 1
    except Exception as exc:
        print(f"FAIL: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
