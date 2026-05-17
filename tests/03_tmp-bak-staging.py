#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

from __future__ import annotations

import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

PROJECT_DIR = Path("C:/Users/human/Desktop/prjx/kitchensync")
JAVA = Path("C:/Users/human/Desktop/prjx/kitchensync/tools/compiler/jdk/bin/java.exe")
JAR = Path("C:/Users/human/Desktop/prjx/kitchensync/released/kitchensync.jar")
WORK = PROJECT_DIR / "tests" / "tmp" / "03_tmp-bak-staging"
STAMP_RE = re.compile(r"^\d{4}-\d{2}-\d{2}_\d{2}-\d{2}-\d{2}_\d{6}Z$")


def reset_dir(path: Path) -> None:
    if path.exists():
        shutil.rmtree(path)
    path.mkdir(parents=True)


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def run_sync(src: Path, dst: Path, timeout: int = 60) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(JAVA), "-jar", str(JAR), f"+{src}", f"-{dst}"],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=timeout,
        check=False,
    )


def bak_entries(parent: Path, basename: str) -> list[Path]:
    """Return paths matching parent/.kitchensync/BAK/<timestamp>/<basename>."""
    root = parent / ".kitchensync" / "BAK"
    if not root.exists():
        return []
    entries: list[Path] = []
    for ts_dir in root.iterdir():
        candidate = ts_dir / basename
        if candidate.exists():
            entries.append(candidate)
    return sorted(entries)


def tmp_uuid_dirs(parent: Path) -> list[Path]:
    """Return all per-transfer <timestamp>/<uuid>/ directories under parent/.kitchensync/TMP/."""
    root = parent / ".kitchensync" / "TMP"
    if not root.exists():
        return []
    dirs: list[Path] = []
    for ts_dir in root.iterdir():
        if ts_dir.is_dir():
            dirs.extend(p for p in ts_dir.iterdir() if p.is_dir())
    return sorted(dirs)


def check(condition: bool, failures: list[str], message: str) -> None:
    if not condition:
        failures.append(message)


# --- Scenario A: file copy with pre-existing destination ---
# Covers: 03.29, 03.31, 03.32, 03.33 BAK timestamps, 03.89
# 03.28: not reasonably testable -- the in-flight TMP path is transient at the public CLI surface.
# 03.30: not reasonably testable -- same-filesystem atomic rename from TMP is not observable after completion.
# 03.33 TMP timestamps: not reasonably testable -- successful copies remove the per-transfer TMP directory.

def check_file_copy(failures: list[str]) -> None:
    src = WORK / "copy-src"
    dst = WORK / "copy-dst"
    reset_dir(src)
    reset_dir(dst)

    src_file = src / "nested" / "note.txt"
    dst_file = dst / "nested" / "note.txt"
    write_text(src_file, "new content\n")
    write_text(dst_file, "old content\n")

    # Known winning mod_time set explicitly so we can assert 03.31
    winning_ns = 1_700_000_123_456_789_000
    os.utime(str(src_file), ns=(winning_ns, winning_ns))

    result = run_sync(src, dst)
    check(
        result.returncode == 0,
        failures,
        f"sync should exit 0; rc={result.returncode}\nstdout:\n{result.stdout}\nstderr:\n{result.stderr}",
    )
    if result.returncode != 0:
        return

    # The replacement must land before BAK and mtime checks can be meaningful.
    check(
        dst_file.exists() and dst_file.read_text(encoding="utf-8") == "new content\n",
        failures,
        "replacement file not at target path or content wrong after sync",
    )

    # 03.31: destination mod_time equals the winning mod_time (not re-read from source)
    copied_ns = dst_file.stat().st_mtime_ns
    check(
        abs(copied_ns - winning_ns) <= 2_000_000_000,
        failures,
        f"03.31: dst mod_time {copied_ns} differs from winning mod_time {winning_ns} by more than 2s",
    )

    # 03.29: pre-existing destination file displaced to BAK before new file placed
    bak = bak_entries(dst / "nested", "note.txt")
    check(len(bak) == 1, failures, f"03.29: expected 1 BAK entry for displaced note.txt; got {bak}")
    if bak:
        check(
            bak[0].read_text(encoding="utf-8") == "old content\n",
            failures,
            "03.29: BAK file should preserve the displaced content",
        )
        # 03.33: BAK timestamp directory uses YYYY-MM-DD_HH-mm-ss_ffffffZ format
        check(
            STAMP_RE.match(bak[0].parent.name) is not None,
            failures,
            f"03.33: BAK timestamp '{bak[0].parent.name}' does not match YYYY-MM-DD_HH-mm-ss_ffffffZ",
        )

    # 03.32: BAK colocated at the parent of the affected entry, not at the sync root
    root_bak = bak_entries(dst, "note.txt")
    check(
        root_bak == [],
        failures,
        f"03.32: nested file displacement aggregated at sync-root BAK instead of parent BAK; found {root_bak}",
    )

    # 03.89: per-transfer TMP <uuid>/ directories removed after successful copy
    uuid_dirs = tmp_uuid_dirs(dst / "nested")
    check(
        uuid_dirs == [],
        failures,
        f"03.89: per-transfer TMP uuid dirs not removed after successful copy; found {uuid_dirs}",
    )


