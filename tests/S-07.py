from __future__ import annotations

import shutil
import subprocess
import sys
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
KITCHENSYNC = ROOT / "released" / "kitchensync.exe"
TIMEOUT_SECONDS = 20


def fail(message: str) -> None:
    raise AssertionError(message)


def write_file(path: Path, content: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(content)


def run_kitchensync(cwd: Path) -> subprocess.CompletedProcess[bytes]:
    try:
        return subprocess.run(
            [str(KITCHENSYNC), "--verbosity", "error", "+A", "B", "-x", "ignored"],
            cwd=cwd,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=TIMEOUT_SECONDS,
            check=False,
        )
    except subprocess.TimeoutExpired as exc:
        fail(f"kitchensync timed out after {TIMEOUT_SECONDS} seconds: {exc.cmd}")


def assert_no_ignored_bak(peer: Path, label: str) -> None:
    bak = peer / ".kitchensync" / "BAK"
    if not bak.exists():
        return

    for path in bak.rglob("*"):
        relative_parts = path.relative_to(bak).parts
        if "ignored" in relative_parts:
            fail(f"{label} displaced ignored/ entry to BAK: {path}")


def test_s_07() -> None:
    if not KITCHENSYNC.is_file():
        fail(f"missing released executable: {KITCHENSYNC}")

    temp_root = Path(tempfile.mkdtemp(prefix="kitchensync-S-07-"))
    try:
        peer_a = temp_root / "A"
        peer_b = temp_root / "B"
        shutil.rmtree(peer_a, ignore_errors=True)
        shutil.rmtree(peer_b, ignore_errors=True)

        write_file(peer_a / "keep.txt", b"copy\n")
        write_file(peer_a / "ignored" / "note.txt", b"do not copy\n")
        write_file(peer_b / "ignored" / "note.txt", b"leave alone\n")

        completed = run_kitchensync(temp_root)

        if completed.returncode != 0:
            fail(f"expected exit code 0, got {completed.returncode}")
        if completed.stdout != b"sync complete\n":
            fail(f"expected stdout b'sync complete\\n', got {completed.stdout!r}")
        if completed.stderr != b"":
            fail(f"expected empty stderr, got {completed.stderr!r}")

        if not (peer_b / "keep.txt").is_file():
            fail("B/keep.txt does not exist")
        if (peer_b / "keep.txt").read_bytes() != b"copy\n":
            fail("B/keep.txt bytes differ")
        if not (peer_b / "ignored" / "note.txt").is_file():
            fail("B/ignored/note.txt does not exist")
        if (peer_b / "ignored" / "note.txt").read_bytes() != b"leave alone\n":
            fail("B/ignored/note.txt bytes differ")

        assert_no_ignored_bak(peer_a, "A")
        assert_no_ignored_bak(peer_b, "B")
    finally:
        shutil.rmtree(temp_root, ignore_errors=True)


def main() -> int:
    try:
        test_s_07()
    except Exception as exc:
        print(f"S-07 failed: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
