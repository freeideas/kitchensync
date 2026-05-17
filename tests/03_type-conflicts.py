#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

from __future__ import annotations

import shutil
import subprocess
import sys
from pathlib import Path


sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

PROJECT_DIR = Path("C:/Users/human/Desktop/prjx/kitchensync")
JAVA = Path("C:/Users/human/Desktop/prjx/kitchensync/tools/compiler/jdk/bin/java.exe")
JAR = Path("C:/Users/human/Desktop/prjx/kitchensync/released/kitchensync.jar")
WORK = PROJECT_DIR / "tmp" / "test_03_type_conflicts"


def run_cli(*args: str) -> subprocess.CompletedProcess[str]:
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


def record(failures: list[str], ok: bool, message: str) -> None:
    if ok:
        print(f"PASS: {message}")
    else:
        print(f"FAIL: {message}")
        failures.append(message)


def require_success(
    failures: list[str], result: subprocess.CompletedProcess[str], label: str
) -> None:
    record(
        failures,
        result.returncode == 0,
        (
            f"{label} exits 0"
            if result.returncode == 0
            else f"{label} exits 0; got {result.returncode}\n"
            f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}"
        ),
    )


def reset_dir(path: Path) -> None:
    if path.exists():
        shutil.rmtree(path)
    path.mkdir(parents=True)


