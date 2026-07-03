from __future__ import annotations

import os
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


def fail(message: str) -> None:
    raise AssertionError(message)


def scenario_time_to_ns(value: str) -> int:
    parsed = datetime.strptime(value, "%Y-%m-%d_%H-%M-%S_%fZ")
    parsed = parsed.replace(tzinfo=timezone.utc)
    return int(parsed.timestamp() * 1_000_000_000)


def write_file(path: Path, content: bytes, mtime_ns: int) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(content)
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
    except subprocess.TimeoutExpired:
        fail(f"kitchensync timed out after {TIMEOUT_SECONDS} seconds")


def assert_completed(completed: subprocess.CompletedProcess[bytes]) -> None:
    if completed.returncode != 0:
        fail(f"expected exit code 0, got {completed.returncode}")
    if completed.stdout != b"C note.txt\nsync complete\n":
        fail(
            "expected stdout b'C note.txt\\nsync complete\\n', "
            f"got {completed.stdout!r}"
        )
    if completed.stderr != b"":
        fail(f"expected empty stderr, got {completed.stderr!r}")


def assert_copied_note(peer_b: Path, expected_mtime_ns: int) -> None:
    note = peer_b / "note.txt"
    if not note.is_file():
        fail("B/note.txt does not exist")
    if note.read_bytes() != b"copy me\n":
        fail("B/note.txt bytes did not match b'copy me\\n'")
    actual_mtime_ns = note.stat().st_mtime_ns
    if actual_mtime_ns != expected_mtime_ns:
        fail(f"B/note.txt mtime was {actual_mtime_ns}, expected {expected_mtime_ns}")


def scenario() -> None:
    if not EXE.is_file():
        fail(f"missing released executable: {EXE}")

    mtime_ns = scenario_time_to_ns(SCENARIO_MTIME)
    temp_root = Path(tempfile.mkdtemp(prefix="kitchensync-S-11-"))
    try:
        peer_a = temp_root / "A"
        peer_b = temp_root / "B"
        shutil.rmtree(peer_a, ignore_errors=True)
        shutil.rmtree(peer_b, ignore_errors=True)
        peer_a.mkdir(parents=True)
        peer_b.mkdir(parents=True)

        write_file(peer_a / "note.txt", b"copy me\n", mtime_ns)

        completed = run_kitchensync(["--verbosity", "info", "+A", "B"], temp_root)
        assert_completed(completed)
        assert_copied_note(peer_b, mtime_ns)
    finally:
        shutil.rmtree(temp_root, ignore_errors=True)


def main() -> int:
    try:
        scenario()
    except Exception as exc:
        print(f"S-11 failed: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
