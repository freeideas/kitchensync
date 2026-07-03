from __future__ import annotations

import re
import shutil
import subprocess
import sys
import tempfile
import os
from datetime import datetime, timezone
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
EXE = ROOT / "released" / "kitchensync.exe"
STAMP_PATTERN = re.compile(r"^\d{4}-\d{2}-\d{2}_\d{2}-\d{2}-\d{2}_\d{6}Z$")


def fail(message: str) -> None:
    raise AssertionError(message)


def require(condition: bool, message: str) -> None:
    if not condition:
        fail(message)


def run_kitchensync(args: list[str], cwd: Path) -> subprocess.CompletedProcess[str]:
    try:
        return subprocess.run(
            [str(EXE), *args],
            cwd=str(cwd),
            text=True,
            encoding="utf-8",
            errors="replace",
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=20,
            check=False,
        )
    except subprocess.TimeoutExpired as exc:
        fail(f"command timed out after {exc.timeout} seconds: {args!r}")


def set_mtime(path: Path, stamp: str) -> None:
    instant = datetime.strptime(stamp, "%Y-%m-%d_%H-%M-%S_%fZ")
    seconds = instant.replace(tzinfo=timezone.utc).timestamp()
    os_time = (seconds, seconds)
    path.touch()
    path.chmod(0o600)
    os.utime(path, os_time)


def write_file(path: Path, content: bytes, stamp: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(content)
    set_mtime(path, stamp)


def assert_completed(action: subprocess.CompletedProcess[str]) -> None:
    require(action.returncode == 0, f"action exit code was {action.returncode}")
    require(action.stdout == "sync complete\n", f"action stdout was {action.stdout!r}")
    require(action.stderr == "", f"action stderr was {action.stderr!r}")


def assert_bak_contains_deleted_file(peer_b: Path) -> None:
    bak = peer_b / ".kitchensync" / "BAK"
    require(bak.is_dir(), "B/.kitchensync/BAK/ does not exist")

    entries = sorted(bak.iterdir(), key=lambda entry: entry.name)
    require(len(entries) == 1, f"BAK entries were {[entry.name for entry in entries]!r}")

    stamp_dir = entries[0]
    require(stamp_dir.is_dir(), f"BAK entry is not a directory: {stamp_dir.name}")
    require(
        STAMP_PATTERN.match(stamp_dir.name) is not None,
        f"BAK directory is not timestamp-named: {stamp_dir.name!r}",
    )

    displaced = stamp_dir / "old.txt"
    require(displaced.is_file(), "BAK timestamp directory does not contain old.txt")
    require(displaced.read_bytes() == b"remove me\n", "BAK old.txt bytes differ")


def scenario() -> None:
    require(EXE.is_file(), f"missing released executable: {EXE}")

    temp_root = Path(tempfile.mkdtemp(prefix="kitchensync-S-05-"))
    try:
        peer_a = temp_root / "A"
        peer_b = temp_root / "B"
        shutil.rmtree(peer_a, ignore_errors=True)
        shutil.rmtree(peer_b, ignore_errors=True)
        peer_a.mkdir(parents=True)
        peer_b.mkdir(parents=True)

        write_file(
            peer_a / "old.txt",
            b"remove me\n",
            "2024-01-01_10-00-00_000000Z",
        )

        first = run_kitchensync(["--verbosity", "error", "+A", "B"], temp_root)
        require(first.returncode == 0, f"first sync exit code was {first.returncode}")

        (peer_a / "old.txt").unlink()

        action = run_kitchensync(["--verbosity", "error", "A", "B"], temp_root)
        assert_completed(action)

        require(not (peer_a / "old.txt").exists(), "A/old.txt still exists")
        require(not (peer_b / "old.txt").exists(), "B/old.txt still exists")
        assert_bak_contains_deleted_file(peer_b)
    finally:
        shutil.rmtree(temp_root, ignore_errors=True)


def main() -> int:
    try:
        scenario()
    except Exception as exc:
        print(f"S-05 failed: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
