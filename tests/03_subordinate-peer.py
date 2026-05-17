#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

from __future__ import annotations

import os
import shutil
import subprocess
import sys
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")


PROJECT_DIR = Path("C:/Users/human/Desktop/prjx/kitchensync")
JAVA = Path("C:/Users/human/Desktop/prjx/kitchensync/tools/compiler/jdk/bin/java.exe")
JAR = Path("C:/Users/human/Desktop/prjx/kitchensync/released/kitchensync.jar")
WORK = PROJECT_DIR / ".test-work" / "03_subordinate-peer"


def write_file(path: Path, text: str, mtime: int) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")
    os.utime(path, (mtime, mtime))


def run_sync(*peers: str) -> subprocess.CompletedProcess[str]:
    command = [str(JAVA), "-jar", str(JAR), *peers]
    try:
        return subprocess.run(
            command,
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
    except subprocess.TimeoutExpired as exc:
        stdout = exc.stdout if isinstance(exc.stdout, str) else ""
        stderr = exc.stderr if isinstance(exc.stderr, str) else ""
        return subprocess.CompletedProcess(
            command,
            124,
            stdout=stdout,
            stderr=f"{stderr}\nTimed out after {exc.timeout} seconds",
        )


def bak_matches(peer: Path, name: str) -> list[Path]:
    bak_root = peer / ".kitchensync" / "BAK"
    if not bak_root.exists():
        return []
    return sorted(path for path in bak_root.rglob(name) if path.is_file())


def record(condition: bool, message: str, failures: list[str]) -> None:
    if not condition:
        failures.append(message)


def record_file(path: Path, expected: str, message: str, failures: list[str]) -> None:
    if not path.exists():
        failures.append(f"{message}: missing {path}")
        return
    actual = path.read_text(encoding="utf-8")
    if actual != expected:
        failures.append(f"{message}: expected {expected!r}, got {actual!r} at {path}")


def record_run(name: str, result: subprocess.CompletedProcess[str], failures: list[str]) -> None:
    if result.returncode != 0:
        failures.append(
            f"{name} exited {result.returncode}\n"
            f"STDOUT:\n{result.stdout}\n"
            f"STDERR:\n{result.stderr}"
        )


def main() -> int:
    failures: list[str] = []
    if WORK.exists():
        shutil.rmtree(WORK)

    alpha = WORK / "alpha"
    beta = WORK / "beta"
    explicit_subordinate = WORK / "explicit-subordinate"
    auto_subordinate = WORK / "auto-subordinate"

    write_file(alpha / "shared.txt", "alpha initial\n", 1_700_000_000)
    write_file(alpha / "folder" / "group.txt", "group nested file\n", 1_700_000_000)
    beta.mkdir(parents=True)

    initial = run_sync(f"+{alpha}", str(beta))
    record_run("initial canon sync", initial, failures)
    record_file(beta / "shared.txt", "alpha initial\n", "initial sync copied root file to beta", failures)
    record_file(
        beta / "folder" / "group.txt",
        "group nested file\n",
        "initial sync copied nested file to beta",
        failures,
    )

    write_file(alpha / "shared.txt", "alpha winner\n", 1_700_000_100)
    write_file(explicit_subordinate / "shared.txt", "explicit subordinate wrong newer\n", 1_700_000_200)
    write_file(explicit_subordinate / "extra-only.txt", "explicit extra\n", 1_700_000_200)
    write_file(auto_subordinate / "shared.txt", "auto subordinate wrong newest\n", 1_700_000_300)
    write_file(auto_subordinate / "extra-auto.txt", "auto extra\n", 1_700_000_300)

    subordinate_run = run_sync(str(alpha), str(beta), f"-{explicit_subordinate}", str(auto_subordinate))
    record_run("subordinate sync", subordinate_run, failures)

    for peer_name, peer in (
        ("alpha", alpha),
        ("beta", beta),
        ("explicit subordinate", explicit_subordinate),
        ("auto subordinate", auto_subordinate),
    ):
        record_file(
            peer / "shared.txt",
            "alpha winner\n",
            f"{peer_name} used the normal peers' decision instead of newer subordinate content",
            failures,
        )

    for peer_name, peer in (
        ("explicit subordinate", explicit_subordinate),
        ("auto subordinate", auto_subordinate),
    ):
        record_file(
            peer / "folder" / "group.txt",
            "group nested file\n",
            f"{peer_name} received a group file it lacked",
            failures,
        )
        snapshot = peer / ".kitchensync" / "snapshot.db"
        record(
            snapshot.exists() and snapshot.stat().st_size > 0,
            f"{peer_name} received an updated snapshot.db after subordinate sync",
            failures,
        )

    record(
        not (explicit_subordinate / "extra-only.txt").exists(),
        "explicit subordinate extra file was removed from the live tree",
        failures,
    )
    explicit_bak = bak_matches(explicit_subordinate, "extra-only.txt")
    record(
        bool(explicit_bak),
        "explicit subordinate extra file was displaced to .kitchensync/BAK",
        failures,
    )
    if explicit_bak:
        record_file(explicit_bak[-1], "explicit extra\n", "explicit subordinate BAK preserved displaced content", failures)

    record(
        not (auto_subordinate / "extra-auto.txt").exists(),
        "auto-subordinate extra file was removed from the live tree",
        failures,
    )
    auto_bak = bak_matches(auto_subordinate, "extra-auto.txt")
    record(
        bool(auto_bak),
        "auto-subordinate extra file was displaced to .kitchensync/BAK",
        failures,
    )
    if auto_bak:
        record_file(auto_bak[-1], "auto extra\n", "auto-subordinate BAK preserved displaced content", failures)

    write_file(explicit_subordinate / "shared.txt", "explicit later normal winner\n", 1_700_000_400)
    later_normal_run = run_sync(str(alpha), str(beta), str(explicit_subordinate))
    record_run("later normal sync", later_normal_run, failures)
    for peer_name, peer in (
        ("alpha", alpha),
        ("beta", beta),
        ("former subordinate", explicit_subordinate),
    ):
        record_file(
            peer / "shared.txt",
            "explicit later normal winner\n",
            f"{peer_name} accepted the former subordinate as a normal peer on a later run",
            failures,
        )

    if failures:
        print("FAILURES:")
        for index, failure in enumerate(failures, 1):
            print(f"{index}. {failure}")
        return 1

    print("03_subordinate-peer passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
