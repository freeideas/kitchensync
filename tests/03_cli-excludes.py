#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

from __future__ import annotations

import shutil
import sqlite3
import stat
import subprocess
import sys
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

PROJECT_DIR = Path("C:/Users/human/Desktop/prjx/kitchensync")
JAVA = PROJECT_DIR / "tools" / "compiler" / "jdk" / "bin" / "java.exe"
JAR = PROJECT_DIR / "released" / "kitchensync.jar"
WORK = PROJECT_DIR / "tests" / ".tmp" / "03_cli_excludes"

FAILURES: list[str] = []


def check(condition: bool, message: str, details: str = "") -> None:
    if not condition:
        FAILURES.append(f"{message}\n{details}" if details else message)


def make_writable(path: Path) -> None:
    if not path.exists() and not path.is_symlink():
        return
    try:
        path.chmod(stat.S_IRUSR | stat.S_IWUSR | stat.S_IXUSR)
    except OSError:
        pass
    if path.is_dir() and not path.is_symlink():
        for child in path.iterdir():
            make_writable(child)


def reset_dir(path: Path) -> None:
    if path.exists():
        make_writable(path)
        shutil.rmtree(path)
    path.mkdir(parents=True, exist_ok=True)


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")


def run_sync(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(JAVA), "-jar", str(JAR), *args],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=60,
        check=False,
    )


def snapshot_text(peer: Path) -> str:
    db = peer / ".kitchensync" / "snapshot.db"
    if not db.exists():
        raise FileNotFoundError(db)
    parts: list[str] = []
    with sqlite3.connect(f"file:{db}?mode=ro", uri=True) as conn:
        rows = conn.execute("select basename from snapshot")
        parts.extend(str(row[0]) for row in rows)
    return "\n".join(parts)


def bak_contains(peer: Path, name: str) -> bool:
    return any("BAK" in path.parts and path.name == name for path in peer.rglob("*"))


def scenario_excludes_leave_entries_untouched() -> None:
    root = WORK / "basic"
    peer_a = root / "a"
    peer_b = root / "b"
    reset_dir(root)
    peer_a.mkdir()
    peer_b.mkdir()

    write_text(peer_a / "keep.txt", "copy me\n")
    write_text(peer_a / "skipdir" / "inside.txt", "excluded source dir\n")
    write_text(peer_a / "skipfile.txt", "excluded source file\n")
    write_text(peer_a / "brave" / "profile.txt", "excluded named dir\n")

    write_text(peer_b / "skipdir" / "inside.txt", "existing dest dir\n")
    write_text(peer_b / "skipfile.txt", "existing dest file\n")
    write_text(peer_b / "brave" / "profile.txt", "existing named dir\n")

    result = run_sync(f"+{peer_a}", f"-{peer_b}", "-x", "skipdir", "-x", "skipfile.txt", "-x", "brave")
    output = result.stdout + result.stderr
    check(result.returncode == 0, "exclude sync should exit 0", output)
    check((peer_b / "keep.txt").read_text(encoding="utf-8") == "copy me\n", "non-excluded file should copy")
    check(
        (peer_b / "skipdir" / "inside.txt").read_text(encoding="utf-8") == "existing dest dir\n",
        "excluded directory should be left untouched",
    )
    check(
        (peer_b / "skipfile.txt").read_text(encoding="utf-8") == "existing dest file\n",
        "excluded file should be left untouched",
    )
    check(
        (peer_b / "brave" / "profile.txt").read_text(encoding="utf-8") == "existing named dir\n",
        "excluded top-level directory should be left untouched",
    )
    check(not bak_contains(peer_b, "skipdir"), "excluded directory should not be displaced to BAK")
    check(not bak_contains(peer_b, "skipfile.txt"), "excluded file should not be displaced to BAK")

    for peer, label in ((peer_a, "peer_a"), (peer_b, "peer_b")):
        try:
            text = snapshot_text(peer)
        except Exception as exc:
            check(False, f"{label}: snapshot should be readable", repr(exc))
            continue
        for name in ("skipdir", "inside.txt", "skipfile.txt", "brave", "profile.txt"):
            check(name not in text, f"{label}: excluded entry {name!r} should not appear in snapshot")


def scenario_excludes_override_syncignore() -> None:
    root = WORK / "syncignore_override"
    peer_a = root / "a"
    peer_b = root / "b"
    reset_dir(root)
    peer_a.mkdir()
    peer_b.mkdir()

    write_text(peer_a / ".syncignore", "!blocked/\n!blocked/inside.txt\n")
    write_text(peer_a / "blocked" / "inside.txt", "must stay excluded\n")

    result = run_sync(f"+{peer_a}", f"-{peer_b}", "-x", "blocked")
    output = result.stdout + result.stderr
    check(result.returncode == 0, "exclude-over-syncignore sync should exit 0", output)
    check((peer_b / ".syncignore").is_file(), ".syncignore itself should still sync")
    check(not (peer_b / "blocked").exists(), "-x should not be overridden by .syncignore negation")


def scenario_excluding_syncignore_prevents_resolution() -> None:
    root = WORK / "exclude_syncignore"
    peer_a = root / "a"
    peer_b = root / "b"
    reset_dir(root)
    peer_a.mkdir()
    peer_b.mkdir()

    write_text(peer_a / ".syncignore", "*.secret\n")
    write_text(peer_a / "visible.secret", "copied because .syncignore is excluded\n")

    result = run_sync(f"+{peer_a}", f"-{peer_b}", "-x", ".syncignore")
    output = result.stdout + result.stderr
    check(result.returncode == 0, "excluded .syncignore sync should exit 0", output)
    check(not (peer_b / ".syncignore").exists(), "excluded .syncignore should not copy")
    check(
        (peer_b / "visible.secret").read_text(encoding="utf-8") == "copied because .syncignore is excluded\n",
        "excluding .syncignore should prevent its patterns from filtering siblings",
    )


def main() -> int:
    reset_dir(WORK)
    try:
        scenario_excludes_leave_entries_untouched()
        scenario_excludes_override_syncignore()
        scenario_excluding_syncignore_prevents_resolution()
    finally:
        make_writable(WORK)

    if FAILURES:
        print(f"{len(FAILURES)} check(s) failed:", file=sys.stderr)
        for index, failure in enumerate(FAILURES, 1):
            print(f"\n{index}. {failure}", file=sys.stderr)
        return 1

    print("03_cli-excludes checks passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
