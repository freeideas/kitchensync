from __future__ import annotations

import os
import shutil
import subprocess
import sys
import tempfile
from datetime import datetime, timezone
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
KITCHENSYNC = ROOT / "released" / "kitchensync.exe"
TIMEOUT_SECONDS = 20


OLD_MTIME = "2024-01-01_10-00-00_000000Z"
NEW_MTIME = "2024-01-02_10-00-00_000000Z"


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


def run_kitchensync(args: list[str]) -> subprocess.CompletedProcess[bytes]:
    try:
        return subprocess.run(
            [str(KITCHENSYNC), *args],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=TIMEOUT_SECONDS,
            check=False,
        )
    except subprocess.TimeoutExpired as exc:
        fail(f"kitchensync timed out after {TIMEOUT_SECONDS} seconds: {exc.cmd}")


def assert_completed(
    completed: subprocess.CompletedProcess[bytes],
    expected_code: int,
    expected_stdout: bytes | None = None,
    expected_stderr: bytes | None = None,
) -> None:
    if completed.returncode != expected_code:
        fail(f"expected exit {expected_code}, got {completed.returncode}")
    if expected_stdout is not None and completed.stdout != expected_stdout:
        fail(f"expected stdout {expected_stdout!r}, got {completed.stdout!r}")
    if expected_stderr is not None and completed.stderr != expected_stderr:
        fail(f"expected stderr {expected_stderr!r}, got {completed.stderr!r}")


def assert_file(path: Path, expected_content: bytes, expected_mtime_ns: int) -> None:
    if path.read_bytes() != expected_content:
        fail(f"{path} did not contain {expected_content!r}")
    actual_mtime_ns = path.stat().st_mtime_ns
    if actual_mtime_ns != expected_mtime_ns:
        fail(f"{path} mtime was {actual_mtime_ns}, expected {expected_mtime_ns}")


def test_s_04() -> None:
    if not KITCHENSYNC.is_file():
        fail(f"missing released executable: {KITCHENSYNC}")

    old_mtime_ns = scenario_time_to_ns(OLD_MTIME)
    new_mtime_ns = scenario_time_to_ns(NEW_MTIME)

    temp_root = Path(tempfile.mkdtemp(prefix="kitchensync-s-04-"))
    try:
        peer_a = temp_root / "A"
        peer_b = temp_root / "B"
        if peer_a.exists():
            shutil.rmtree(peer_a)
        if peer_b.exists():
            shutil.rmtree(peer_b)
        peer_a.mkdir(parents=True)
        peer_b.mkdir(parents=True)

        write_file(peer_a / "report.txt", b"old\n", old_mtime_ns)

        first = run_kitchensync(
            ["--verbosity", "error", f"+{peer_a}", str(peer_b)]
        )
        assert_completed(first, 0)

        write_file(peer_b / "report.txt", b"new\n", new_mtime_ns)

        second = run_kitchensync(
            ["--verbosity", "error", str(peer_a), str(peer_b)]
        )
        assert_completed(second, 0, b"sync complete\n", b"")

        assert_file(peer_a / "report.txt", b"new\n", new_mtime_ns)
        assert_file(peer_b / "report.txt", b"new\n", new_mtime_ns)
    finally:
        shutil.rmtree(temp_root, ignore_errors=True)


def main() -> int:
    try:
        test_s_04()
    except Exception as exc:
        print(f"FAIL: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
