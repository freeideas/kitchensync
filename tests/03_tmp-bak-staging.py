#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

from __future__ import annotations

import os
import re
import shutil
import stat
import subprocess
import sys
from pathlib import Path


PROJECT_DIR = Path("/home/ace/Desktop/prjx/kitchensync")
JAVA = Path("/home/ace/Desktop/prjx/kitchensync/tools/compiler/jdk/bin/java")
JAR = Path("/home/ace/Desktop/prjx/kitchensync/released/kitchensync.jar")
WORK = PROJECT_DIR / "tests" / ".tmp" / "03_tmp-bak-staging"
TIMESTAMP_RE = re.compile(r"^\d{4}-\d{2}-\d{2}_\d{2}-\d{2}-\d{2}_\d{6}Z$")
UUID_RE = re.compile(
    r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-"
    r"[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$"
)


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


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")


def set_mtime(path: Path, seconds: float) -> None:
    ns = int(seconds * 1_000_000_000)
    os.utime(path, ns=(ns, ns))


def collect_relative_files(root: Path) -> set[str]:
    if not root.exists():
        return set()
    return {
        str(path.relative_to(root))
        for path in root.rglob("*")
        if path.is_file() and ".kitchensync" not in path.parts
    }


def timestamp_dirs(path: Path) -> list[Path]:
    if not path.exists():
        return []
    return sorted(child for child in path.iterdir() if child.is_dir())


def check(condition: bool, failures: list[str], message: str) -> None:
    if not condition:
        failures.append(message)


def check_successful_file_overwrite(failures: list[str]) -> None:
    src = WORK / "overwrite-src"
    dst = WORK / "overwrite-dst"
    target = dst / "nested" / "same.txt"
    source_file = src / "nested" / "same.txt"
    write_text(source_file, "new winner\n")
    write_text(target, "old displaced file\n")
    set_mtime(source_file, 1_700_000_000.123456)
    set_mtime(target, 1_600_000_000.654321)

    result = run_cli(f"+{src}", str(dst))
    check(
        result.returncode == 0,
        failures,
        "file overwrite sync should exit 0; "
        f"stdout={result.stdout!r} stderr={result.stderr!r}",
    )

    check(target.is_file(), failures, "destination file should exist after overwrite copy")
    if target.exists():
        check(
            target.read_text(encoding="utf-8") == "new winner\n",
            failures,
            "destination file should contain the copied source content",
        )
        check(
            abs(target.stat().st_mtime - source_file.stat().st_mtime) < 0.01,
            failures,
            "destination file mod_time should match the winning source mod_time",
        )

    bak_root = dst / "nested" / ".kitchensync" / "BAK"
    bak_timestamps = timestamp_dirs(bak_root)
    check(bak_timestamps, failures, "overwritten file should create colocated BAK timestamp dir")
    check(
        all(TIMESTAMP_RE.match(path.name) for path in bak_timestamps),
        failures,
        "BAK timestamp directories should use YYYY-MM-DD_HH-mm-ss_ffffffZ",
    )
    displaced_files = [path / "same.txt" for path in bak_timestamps if (path / "same.txt").is_file()]
    check(
        len(displaced_files) == 1,
        failures,
        "old destination file should be displaced to nested/.kitchensync/BAK/<timestamp>/same.txt",
    )
    if displaced_files:
        check(
            displaced_files[0].read_text(encoding="utf-8") == "old displaced file\n",
            failures,
            "BAK copy should preserve the overwritten file content",
        )
    check(
        not (dst / ".kitchensync" / "BAK").exists(),
        failures,
        "nested file displacement should not be aggregated under the peer root BAK",
    )

    tmp_root = dst / "nested" / ".kitchensync" / "TMP"
    tmp_timestamps = timestamp_dirs(tmp_root)
    check(tmp_timestamps, failures, "successful copy should create colocated TMP timestamp dir")
    check(
        all(TIMESTAMP_RE.match(path.name) for path in tmp_timestamps),
        failures,
        "TMP timestamp directories should use YYYY-MM-DD_HH-mm-ss_ffffffZ",
    )
    leftover_uuid_dirs = [
        child
        for timestamp in tmp_timestamps
        for child in timestamp.iterdir()
        if child.is_dir() and UUID_RE.match(child.name)
    ]
    check(
        not leftover_uuid_dirs,
        failures,
        "successful copy should remove empty per-transfer TMP <timestamp>/<uuid> dirs",
    )


