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


PROJECT_DIR = Path("/home/ace/Desktop/prjx/kitchensync")
JAVA = PROJECT_DIR / "tools/compiler/jdk/bin/java"
JAR = PROJECT_DIR / "released/kitchensync.jar"
WORK = PROJECT_DIR / "tests/.tmp/03_type-conflicts"


def run_cli(*peers: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(JAVA), "-jar", str(JAR), *[str(peer) for peer in peers]],
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


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")


def bak_entries(peer: Path, name: str) -> list[Path]:
    bak = peer / ".kitchensync/BAK"
    if not bak.exists():
        return []
    return sorted(entry for entry in bak.glob(f"*/{name}"))


def check(condition: bool, failures: list[str], message: str) -> None:
    if condition:
        print(f"PASS: {message}")
    else:
        print(f"FAIL: {message}")
        failures.append(message)


def check_file(path: Path, text: str, failures: list[str], message: str) -> None:
    if path.is_file():
        actual = path.read_text(encoding="utf-8")
        check(actual == text, failures, f"{message}: expected content {text!r}")
    else:
        check(False, failures, f"{message}: expected file at {path}")


def check_absent(path: Path, failures: list[str], message: str) -> None:
    check(not path.exists(), failures, f"{message}: expected absent at {path}")


def check_run_ok(
    result: subprocess.CompletedProcess[str], failures: list[str], message: str
) -> bool:
    detail = (
        f"{message}: exit {result.returncode}; "
        f"stdout={result.stdout.strip()!r}; stderr={result.stderr.strip()!r}"
    )
    check(result.returncode == 0, failures, detail)
    return result.returncode == 0


def scenario_file_wins_without_canon(failures: list[str]) -> None:
    root = WORK / "file-wins-without-canon"
    reset_dir(root)
    file_peer = root / "file-peer"
    dir_peer = root / "dir-peer"
    empty_peer = root / "empty-peer"
    for peer in (file_peer, dir_peer, empty_peer):
        peer.mkdir()

    write_text(file_peer / "seed.txt", "snapshot history\n")
    seeded = run_cli(Path("+" + str(file_peer)), dir_peer, empty_peer)
    if not check_run_ok(
        seeded,
        failures,
        "03.36 setup creates snapshot history before no-canon conflict",
    ):
        return

    write_text(file_peer / "conflict", "file winner\n")
    write_text(dir_peer / "conflict/inside.txt", "directory loser\n")

    result = run_cli(file_peer, dir_peer, empty_peer)
    if not check_run_ok(
        result,
        failures,
        "03.36/03.37 sync succeeds when file and directory conflict without canon",
    ):
        return

    for peer in (file_peer, dir_peer, empty_peer):
        check_file(
            peer / "conflict",
            "file winner\n",
            failures,
            f"03.37 winning file is present on {peer.name}",
        )

    entries = bak_entries(dir_peer, "conflict")
    check(
        len(entries) == 1 and entries[0].is_dir(),
        failures,
        "03.36 losing directory is displaced to BAK on the peer that had it",
    )
    if entries and entries[0].is_dir():
        check_file(
            entries[0] / "inside.txt",
            "directory loser\n",
            failures,
            "03.36 displaced directory keeps its original contents",
        )


def scenario_canon_type_wins(failures: list[str]) -> None:
    root = WORK / "canon-type-wins"
    reset_dir(root)
    canon_dir = root / "canon-dir"
    file_peer = root / "file-peer"
    empty_peer = root / "empty-peer"
    for peer in (canon_dir, file_peer, empty_peer):
        peer.mkdir()

    write_text(canon_dir / "conflict/from-canon.txt", "canon directory\n")
    write_text(file_peer / "conflict", "non-canon file\n")

    result = run_cli(Path("+" + str(canon_dir)), file_peer, empty_peer)
    if not check_run_ok(
        result,
        failures,
        "03.38 sync succeeds when canon directory conflicts with non-canon file",
    ):
        return

    for peer in (canon_dir, file_peer, empty_peer):
        check(
            (peer / "conflict").is_dir(),
            failures,
            f"03.38 canon directory type is present on {peer.name}",
        )
        check_file(
            peer / "conflict/from-canon.txt",
            "canon directory\n",
            failures,
            f"03.38 canon directory contents are synced to {peer.name}",
        )

    entries = bak_entries(file_peer, "conflict")
    check(
        len(entries) == 1 and entries[0].is_file(),
        failures,
        "03.38 losing file is displaced to BAK on the peer that had it",
    )
    if entries and entries[0].is_file():
        check_file(
            entries[0],
            "non-canon file\n",
            failures,
            "03.38 displaced file keeps its original contents",
        )


def scenario_canon_absence_wins(failures: list[str]) -> None:
    root = WORK / "canon-absence-wins"
    reset_dir(root)
    canon_empty = root / "canon-empty"
    file_peer = root / "file-peer"
    dir_peer = root / "dir-peer"
    for peer in (canon_empty, file_peer, dir_peer):
        peer.mkdir()

    write_text(file_peer / "conflict", "file displaced\n")
    write_text(dir_peer / "conflict/inside.txt", "directory displaced\n")

    result = run_cli(Path("+" + str(canon_empty)), file_peer, dir_peer)
    if not check_run_ok(
        result,
        failures,
        "03.39 sync succeeds when canon lacks a file/directory conflict path",
    ):
        return

    for peer in (canon_empty, file_peer, dir_peer):
        check_absent(
            peer / "conflict",
            failures,
            f"03.39 canon absence removes conflict path from {peer.name}",
        )

    file_entries = bak_entries(file_peer, "conflict")
    check(
        len(file_entries) == 1 and file_entries[0].is_file(),
        failures,
        "03.39 non-canon file is displaced to BAK when canon lacks the path",
    )
    if file_entries and file_entries[0].is_file():
        check_file(
            file_entries[0],
            "file displaced\n",
            failures,
            "03.39 displaced file keeps its original contents",
        )

    dir_entries = bak_entries(dir_peer, "conflict")
    check(
        len(dir_entries) == 1 and dir_entries[0].is_dir(),
        failures,
        "03.39 non-canon directory is displaced to BAK when canon lacks the path",
    )
    if dir_entries and dir_entries[0].is_dir():
        check_file(
            dir_entries[0] / "inside.txt",
            "directory displaced\n",
            failures,
            "03.39 displaced directory keeps its original contents",
        )


def main() -> int:
    reset_dir(WORK)
    failures: list[str] = []

    scenario_file_wins_without_canon(failures)
    scenario_canon_type_wins(failures)
    scenario_canon_absence_wins(failures)

    if failures:
        print("\nFailures:")
        for failure in failures:
            print(f"- {failure}")
        return 1

    print("\nAll type-conflict checks passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
