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
KITCHENSYNC = ROOT / "released" / "kitchensync.exe"
TIMEOUT_SECONDS = 20
GROUP_MTIME = "2024-01-01_10-00-00_000000Z"
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
            [str(KITCHENSYNC), *args],
            cwd=str(cwd),
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=TIMEOUT_SECONDS,
            check=False,
        )
    except subprocess.TimeoutExpired as exc:
        fail(f"kitchensync timed out after {TIMEOUT_SECONDS} seconds: {exc.cmd}")


def assert_completed(completed: subprocess.CompletedProcess[bytes]) -> None:
    if completed.returncode != 0:
        fail(f"expected exit 0, got {completed.returncode}")
    if completed.stdout != b"sync complete\n":
        fail(f"expected stdout b'sync complete\\n', got {completed.stdout!r}")
    if completed.stderr != b"":
        fail(f"expected empty stderr, got {completed.stderr!r}")


def assert_file(path: Path, expected_content: bytes, expected_mtime_ns: int) -> None:
    if not path.is_file():
        fail(f"missing file: {path}")
    actual_content = path.read_bytes()
    if actual_content != expected_content:
        fail(f"{path} contained {actual_content!r}, expected {expected_content!r}")
    actual_mtime_ns = path.stat().st_mtime_ns
    if actual_mtime_ns != expected_mtime_ns:
        fail(f"{path} mtime was {actual_mtime_ns}, expected {expected_mtime_ns}")


def assert_c_backup(peer_c: Path) -> None:
    bak = peer_c / ".kitchensync" / "BAK"
    if not bak.is_dir():
        fail("C/.kitchensync/BAK does not exist")

    stamp_dirs = sorted(path for path in bak.iterdir() if path.is_dir())
    names = [path.name for path in stamp_dirs]
    if len(stamp_dirs) != 1:
        fail(f"expected one C backup timestamp directory, got {names!r}")
    if STAMP_PATTERN.fullmatch(stamp_dirs[0].name) is None:
        fail(f"C backup directory is not timestamp-named: {stamp_dirs[0].name!r}")

    files = sorted(path for path in stamp_dirs[0].rglob("*") if path.is_file())
    relative_files = [path.relative_to(stamp_dirs[0]).as_posix() for path in files]
    if relative_files != ["extra.txt", "shared.txt"]:
        fail(f"C backup files were {relative_files!r}")

    expected = {
        "extra.txt": b"extra\n",
        "shared.txt": b"wrong\n",
    }
    for relative_path, expected_content in expected.items():
        actual_content = (stamp_dirs[0] / relative_path).read_bytes()
        if actual_content != expected_content:
            fail(
                f"C backup {relative_path} contained "
                f"{actual_content!r}, expected {expected_content!r}"
            )


def test_s_06() -> None:
    if not KITCHENSYNC.is_file():
        fail(f"missing released executable: {KITCHENSYNC}")

    group_mtime_ns = scenario_time_to_ns(GROUP_MTIME)
    temp_root = Path(tempfile.mkdtemp(prefix="kitchensync-s-06-"))
    try:
        peer_a = temp_root / "A"
        peer_b = temp_root / "B"
        peer_c = temp_root / "C"
        shutil.rmtree(peer_a, ignore_errors=True)
        shutil.rmtree(peer_b, ignore_errors=True)
        shutil.rmtree(peer_c, ignore_errors=True)
        peer_a.mkdir(parents=True)
        peer_b.mkdir(parents=True)
        peer_c.mkdir(parents=True)

        write_file(peer_a / "shared.txt", b"group\n", group_mtime_ns)

        first = run_kitchensync(["--verbosity", "error", "+A", "B"], temp_root)
        if first.returncode != 0:
            fail(
                "first sync exit code was "
                f"{first.returncode}, stdout {first.stdout!r}, stderr {first.stderr!r}"
            )

        write_file(peer_c / "shared.txt", b"wrong\n")
        write_file(peer_c / "extra.txt", b"extra\n")
        shutil.rmtree(peer_c / ".kitchensync", ignore_errors=True)

        action = run_kitchensync(["--verbosity", "error", "A", "B", "-C"], temp_root)
        assert_completed(action)

        assert_file(peer_c / "shared.txt", b"group\n", group_mtime_ns)
        if (peer_c / "extra.txt").exists():
            fail("C/extra.txt still exists")
        assert_c_backup(peer_c)
    finally:
        shutil.rmtree(temp_root, ignore_errors=True)


def main() -> int:
    try:
        test_s_06()
    except Exception as exc:
        print(f"S-06 failed: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