def check_directory_displacement(failures: list[str]) -> None:
    src = WORK / "dir-displace-src"
    dst = WORK / "dir-displace-dst"
    source_file = src / "entry"
    write_text(source_file, "file replacing directory\n")
    write_text(dst / "entry" / "child" / "grandchild.txt", "preserved subtree\n")
    set_mtime(source_file, 1_710_000_000.222222)

    result = run_cli(f"+{src}", str(dst))
    check(
        result.returncode == 0,
        failures,
        "directory displacement sync should exit 0; "
        f"stdout={result.stdout!r} stderr={result.stderr!r}",
    )

    check(source_file.is_file(), failures, "source replacement file should remain a file")
    check((dst / "entry").is_file(), failures, "destination directory should be replaced by file")
    if (dst / "entry").is_file():
        check(
            (dst / "entry").read_text(encoding="utf-8") == "file replacing directory\n",
            failures,
            "replacement file content should be copied into place",
        )

    bak_root = dst / ".kitchensync" / "BAK"
    bak_timestamps = timestamp_dirs(bak_root)
    check(bak_timestamps, failures, "displaced directory should create peer-root BAK timestamp dir")
    check(
        all(TIMESTAMP_RE.match(path.name) for path in bak_timestamps),
        failures,
        "directory BAK timestamp directories should use YYYY-MM-DD_HH-mm-ss_ffffffZ",
    )
    preserved = [
        path / "entry" / "child" / "grandchild.txt"
        for path in bak_timestamps
        if (path / "entry" / "child" / "grandchild.txt").is_file()
    ]
    check(
        len(preserved) == 1,
        failures,
        "displaced directory should move to BAK as one subtree-preserving entry",
    )
    if preserved:
        check(
            preserved[0].read_text(encoding="utf-8") == "preserved subtree\n",
            failures,
            "BAK directory subtree should preserve original nested file content",
        )


def check_failed_transfer_preserves_destination(failures: list[str]) -> None:
    src = WORK / "failure-src"
    dst = WORK / "failure-dst"
    source_file = src / "nested" / "blocked.txt"
    target = dst / "nested" / "blocked.txt"
    write_text(source_file, "unreadable replacement\n")
    write_text(target, "must survive failure\n")
    set_mtime(source_file, 1_720_000_000.333333)
    source_file.chmod(0)

    try:
        result = run_cli(f"+{src}", str(dst))
    finally:
        source_file.chmod(stat.S_IRUSR | stat.S_IWUSR)

    check(
        "Unable to access jarfile" not in result.stderr,
        failures,
        "failed-transfer case should reach the KitchenSync product, not fail in the Java launcher",
    )
    check(target.is_file(), failures, "failed transfer should leave existing destination file in place")
    if target.exists():
        check(
            target.read_text(encoding="utf-8") == "must survive failure\n",
            failures,
            "failed transfer should not partially replace destination content",
        )

    tmp_root = dst / "nested" / ".kitchensync" / "TMP"
    staged_files = [
        path
        for path in tmp_root.rglob("blocked.txt")
        if path.is_file()
    ] if tmp_root.exists() else []
    check(
        not staged_files,
        failures,
        "failed transfer should delete its TMP staging file",
    )
    check(
        collect_relative_files(dst) == {"nested/blocked.txt"},
        failures,
        "failed transfer should not create extra non-metadata files in destination tree",
    )


def main() -> int:
    failures: list[str] = []
    shutil.rmtree(WORK, ignore_errors=True)
    WORK.mkdir(parents=True, exist_ok=True)

    try:
        check_successful_file_overwrite(failures)
        check_directory_displacement(failures)
        check_failed_transfer_preserves_destination(failures)
    finally:
        shutil.rmtree(WORK, ignore_errors=True)

    if failures:
        print("FAIL")
        for index, failure in enumerate(failures, start=1):
            print(f"{index}. {failure}")
        return 1

    print("PASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
