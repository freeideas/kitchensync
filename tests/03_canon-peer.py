#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Canon peer (03.15/03.16/03.17/03.40): canon state wins unconditionally."""

from __future__ import annotations

import os, shutil, subprocess, sys, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

TMP = Path(PROJECT) / "tmp" / "testks" / "03_canon-peer"


def _run(*peer_args, timeout=60):
    return subprocess.run(
        [str(UV), "run", "--script", str(BUILD_PY), "invoke-cli", PROJECT, *peer_args],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        text=True, encoding="utf-8",
        timeout=timeout,
    )


def _in_bak(peer_dir: Path, name: str) -> bool:
    bak_root = peer_dir / ".kitchensync" / "BAK"
    if not bak_root.exists():
        return False
    for ts_dir in bak_root.iterdir():
        if (ts_dir / name).exists():
            return True
    return False


def main() -> int:
    if TMP.exists():
        shutil.rmtree(TMP)
    TMP.mkdir(parents=True)

    failures = []

    try:
        # --- 03.15: Canon has file → copied to every other peer ---
        peer1 = TMP / "t0315" / "peer1"
        peer2 = TMP / "t0315" / "peer2"
        peer1.mkdir(parents=True)
        peer2.mkdir(parents=True)
        (peer1 / "canon.txt").write_text("canon-content")
        _run("+" + peer1.resolve().as_uri(), peer2.resolve().as_uri())
        got = (peer2 / "canon.txt").read_text() if (peer2 / "canon.txt").exists() else None
        print(f"[03.15] peer2/canon.txt content: {got!r}")
        if got != "canon-content":
            failures.append(f"03.15: expected 'canon-content' on peer2, got {got!r}")

        # --- 03.15 mod_time: canon file wins even when peer has a newer version ---
        peer1 = TMP / "t0315b" / "peer1"
        peer2 = TMP / "t0315b" / "peer2"
        peer1.mkdir(parents=True)
        peer2.mkdir(parents=True)
        # Run 1: establish snapshots with matching content
        (peer1 / "alpha.txt").write_text("v1-canon")
        (peer2 / "alpha.txt").write_text("v1-canon")
        _run("+" + peer1.resolve().as_uri(), peer2.resolve().as_uri())
        # peer2 now modifies its copy with a newer timestamp
        (peer2 / "alpha.txt").write_text("v2-modified")
        newer = time.time() + 100
        os.utime(peer2 / "alpha.txt", (newer, newer))
        # Run 2: canon peer1 (older, v1) should win over peer2's newer v2
        _run("+" + peer1.resolve().as_uri(), peer2.resolve().as_uri())
        got = (peer2 / "alpha.txt").read_text() if (peer2 / "alpha.txt").exists() else None
        print(f"[03.15b] peer2/alpha.txt after canon override of newer file: {got!r}")
        if got != "v1-canon":
            failures.append(f"03.15b: expected 'v1-canon' (canon wins over newer), got {got!r}")

        # --- 03.16: Canon lacks file → displaced to BAK/ on every other peer ---
        peer1 = TMP / "t0316" / "peer1"
        peer2 = TMP / "t0316" / "peer2"
        peer1.mkdir(parents=True)
        peer2.mkdir(parents=True)
        (peer2 / "extra.txt").write_text("should-be-displaced")
        _run("+" + peer1.resolve().as_uri(), peer2.resolve().as_uri())
        present = (peer2 / "extra.txt").exists()
        in_bak = _in_bak(peer2, "extra.txt")
        print(f"[03.16] extra.txt present={present}, in BAK={in_bak}")
        if present:
            failures.append("03.16: extra.txt still present on peer2 after canon-lacks displacement")
        if not in_bak:
            failures.append("03.16: extra.txt not found in peer2/.kitchensync/BAK/")

        # --- 03.17: Canon has directory → created on every other peer ---
        peer1 = TMP / "t0317" / "peer1"
        peer2 = TMP / "t0317" / "peer2"
        peer1.mkdir(parents=True)
        peer2.mkdir(parents=True)
        (peer1 / "subdir").mkdir()
        (peer1 / "subdir" / "keep.txt").write_text("inside")
        _run("+" + peer1.resolve().as_uri(), peer2.resolve().as_uri())
        created = (peer2 / "subdir").is_dir()
        print(f"[03.17] peer2/subdir created={created}")
        if not created:
            failures.append("03.17: subdir/ not created on peer2 by canon")

        # --- 03.40: Canon lacks directory → displaced to BAK/ on every other peer ---
        peer1 = TMP / "t0340" / "peer1"
        peer2 = TMP / "t0340" / "peer2"
        peer1.mkdir(parents=True)
        peer2.mkdir(parents=True)
        (peer2 / "extradir").mkdir()
        (peer2 / "extradir" / "file.txt").write_text("inside")
        _run("+" + peer1.resolve().as_uri(), peer2.resolve().as_uri())
        present = (peer2 / "extradir").exists()
        in_bak = _in_bak(peer2, "extradir")
        print(f"[03.40] extradir present={present}, in BAK={in_bak}")
        if present:
            failures.append("03.40: extradir/ still present on peer2 after canon-lacks displacement")
        if not in_bak:
            failures.append("03.40: extradir/ not found in peer2/.kitchensync/BAK/")

    finally:
        shutil.rmtree(TMP, ignore_errors=True)

    if failures:
        print("\nFAILURES:")
        for f in failures:
            print(f"  - {f}")
        return 1
    print("\nAll assertions passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
