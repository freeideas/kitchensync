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


PROJECT_DIR = Path(".")
JAVA = Path("tools/compiler/jdk/bin/java")
JAR = Path("released/kitchensync.jar")
WORK_DIR = Path("tests/.tmp_03_canon_peer")


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")


def read_text(path: Path) -> str | None:
    try:
        return path.read_text(encoding="utf-8")
    except FileNotFoundError:
        return None


def run_sync(*peers: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(JAVA), "-jar", str(JAR), *peers],
        cwd=PROJECT_DIR,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=60,
        check=False,
    )


def result_summary(result: subprocess.CompletedProcess[str]) -> str:
    out = result.stdout.strip()
    err = result.stderr.strip()
    parts = [f"exit={result.returncode}"]
    if out:
        parts.append(f"stdout={out[-800:]}")
    if err:
        parts.append(f"stderr={err[-800:]}")
    return "; ".join(parts)


def bak_matches(peer: Path, relative: str, expected_text: str | None = None) -> list[Path]:
    matches = sorted((peer / ".kitchensync" / "BAK").glob(f"*/{relative}"))
    if expected_text is None:
        return matches
    return [path for path in matches if read_text(path) == expected_text]


def check(failures: list[str], condition: bool, message: str) -> None:
    if not condition:
        failures.append(message)


def main() -> int:
    failures: list[str] = []
    canon = WORK_DIR / "canon"
    peer_a = WORK_DIR / "peer_a"
    peer_b = WORK_DIR / "peer_b"
    peers = (peer_a, peer_b)

    if WORK_DIR.exists():
        shutil.rmtree(WORK_DIR)
    canon.mkdir(parents=True)
    for peer in peers:
        peer.mkdir(parents=True)

    initial_shared = "initial shared file\n"
    initial_obsolete = "initial obsolete file\n"
    initial_obsolete_dir = "initial obsolete directory file\n"
    canon_shared = "canon\n"
    peer_shared = "peer has a newer, larger file that canon must overwrite\n"
    peer_obsolete = "peer-only file canon lacks and must displace\n"
    peer_obsolete_dir = "peer-only directory content canon lacks and must displace\n"
    canon_new_dir = "directory created from canon\n"

    try:
        write_text(canon / "shared.txt", initial_shared)
        write_text(canon / "obsolete.txt", initial_obsolete)
        write_text(canon / "obsolete_dir" / "inside.txt", initial_obsolete_dir)

        seed = run_sync(f"+{canon}", *(str(peer) for peer in peers))
        check(
            failures,
            seed.returncode == 0,
            "initial canon sync should succeed so every peer has snapshot history; "
            + result_summary(seed),
        )

        write_text(canon / "shared.txt", canon_shared)
        os.utime(canon / "shared.txt", (946684800, 946684800))
        for index, peer in enumerate(peers, start=1):
            write_text(peer / "shared.txt", f"{peer_shared} peer {index}\n")
            os.utime(peer / "shared.txt", (4102444800 + index, 4102444800 + index))

        (canon / "obsolete.txt").unlink(missing_ok=True)
        for index, peer in enumerate(peers, start=1):
            write_text(peer / "obsolete.txt", f"{peer_obsolete} peer {index}\n")
            os.utime(peer / "obsolete.txt", (4102444800 + index, 4102444800 + index))

        shutil.rmtree(canon / "obsolete_dir", ignore_errors=True)
        for index, peer in enumerate(peers, start=1):
            write_text(peer / "obsolete_dir" / "inside.txt", f"{peer_obsolete_dir} peer {index}\n")

        write_text(canon / "new_dir" / "inside.txt", canon_new_dir)
        for peer in peers:
            shutil.rmtree(peer / "new_dir", ignore_errors=True)

        canon_run = run_sync(f"+{canon}", *(str(peer) for peer in peers))
        check(
            failures,
            canon_run.returncode == 0,
            "canon sync should succeed after conflicting peer changes; "
            + result_summary(canon_run),
        )

        for index, peer in enumerate(peers, start=1):
            check(
                failures,
                read_text(peer / "shared.txt") == canon_shared,
                f"03.15 canon file should overwrite peer {index} even when the peer file is newer, larger, and has snapshot history",
            )
            check(
                failures,
                not (peer / "obsolete.txt").exists(),
                f"03.16 peer {index} file should be removed from its original path when the canon peer lacks that file",
            )
            check(
                failures,
                bool(bak_matches(peer, "obsolete.txt", f"{peer_obsolete} peer {index}\n")),
                f"03.16 displaced peer {index} file should be recoverable under that peer's .kitchensync/BAK with its peer content",
            )
            check(
                failures,
                (peer / "new_dir").is_dir() and read_text(peer / "new_dir" / "inside.txt") == canon_new_dir,
                f"03.17 canon directory should be created on peer {index} when that peer lacks it",
            )
            check(
                failures,
                not (peer / "obsolete_dir").exists(),
                f"03.40 peer {index} directory should be removed from its original path when the canon peer lacks that directory",
            )
            check(
                failures,
                bool(bak_matches(peer, "obsolete_dir/inside.txt", f"{peer_obsolete_dir} peer {index}\n")),
                f"03.40 displaced peer {index} directory should be recoverable under that peer's .kitchensync/BAK with its contents",
            )
    finally:
        shutil.rmtree(WORK_DIR, ignore_errors=True)

    if failures:
        print("FAILURES:", file=sys.stderr)
        for index, failure in enumerate(failures, start=1):
            print(f"{index}. {failure}", file=sys.stderr)
        return 1

    print("03_canon-peer passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
