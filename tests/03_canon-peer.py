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
import tempfile
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

JAVA = Path("C:/Users/human/Desktop/prjx/kitchensync/tools/compiler/jdk/bin/java.exe")
JAR = Path("C:/Users/human/Desktop/prjx/kitchensync/released/kitchensync.jar")

failures: list[str] = []


def check(condition: bool, message: str, detail: str = "") -> None:
    if condition:
        print(f"PASS: {message}")
    else:
        full = f"{message}\n  {detail}" if detail else message
        failures.append(full)
        print(f"FAIL: {full}")


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


def run_detail(result: subprocess.CompletedProcess[str]) -> str:
    return f"rc={result.returncode}\nstdout: {result.stdout}\nstderr: {result.stderr}"


def find_in_bak(peer_root: Path, name: str) -> list[Path]:
    bak = peer_root / ".kitchensync" / "BAK"
    if not bak.is_dir():
        return []
    return [p for p in bak.rglob("*") if p.name == name]


def main() -> None:
    with tempfile.TemporaryDirectory() as td:
        canon = Path(td) / "canon"
        peer_a = Path(td) / "peer-a"
        peer_b = Path(td) / "peer-b"
        for d in (canon, peer_a, peer_b):
            d.mkdir()

        # Phase 1: seed sync -- establish snapshot history on all peers.
        (canon / "conflict.txt").write_text("initial\n", encoding="utf-8")
        os.utime(str(canon / "conflict.txt"), (1_700_000_000, 1_700_000_000))
        (canon / "will-displace.txt").write_text("exists now\n", encoding="utf-8")
        (canon / "will-displace-dir").mkdir()
        (canon / "will-displace-dir" / "nested.txt").write_text("dir content\n", encoding="utf-8")

        seed = run_sync("+" + str(canon), str(peer_a), str(peer_b))
        check(
            seed.returncode == 0,
            "seed sync must succeed before canon-peer checks can be meaningful",
            run_detail(seed),
        )

        # Phase 2: mutate canon then re-sync with + to assert all four canon behaviors.
        #
        # 03.15: rewrite conflict.txt on canon with an OLDER mtime than the peer copies,
        #        then advance peer mtimes -- canon must still win regardless of mod_time
        #        and regardless of snapshot history.
        canon_content = "canon wins\n"
        (canon / "conflict.txt").write_text(canon_content, encoding="utf-8")
        os.utime(str(canon / "conflict.txt"), (1_600_000_000, 1_600_000_000))
        for peer in (peer_a, peer_b):
            (peer / "conflict.txt").write_text("peer newer content\n", encoding="utf-8")
            os.utime(str(peer / "conflict.txt"), (1_900_000_000, 1_900_000_000))

        # 03.16: remove will-displace.txt from canon; peers still have modified copies.
        (canon / "will-displace.txt").unlink()
        file_contents: dict[Path, str] = {}
        for peer in (peer_a, peer_b):
            content = f"{peer.name} changed after snapshot\n"
            (peer / "will-displace.txt").write_text(content, encoding="utf-8")
            os.utime(str(peer / "will-displace.txt"), (1_900_000_000, 1_900_000_000))
            file_contents[peer] = content

        # 03.17: add a new directory to canon; peers don't have it.
        (canon / "new-canon-dir").mkdir()
        (canon / "new-canon-dir" / "readme.txt").write_text("from canon\n", encoding="utf-8")

        # 03.40: remove will-displace-dir from canon; peers still have modified copies.
        shutil.rmtree(str(canon / "will-displace-dir"))
        for peer in (peer_a, peer_b):
            (peer / "will-displace-dir" / f"{peer.name}.txt").write_text(
                "peer directory changed after snapshot\n",
                encoding="utf-8",
            )

        result = run_sync("+" + str(canon), str(peer_a), str(peer_b))
        check(
            result.returncode == 0,
            "canon sync with snapshot history must exit 0",
            run_detail(result),
        )

        for peer in (peer_a, peer_b):
            # 03.15: peer file must carry canon's content despite its older mtime
            actual = (
                (peer / "conflict.txt").read_text(encoding="utf-8")
                if (peer / "conflict.txt").exists()
                else None
            )
            check(
                actual == canon_content,
                f"03.15: {peer.name}/conflict.txt must equal canon content regardless of mod_time",
                f"got {actual!r}",
            )

            # 03.16: file absent from canon must be displaced to BAK/ on peers
            check(
                not (peer / "will-displace.txt").exists(),
                f"03.16: {peer.name}/will-displace.txt must not remain at original path",
            )
            archived = find_in_bak(peer, "will-displace.txt")
            check(
                any(p.is_file() and p.read_text(encoding="utf-8") == file_contents[peer] for p in archived),
                f"03.16: modified will-displace.txt must be displaced under {peer.name}/.kitchensync/BAK/",
                f"BAK contents: {list((peer / '.kitchensync' / 'BAK').rglob('*')) if (peer / '.kitchensync' / 'BAK').is_dir() else 'no BAK dir'}",
            )

            # 03.17: directory present on canon must be created on peers that lack it
            check(
                (peer / "new-canon-dir").is_dir(),
                f"03.17: new-canon-dir must be created on {peer.name}",
            )

            # 03.40: directory absent from canon must be displaced to BAK/ on peers
            check(
                not (peer / "will-displace-dir").exists(),
                f"03.40: {peer.name}/will-displace-dir must not remain at original path",
            )
            archived_dir = find_in_bak(peer, "will-displace-dir")
            check(
                any((p / f"{peer.name}.txt").is_file() for p in archived_dir if p.is_dir()),
                f"03.40: modified will-displace-dir must be displaced under {peer.name}/.kitchensync/BAK/",
                f"BAK contents: {list((peer / '.kitchensync' / 'BAK').rglob('*')) if (peer / '.kitchensync' / 'BAK').is_dir() else 'no BAK dir'}",
            )


if __name__ == "__main__":
    main()

    if failures:
        print("FAILURES:", file=sys.stderr)
        for i, f in enumerate(failures, 1):
            print(f"{i}. {f}", file=sys.stderr)
        sys.exit(1)
    else:
        print("All checks passed.")
        sys.exit(0)
