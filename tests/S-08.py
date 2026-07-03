from __future__ import annotations

import shutil
import subprocess
import sys
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
EXE = ROOT / "released" / "kitchensync.exe"


def fail(message: str) -> None:
    raise AssertionError(message)


def require(condition: bool, message: str) -> None:
    if not condition:
        fail(message)


def run_kitchensync(cwd: Path) -> subprocess.CompletedProcess[str]:
    try:
        return subprocess.run(
            [str(EXE), "--dry-run", "--verbosity", "error", "+A", "B"],
            cwd=str(cwd),
            stdin=subprocess.DEVNULL,
            text=True,
            encoding="utf-8",
            errors="replace",
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=20,
            check=False,
        )
    except subprocess.TimeoutExpired as exc:
        fail(f"command timed out after {exc.timeout} seconds")


def user_files(peer: Path) -> list[str]:
    paths: list[str] = []
    for path in peer.rglob("*"):
        relative = path.relative_to(peer)
        if relative.parts[0] == ".kitchensync":
            continue
        if path.is_file():
            paths.append(relative.as_posix())
    return sorted(paths)


def assert_no_kitchensync_state(peer: Path, label: str) -> None:
    for relative in (
        ".kitchensync/snapshot.db",
        ".kitchensync/TMP",
        ".kitchensync/SWAP",
        ".kitchensync/BAK",
    ):
        require(
            not (peer / relative).exists(),
            f"{label}/{relative} exists after dry run",
        )


def scenario() -> None:
    require(EXE.is_file(), f"missing released executable: {EXE}")

    temp_root = Path(tempfile.mkdtemp(prefix="kitchensync-S-08-"))
    try:
        peer_a = temp_root / "A"
        peer_b = temp_root / "B"
        shutil.rmtree(peer_a, ignore_errors=True)
        shutil.rmtree(peer_b, ignore_errors=True)
        peer_a.mkdir(parents=True)
        peer_b.mkdir(parents=True)
        (peer_a / "dry.txt").write_bytes(b"plan only\n")

        action = run_kitchensync(temp_root)

        require(action.returncode == 0, f"exit code was {action.returncode}")
        require(
            action.stdout == "dry run\nsync complete\n",
            f"stdout was {action.stdout!r}",
        )
        require(action.stderr == "", f"stderr was {action.stderr!r}")
        require(
            (peer_a / "dry.txt").read_bytes() == b"plan only\n",
            "A/dry.txt bytes changed",
        )
        require(user_files(peer_b) == [], f"B user files were {user_files(peer_b)!r}")
        assert_no_kitchensync_state(peer_a, "A")
        assert_no_kitchensync_state(peer_b, "B")
    finally:
        shutil.rmtree(temp_root, ignore_errors=True)


def main() -> int:
    try:
        scenario()
    except Exception as exc:
        print(f"S-08 failed: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