def write_file(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def bak_matches(peer: Path, name: str) -> list[Path]:
    bak_root = peer / ".kitchensync" / "BAK"
    if not bak_root.exists():
        return []
    return [candidate for candidate in bak_root.glob(f"*/{name}") if candidate.exists()]


def any_bak_file_contains(peer: Path, name: str, text: str) -> bool:
    for candidate in bak_matches(peer, name):
        if candidate.is_file() and candidate.read_text(encoding="utf-8") == text:
            return True
    return False


def any_bak_dir_contains(peer: Path, name: str, child: str, text: str) -> bool:
    for candidate in bak_matches(peer, name):
        displaced_child = candidate / child
        if (
            candidate.is_dir()
            and displaced_child.is_file()
            and displaced_child.read_text(encoding="utf-8") == text
        ):
            return True
    return False


def sync(failures: list[str], label: str, *peers: str) -> None:
    require_success(failures, run_cli(*peers), label)


def test_no_canon_file_wins(failures: list[str]) -> None:
    base = WORK / "no_canon_file_wins"
    reset_dir(base)
    file_peer = base / "file_peer"
    dir_peer = base / "dir_peer"
    file_peer.mkdir()
    dir_peer.mkdir()

    write_file(file_peer / "seed.txt", "seed\n")
    sync(failures, "seed no-canon peers", f"+{file_peer}", str(dir_peer))

    write_file(file_peer / "shared", "file wins\n")
    (dir_peer / "shared").mkdir()
    write_file(dir_peer / "shared" / "old.txt", "directory loses\n")
    sync(failures, "resolve no-canon type conflict", str(file_peer), str(dir_peer))

    record(
        failures,
        (file_peer / "shared").is_file()
        and (file_peer / "shared").read_text(encoding="utf-8") == "file wins\n",
        "03.36 no-canon conflict keeps the file at the conflicting path",
    )
    record(
        failures,
        (dir_peer / "shared").is_file()
        and (dir_peer / "shared").read_text(encoding="utf-8") == "file wins\n",
        "03.37 winning file is propagated to the peer that had a directory",
    )
    record(
        failures,
        any_bak_dir_contains(dir_peer, "shared", "old.txt", "directory loses\n"),
        "03.36 losing directory is displaced to BAK on the peer that had it",
    )


def test_canon_entry_type_wins(failures: list[str]) -> None:
    base = WORK / "canon_entry_type_wins"
    reset_dir(base)
    canon = base / "canon"
    file_peer = base / "file_peer"
    canon.mkdir()
    file_peer.mkdir()

    write_file(canon / "seed.txt", "seed\n")
    sync(failures, "seed canon-entry peers", f"+{canon}", str(file_peer))

    (canon / "shared").mkdir()
    write_file(canon / "shared" / "canon-child.txt", "canon directory\n")
    write_file(file_peer / "shared", "file loses to canon directory\n")
    sync(failures, "resolve canon-entry type conflict", f"+{canon}", str(file_peer))

    record(
        failures,
        (file_peer / "shared").is_dir(),
        "03.38 canon peer directory type replaces the other peer's file type",
    )
    record(
        failures,
        (file_peer / "shared" / "canon-child.txt").is_file()
        and (file_peer / "shared" / "canon-child.txt").read_text(encoding="utf-8")
        == "canon directory\n",
        "03.38 canon directory contents are synced after the type decision",
    )
    record(
        failures,
        any_bak_file_contains(file_peer, "shared", "file loses to canon directory\n"),
        "03.38 losing file is displaced to BAK on the peer that had it",
    )


def test_canon_file_entry_wins(failures: list[str]) -> None:
    base = WORK / "canon_file_entry_wins"
    reset_dir(base)
    canon = base / "canon"
    dir_peer = base / "dir_peer"
    canon.mkdir()
    dir_peer.mkdir()

    write_file(canon / "seed.txt", "seed\n")
    sync(failures, "seed canon-file peers", f"+{canon}", str(dir_peer))

    write_file(canon / "shared", "canon file wins\n")
    (dir_peer / "shared").mkdir()
    write_file(dir_peer / "shared" / "old.txt", "directory loses to canon file\n")
    sync(failures, "resolve canon-file type conflict", f"+{canon}", str(dir_peer))

    record(
        failures,
        (dir_peer / "shared").is_file()
        and (dir_peer / "shared").read_text(encoding="utf-8") == "canon file wins\n",
        "03.38 canon peer file type replaces the other peer's directory type",
    )
    record(
        failures,
        any_bak_dir_contains(dir_peer, "shared", "old.txt", "directory loses to canon file\n"),
        "03.38 losing directory is displaced to BAK on the peer that had it",
    )


def test_canon_absence_wins(failures: list[str]) -> None:
    base = WORK / "canon_absence_wins"
    reset_dir(base)
    canon = base / "canon"
    file_peer = base / "file_peer"
    dir_peer = base / "dir_peer"
    canon.mkdir()
    file_peer.mkdir()
    dir_peer.mkdir()

    write_file(canon / "seed.txt", "seed\n")
    sync(failures, "seed canon-absence peers", f"+{canon}", str(file_peer), str(dir_peer))

    write_file(file_peer / "shared", "file loses to canon absence\n")
    (dir_peer / "shared").mkdir()
    write_file(dir_peer / "shared" / "old.txt", "directory loses to canon absence\n")
    sync(
        failures,
        "resolve canon-absence type conflict",
        f"+{canon}",
        str(file_peer),
        str(dir_peer),
    )

    record(
        failures,
        not (canon / "shared").exists()
        and not (file_peer / "shared").exists()
        and not (dir_peer / "shared").exists(),
        "03.39 canon absence removes the conflicting path from every peer",
    )
    record(
        failures,
        any_bak_file_contains(file_peer, "shared", "file loses to canon absence\n"),
        "03.39 file at canon-absent conflict path is displaced to BAK",
    )
    record(
        failures,
        any_bak_dir_contains(
            dir_peer, "shared", "old.txt", "directory loses to canon absence\n"
        ),
        "03.39 directory at canon-absent conflict path is displaced to BAK",
    )


def main() -> int:
    failures: list[str] = []
    if WORK.exists():
        shutil.rmtree(WORK)
    WORK.mkdir(parents=True)

    test_no_canon_file_wins(failures)
    test_canon_entry_type_wins(failures)
    test_canon_file_entry_wins(failures)
    test_canon_absence_wins(failures)

    if failures:
        print(f"\n{len(failures)} check(s) failed:")
        for failure in failures:
            print(f"- {failure}")
        return 1
    print("\nAll type-conflict checks passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
