#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

from __future__ import annotations

import os
import shutil
import socket
import subprocess
import sys
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

JAVA = Path("C:/Users/human/Desktop/prjx/kitchensync/tools/compiler/jdk/bin/java.exe")
JAR = Path("C:/Users/human/Desktop/prjx/kitchensync/released/kitchensync.jar")
PROJECT_DIR = Path("C:/Users/human/Desktop/prjx/kitchensync")
WORK = PROJECT_DIR / "tests" / "tmp" / "03_builtin-excludes"


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


def reset_dir(path: Path) -> None:
    if path.exists():
        shutil.rmtree(path)
    path.mkdir(parents=True)


def write(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def check(failures: list[str], condition: bool, message: str) -> None:
    if not condition:
        failures.append(message)


def check_run(
    failures: list[str], result: subprocess.CompletedProcess[str], label: str
) -> None:
    if result.returncode != 0:
        failures.append(
            f"{label} exited {result.returncode}\n"
            f"stdout:\n{result.stdout}\n"
            f"stderr:\n{result.stderr}"
        )


# 03.47, 03.48, 03.49: .kitchensync/, symlinks, and special files are never
# synced even when .syncignore explicitly attempts to negate their exclusion.
def scenario_hard_builtin_excludes(failures: list[str]) -> None:
    source = WORK / "hard-src"
    dest = WORK / "hard-dst"
    reset_dir(source)
    reset_dir(dest)

    write(source / "ordinary.txt", "ordinary\n")
    write(source / ".kitchensync" / "private.txt", "must not sync\n")
    write(source / "nested" / ".kitchensync" / "private.txt", "must not sync\n")

    # attempt symlinks -- skip symlink assertions if the OS disallows creation
    symlinks_created = False
    write(source / "file-target.txt", "target\n")
    (source / "dir-target").mkdir()
    write(source / "dir-target" / "inside.txt", "inside\n")
    try:
        (source / "link-file").symlink_to(source / "file-target.txt")
        (source / "link-dir").symlink_to(source / "dir-target", target_is_directory=True)
        symlinks_created = True
    except OSError:
        print("03.48: skipped -- cannot create symlinks in this environment")

    # attempt special files (platform-dependent)
    special_entries: list[Path] = []
    open_sockets: list[socket.socket] = []
    if hasattr(os, "mkfifo"):
        fifo = source / "special-fifo"
        try:
            os.mkfifo(fifo)
            special_entries.append(fifo)
        except OSError:
            pass
    if hasattr(socket, "AF_UNIX"):
        sock_path = source / "special-socket"
        try:
            unix_sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            unix_sock.bind(str(sock_path))
            unix_sock.listen(1)
            open_sockets.append(unix_sock)
            special_entries.append(sock_path)
        except OSError:
            pass
    if not special_entries:
        print("03.49: not reasonably testable on this host -- no FIFO or AF_UNIX socket available")

    # .syncignore negates every hard exclude -- none should have any effect
    ignore_lines = [
        "!.kitchensync/", "!.kitchensync/**",
        "!nested/.kitchensync/", "!nested/.kitchensync/**",
    ]
    if symlinks_created:
        ignore_lines += ["!link-file", "!link-dir/"]
    for entry in special_entries:
        ignore_lines.append(f"!{entry.name}")
    write(source / ".syncignore", "\n".join(ignore_lines) + "\n")

    try:
        result = run_cli(f"+{source}", str(dest))
    finally:
        for sock in open_sockets:
            sock.close()

    check_run(failures, result, "hard built-in excludes")
    check(
        failures,
        (dest / "ordinary.txt").read_text(encoding="utf-8") == "ordinary\n"
        if (dest / "ordinary.txt").exists() else False,
        "ordinary.txt must sync (baseline)",
    )
    # 03.47
    check(
        failures,
        not (dest / ".kitchensync" / "private.txt").exists(),
        "03.47: root .kitchensync/ content must not sync even when negated",
    )
    check(
        failures,
        not (dest / "nested" / ".kitchensync" / "private.txt").exists(),
        "03.47: nested .kitchensync/ content must not sync even when negated",
    )
    # 03.48
    if symlinks_created:
        check(
            failures,
            not (dest / "link-file").exists() and not (dest / "link-file").is_symlink(),
            "03.48: file symlink must not sync even when negated",
        )
        check(
            failures,
            not (dest / "link-dir").exists() and not (dest / "link-dir").is_symlink(),
            "03.48: directory symlink must not sync even when negated",
        )
    # 03.49
    for entry in special_entries:
        check(
            failures,
            not (dest / entry.name).exists(),
            f"03.49: special entry {entry.name!r} must not sync even when negated",
        )


# 03.50: .git/ is excluded from sync by default (no .syncignore involved).
def scenario_git_default_excluded(failures: list[str]) -> None:
    source = WORK / "git-default-src"
    dest = WORK / "git-default-dst"
    reset_dir(source)
    reset_dir(dest)

    write(source / "ordinary.txt", "ordinary\n")
    write(source / ".git" / "config", "must not sync\n")
    result = run_cli(f"+{source}", str(dest))

    check_run(failures, result, ".git default exclusion")
    check(failures, (dest / "ordinary.txt").exists(), "ordinary.txt must sync (baseline)")
    check(failures, not (dest / ".git").exists(), "03.50: .git/ must be excluded by default")


# 03.51: !.git/ in .syncignore re-enables .git/ at that level and below.
def scenario_git_reenabled(failures: list[str]) -> None:
    source = WORK / "git-reenabled-src"
    dest = WORK / "git-reenabled-dst"
    reset_dir(source)
    reset_dir(dest)

    write(source / ".syncignore", "!.git/\n")
    write(source / ".git" / "config", "root git config\n")
    write(source / "nested" / ".git" / "config", "nested git config\n")
    result = run_cli(f"+{source}", str(dest))

    check_run(failures, result, ".git re-enabled")
    check(
        failures,
        (dest / ".git" / "config").read_text(encoding="utf-8") == "root git config\n"
        if (dest / ".git" / "config").exists() else False,
        "03.51: !.git/ must allow .git/ to sync at .syncignore level",
    )
    check(
        failures,
        (dest / "nested" / ".git" / "config").read_text(encoding="utf-8") == "nested git config\n"
        if (dest / "nested" / ".git" / "config").exists() else False,
        "03.51: !.git/ must allow .git/ to sync below .syncignore level",
    )


def main() -> int:
    failures: list[str] = []
    reset_dir(WORK)
    try:
        scenario_hard_builtin_excludes(failures)
        scenario_git_default_excluded(failures)
        scenario_git_reenabled(failures)
    finally:
        if WORK.exists():
            shutil.rmtree(WORK)

    if failures:
        print("\nFAILURES:")
        for i, f in enumerate(failures, 1):
            print(f"\n{i}. {f}")
        return 1

    print("03_builtin-excludes: all checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