# --- Scenario B: directory displaced to BAK ---
# Covers: 03.34

def check_directory_displacement(failures: list[str]) -> None:
    src = WORK / "dir-src"
    dst = WORK / "dir-dst"
    reset_dir(src)
    reset_dir(dst)

    # src has "item" as a file; dst has "item" as a directory with content
    write_text(src / "item", "replacement file\n")
    write_text(dst / "item" / "child" / "kept.txt", "subtree content\n")

    result = run_sync(src, dst)
    check(
        result.returncode == 0,
        failures,
        f"directory displacement sync should exit 0; rc={result.returncode}\nstdout:\n{result.stdout}\nstderr:\n{result.stderr}",
    )
    if result.returncode != 0:
        return

    # Winning file from src is now at the destination path
    check(
        (dst / "item").is_file(),
        failures,
        "03.34: dst/item should be a file after canon src file displaces the directory",
    )
    check(
        (dst / "item").read_text(encoding="utf-8") == "replacement file\n",
        failures,
        "03.34: dst/item content wrong after directory displacement",
    )

    # 03.34: displaced directory moved to BAK as a single rename, subtree preserved
    bak = bak_entries(dst, "item")
    check(len(bak) == 1, failures, f"03.34: expected 1 BAK entry for displaced directory; got {bak}")
    if bak:
        check(
            bak[0].is_dir(),
            failures,
            "03.34: BAK entry for displaced directory should remain a directory",
        )
        check(
            (bak[0] / "child" / "kept.txt").exists()
            and (bak[0] / "child" / "kept.txt").read_text(encoding="utf-8") == "subtree content\n",
            failures,
            "03.34: displaced directory subtree not preserved in BAK",
        )
        check(
            STAMP_RE.match(bak[0].parent.name) is not None,
            failures,
            f"03.33: directory BAK timestamp '{bak[0].parent.name}' does not match YYYY-MM-DD_HH-mm-ss_ffffffZ",
        )


# 03.35: TMP staging file deleted on transfer failure
# not reasonably testable -- triggering a mid-transfer failure through the public CLI
# would require sabotaging the filesystem environment


def main() -> int:
    failures: list[str] = []
    reset_dir(WORK)
    try:
        check_file_copy(failures)
        check_directory_displacement(failures)
    finally:
        if WORK.exists():
            shutil.rmtree(WORK)

    if failures:
        print("FAIL")
        for i, f in enumerate(failures, 1):
            print(f"{i}. {f}")
        return 1

    print("PASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
