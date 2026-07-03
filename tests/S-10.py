from __future__ import annotations

import datetime
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
KITCHENSYNC = ROOT / "released" / "kitchensync.exe"
TIMEOUT_SECONDS = 20
GROUP_MTIME = datetime.datetime(
    2024, 1, 1, 10, 0, 0, tzinfo=datetime.timezone.utc
)
GROUP_MTIME_NS = int(GROUP_MTIME.timestamp() * 1_000_000_000)


def fail(message: str) -> None:
    raise AssertionError(message)


def require(condition: bool, message: str) -> None:
    if not condition:
        fail(message)


def write_file(path: Path, content: bytes, mtime_ns: int | None = None) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(content)
    if mtime_ns is not None:
        os.utime(path, ns=(mtime_ns, mtime_ns))


def run_kitchensync(args: list[str], cwd: Path) -> subprocess.CompletedProcess[bytes]:
    try:
        return subprocess.run(
            [str(KITCHENSYNC), "--verbosity", "error", *args],
            cwd=cwd,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=TIMEOUT_SECONDS,
            check=False,
        )
    except subprocess.TimeoutExpired as exc:
        fail(f"kitchensync timed out after {TIMEOUT_SECONDS} seconds: {exc.cmd}")


def assert_completed(result: subprocess.CompletedProcess[bytes]) -> None:
    require(result.returncode == 0, f"exit code was {result.returncode}")
    require(result.stdout == b"sync complete\n", f"stdout was {result.stdout!r}")
    require(result.stderr == b"", f"stderr was {result.stderr!r}")


def assert_c_bak(peer_c: Path) -> None:
    bak = peer_c / ".kitchensync" / "BAK"
    require(bak.is_dir(), "C/.kitchensync/BAK does not exist")

    stamp_dirs = sorted(entry for entry in bak.iterdir() if entry.is_dir())
    require(
        len(stamp_dirs) == 1,
        f"C/.kitchensync/BAK entries were {[entry.name for entry in bak.iterdir()]!r}",
    )

    bak_files = sorted(
        path.relative_to(stamp_dirs[0]).as_posix()
        for path in stamp_dirs[0].rglob("*")
        if path.is_file()
    )
    require(bak_files == ["extra.txt", "shared.txt"], f"BAK files were {bak_files!r}")
    require(
        (stamp_dirs[0] / "shared.txt").read_bytes() == b"wrong\n",
        "BAK shared.txt bytes differ",
    )
    require(
        (stamp_dirs[0] / "extra.txt").read_bytes() == b"extra\n",
        "BAK extra.txt bytes differ",
    )


def scenario() -> None:
    require(KITCHENSYNC.is_file(), f"missing released executable: {KITCHENSYNC}")

    temp_root = Path(tempfile.mkdtemp(prefix="kitchensync-S-10-"))
    try:
        peer_a = temp_root / "A"
        peer_b = temp_root / "B"
        peer_c = temp_root / "C"
        shutil.rmtree(peer_a, ignore_errors=True)
        shutil.rmtree(peer_b, ignore_errors=True)
        shutil.rmtree(peer_c, ignore_errors=True)

        write_file(peer_a / "shared.txt", b"group\n", GROUP_MTIME_NS)
        peer_b.mkdir(parents=True)

        first = run_kitchensync(["+A", "B"], temp_root)
        require(first.returncode == 0, f"first sync exit code was {first.returncode}")

        write_file(peer_c / "shared.txt", b"wrong\n")
        write_file(peer_c / "extra.txt", b"extra\n")
        require(
            not (peer_c / ".kitchensync" / "snapshot.db").exists(),
            "C has .kitchensync/snapshot.db before action",
        )

        action = run_kitchensync(["A", "B", "C"], temp_root)
        assert_completed(action)

        shared = peer_c / "shared.txt"
        require(shared.is_file(), "C/shared.txt does not exist")
        require(shared.read_bytes() == b"group\n", "C/shared.txt bytes differ")
        require(
            shared.stat().st_mtime_ns == GROUP_MTIME_NS,
            f"C/shared.txt mtime was {shared.stat().st_mtime_ns}",
        )
        require(not (peer_c / "extra.txt").exists(), "C/extra.txt still exists")
        assert_c_bak(peer_c)
        require(
            (peer_c / ".kitchensync" / "snapshot.db").is_file(),
            "C/.kitchensync/snapshot.db does not exist",
        )
    finally:
        shutil.rmtree(temp_root, ignore_errors=True)


def main() -> int:
    try:
        scenario()
    except Exception as exc:
        print(f"S-10 failed: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
