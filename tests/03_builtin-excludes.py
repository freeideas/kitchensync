#!/usr/bin/env -S uv run --script
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


PROJECT_DIR = Path("/home/ace/Desktop/prjx/kitchensync")
JAVA = PROJECT_DIR / "tools/compiler/jdk/bin/java"
JAR = PROJECT_DIR / "released/kitchensync.jar"
WORK = PROJECT_DIR / "tests/.tmp/03_builtin-excludes"


def clean(path: Path) -> None:
    if path.exists() or path.is_symlink():
        shutil.rmtree(path, ignore_errors=True)


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")


def run_sync(*peers: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(JAVA), "-jar", str(JAR), *peers],
        cwd=str(PROJECT_DIR),
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=60,
        check=False,
    )


def note_result(
    failures: list[str], name: str, condition: bool, detail: str = ""
) -> None:
    if condition:
        print(f"PASS: {name}")
    else:
        message = f"FAIL: {name}"
        if detail:
            message = f"{message}: {detail}"
        print(message)
        failures.append(message)


def no_entry(path: Path) -> bool:
    return not path.exists() and not path.is_symlink()


def text_equals(path: Path, expected: str, failures: list[str], name: str) -> None:
    try:
        actual = path.read_text(encoding="utf-8")
    except OSError as exc:
        note_result(failures, name, False, f"could not read {path}: {exc}")
        return
    note_result(
        failures,
        name,
        actual == expected,
        f"expected {expected!r} at {path}, got {actual!r}",
    )


def check_sync_succeeded(
    failures: list[str], name: str, result: subprocess.CompletedProcess[str]
) -> None:
    detail = (
        f"exit={result.returncode}\n"
        f"stdout:\n{result.stdout}\n"
        f"stderr:\n{result.stderr}"
    )
    note_result(failures, name, result.returncode == 0, detail)


def make_symlink(link: Path, target: Path, failures: list[str], name: str) -> None:
    try:
        link.symlink_to(target, target_is_directory=target.is_dir())
    except OSError as exc:
        failures.append(f"FAIL: {name}: could not create symlink: {exc}")


def make_fifo(path: Path, failures: list[str]) -> None:
    try:
        os.mkfifo(path)
    except OSError as exc:
        failures.append(f"FAIL: fifo fixture: could not create FIFO: {exc}")


def bind_socket(path: Path, failures: list[str]) -> socket.socket | None:
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    try:
        sock.bind(str(path))
        sock.listen(1)
        return sock
    except OSError as exc:
        sock.close()
        failures.append(f"FAIL: socket fixture: could not create socket: {exc}")
        return None


def exercise_hard_excludes(failures: list[str]) -> None:
    source = WORK / "hard-source"
    target = WORK / "hard-target"
    source.mkdir(parents=True)
    target.mkdir(parents=True)

    write_text(
        source / ".syncignore",
        "\n".join(
            [
                "!.kitchensync/",
                "!nested/.kitchensync/",
                "!link-file",
                "!link-dir/",
                "!fifo-entry",
                "!socket-entry",
                "",
            ]
        ),
    )
    write_text(source / "regular.txt", "regular file copied\n")
    write_text(source / "nested/keep.txt", "keeps parent directory observable\n")
    write_text(source / ".kitchensync/root-hidden.txt", "must not sync\n")
    write_text(source / "nested/.kitchensync/hidden.txt", "must not sync\n")
    write_text(source / ".git/config", "must not sync by default\n")
    write_text(source / "real-file.txt", "symlink target\n")
    write_text(source / "real-dir/inside.txt", "directory symlink target\n")
    make_symlink(source / "link-file", source / "real-file.txt", failures, "file symlink fixture")
    make_symlink(source / "link-dir", source / "real-dir", failures, "directory symlink fixture")
    # Device nodes are also special files, but portable plain-Python tests cannot
    # create them without elevated, OS-specific setup. FIFOs and sockets cover the
    # observable special-file exclusion through the public sync surface.
    make_fifo(source / "fifo-entry", failures)
    sock = bind_socket(source / "socket-entry", failures)

    try:
        result = run_sync(f"+{source}", f"-{target}")
    finally:
        if sock is not None:
            sock.close()

    check_sync_succeeded(failures, "sync with built-in excluded entries", result)
    text_equals(
        target / "regular.txt",
        "regular file copied\n",
        failures,
        "ordinary files still sync in built-in exclusion scenario",
    )
    note_result(
        failures,
        "root .kitchensync payload is never synced even when unignored",
        no_entry(target / ".kitchensync/root-hidden.txt"),
        f"unexpected entry at {target / '.kitchensync/root-hidden.txt'}",
    )
    note_result(
        failures,
        "nested .kitchensync payload is never synced even when unignored",
        no_entry(target / "nested/.kitchensync/hidden.txt"),
        f"unexpected entry at {target / 'nested/.kitchensync/hidden.txt'}",
    )
    note_result(
        failures,
        "file symlink is never synced even when unignored",
        no_entry(target / "link-file"),
        f"unexpected entry at {target / 'link-file'}",
    )
    note_result(
        failures,
        "directory symlink is never synced even when unignored",
        no_entry(target / "link-dir"),
        f"unexpected entry at {target / 'link-dir'}",
    )
    note_result(
        failures,
        "FIFO is never synced even when unignored",
        no_entry(target / "fifo-entry"),
        f"unexpected entry at {target / 'fifo-entry'}",
    )
    note_result(
        failures,
        "socket is never synced even when unignored",
        no_entry(target / "socket-entry"),
        f"unexpected entry at {target / 'socket-entry'}",
    )
    note_result(
        failures,
        ".git is excluded by default",
        no_entry(target / ".git/config"),
        f"unexpected entry at {target / '.git/config'}",
    )


def exercise_git_unignore(failures: list[str]) -> None:
    source = WORK / "git-source"
    target = WORK / "git-target"
    source.mkdir(parents=True)
    target.mkdir(parents=True)

    write_text(source / ".syncignore", "!.git/\n")
    write_text(source / ".git/config", "[core]\n\trepositoryformatversion = 0\n")
    write_text(source / ".git/refs/heads/main", "0123456789abcdef\n")
    write_text(source / "subdir/.git/config", "[core]\n\tbare = false\n")
    write_text(source / "normal.txt", "normal file copied\n")

    result = run_sync(f"+{source}", f"-{target}")
    check_sync_succeeded(failures, "sync with .git explicitly unignored", result)
    text_equals(
        target / ".git/config",
        "[core]\n\trepositoryformatversion = 0\n",
        failures,
        "!.git/ re-enables .git at that level",
    )
    text_equals(
        target / ".git/refs/heads/main",
        "0123456789abcdef\n",
        failures,
        "!.git/ re-enables entries below that level",
    )
    text_equals(
        target / "subdir/.git/config",
        "[core]\n\tbare = false\n",
        failures,
        "!.git/ follows normal hierarchy for lower .git directories",
    )


def main() -> int:
    failures: list[str] = []
    clean(WORK)
    WORK.mkdir(parents=True, exist_ok=True)

    try:
        exercise_hard_excludes(failures)
        exercise_git_unignore(failures)
    except subprocess.TimeoutExpired as exc:
        failures.append(f"FAIL: sync timed out: {exc}")
    except Exception as exc:
        failures.append(f"FAIL: unexpected test error: {type(exc).__name__}: {exc}")
    finally:
        clean(WORK)

    if failures:
        print("\nFailures:")
        for failure in failures:
            print(f"- {failure}")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
