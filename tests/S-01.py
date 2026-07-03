from __future__ import annotations

import subprocess
import sys
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
EXECUTABLE = ROOT / "released" / "kitchensync.exe"
HELP_SOURCE = ROOT / "specs" / "help.md"


def fail(message: str) -> None:
    raise AssertionError(message)


def expected_help_bytes() -> bytes:
    text = HELP_SOURCE.read_text(encoding="utf-8-sig", newline="")
    start_marker = "```\n"
    start = text.find(start_marker)
    if start == -1:
        fail(f"missing opening help text fence in {HELP_SOURCE}")
    start += len(start_marker)
    end = text.find("\n```", start)
    if end == -1:
        fail(f"missing closing help text fence in {HELP_SOURCE}")
    help_text = text[start : end + 1]
    if not help_text.endswith("\n"):
        fail(f"help text in {HELP_SOURCE} does not end with a newline")
    return help_text.encode("utf-8")


def snapshot_tree(root: Path) -> list[tuple[str, str, int | None, bytes | None]]:
    snapshot: list[tuple[str, str, int | None, bytes | None]] = []
    for path in sorted(root.rglob("*")):
        relative = path.relative_to(root).as_posix()
        if path.is_dir():
            snapshot.append((relative, "dir", None, None))
        elif path.is_file():
            data = path.read_bytes()
            snapshot.append((relative, "file", len(data), data))
        else:
            snapshot.append((relative, "other", None, None))
    return snapshot


def run_help() -> None:
    if not EXECUTABLE.is_file():
        fail(f"missing released executable: {EXECUTABLE}")

    expected_stdout = expected_help_bytes()
    with tempfile.TemporaryDirectory(prefix="kitchensync-S-01-") as work_dir_name:
        work_dir = Path(work_dir_name)
        before = snapshot_tree(work_dir)
        completed = subprocess.run(
            [str(EXECUTABLE)],
            cwd=work_dir,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=10,
            check=False,
        )
        after = snapshot_tree(work_dir)

    if completed.returncode != 0:
        fail(f"expected exit code 0, got {completed.returncode}")
    if completed.stdout != expected_stdout:
        fail("stdout did not exactly match the help text in specs/help.md")
    if completed.stderr != b"":
        fail(f"expected empty stderr, got {completed.stderr!r}")
    if after != before:
        fail("filesystem changed while running help with no arguments")


def main() -> int:
    try:
        run_help()
    except subprocess.TimeoutExpired:
        print("released executable timed out while printing help", file=sys.stderr)
        return 1
    except Exception as exc:
        print(str(exc), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
