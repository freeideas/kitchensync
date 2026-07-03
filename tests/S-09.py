from __future__ import annotations

import os
import re
import shutil
import subprocess
import sys
import tempfile
from datetime import datetime, timezone
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
EXE = ROOT / "released" / "kitchensync.exe"
TIMEOUT_SECONDS = 20
SCENARIO_MTIME = "2024-01-01_10-00-00_000000Z"
STAMP_PATTERN = re.compile(r"^\d{4}-\d{2}-\d{2}_\d{2}-\d{2}-\d{2}_\d{6}Z$")


def fail(message: str) -> None:
    raise AssertionError(message)


def scenario_time_to_ns(value: str) -> int:
    parsed = datetime.strptime(value, "%Y-%m-%d_%H-%M-%S_%fZ")
    parsed = parsed.replace(tzinfo=timezone.utc)
    return int(parsed.timestamp() * 1_000_000_000)


def write_file(path: Path, content: bytes, mtime_ns: int | None = None) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(content)
    if mtime_ns is not None:
        os.utime(path, ns=(mtime_ns, mtime_ns))


def run_kitchensync(args: list[str], cwd: Path) -> subprocess.CompletedProcess[bytes]:
    try:
        return subprocess.run(
            [str(EXE), *args],
            cwd=str(cwd),
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=TIMEOUT_SECONDS,
            check=False,
        )
    except subprocess.TimeoutExpired as exc:
        fail(f"kitchensync timed out after {TIMEOUT_SECONDS} seconds: {exc.cmd!r}")


def assert_completed(completed: subprocess.CompletedProcess[bytes]) -> None:
    if completed.returncode != 0:
        fail(f"expected exit code 0, got {completed.returncode}")
    if completed.stdout != b"sync complete\n":
        fail(f"expected stdout b'sync complete\\n', got {completed.stdout!r}")
    if completed.stderr != b"":
        fail(f"expected empty stderr, got {completed.stderr!r}")


def assert_b_item(peer_b: Path, expected_mtime_ns: int) -> None:
    item = peer_b / "item"
    if not item.is_file():
        fail("B/item is not a file")
    if item.read_bytes() != b"file wins\n":
        fail("B/item bytes did not match b'file wins\\n'")
    actual_mtime_ns = item.stat().st_mtime_ns
    if actual_mtime_ns != expected_mtime_ns:
        fail(f"B/item mtime was {actual_mtime_ns}, expected {expected_mtime_ns}")


def assert_displaced_directory_backup(peer_b: Path) -> None:
    bak = peer_b / ".kitchensync" / "BAK"
    if not bak.is_dir():
        fail("B/.kitchensync/BAK/ does not exist")

    entries = sorted(bak.iterdir(), key=lambda entry: entry.name)
    if len(entries) != 1:
        fail(f"expected exactly one BAK entry, got {[entry.name for entry in entries]!r}")

    stamp_dir = entries[0]
    if not stamp_dir.is_dir():
        fail(f"BAK entry is not a directory: {stamp_dir.name}")
    if STAMP_PATTERN.match(stamp_dir.name) is None:
        fail(f"BAK directory is not timestamp-named: {stamp_dir.name!r}")

    displaced_item = stamp_dir / "item"
    if not displaced_item.is_dir():
        fail("BAK timestamp directory does not contain displaced item/")

    nested = displaced_item / "nested.txt"
    if not nested.is_file():
        fail("BAK displaced item/ does not contain nested.txt")
    if nested.read_bytes() != b"directory loses\n":
        fail("BAK item/nested.txt bytes did not match b'directory loses\\n'")


def scenario() -> None:
    if not EXE.is_file():
        fail(f"missing released executable: {EXE}")

    mtime_ns = scenario_time_to_ns(SCENARIO_MTIME)
    temp_root = Path(tempfile.mkdtemp(prefix="kitchensync-S-09-"))
    try:
        peer_a = temp_root / "A"
        peer_b = temp_root / "B"
        shutil.rmtree(peer_a, ignore_errors=True)
        shutil.rmtree(peer_b, ignore_errors=True)
        peer_a.mkdir(parents=True)
        peer_b.mkdir(parents=True)

        write_file(peer_a / "item", b"file wins\n", mtime_ns)
        write_file(peer_b / "item" / "nested.txt", b"directory loses\n")

        completed = run_kitchensync(["--verbosity", "error", "+A", "B"], temp_root)
        assert_completed(completed)
        assert_b_item(peer_b, mtime_ns)
        assert_displaced_directory_backup(peer_b)
    finally:
        shutil.rmtree(temp_root, ignore_errors=True)


def main() -> int:
    try:
        scenario()
    except Exception as exc:
        print(f"S-09 failed: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
