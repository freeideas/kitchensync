#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Type-conflict resolution: file vs directory at same path (03.36, 03.37, 03.38)."""

from __future__ import annotations

import os, shutil, subprocess, sys
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

TMP = Path(PROJECT) / "tmp" / "testks" / "03_type-conflicts"


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
        # ── 03.36 + 03.37: no canon peer → file wins, directory displaced ───

        peer_a = TMP / "t0336" / "peer_a"
        peer_b = TMP / "t0336" / "peer_b"
        peer_a.mkdir(parents=True)
        peer_b.mkdir(parents=True)

        # Phase 1: establish snapshots so a no-canon sync is valid on phase 2
        (peer_a / "baseline.txt").write_text("base")
        (peer_b / "baseline.txt").write_text("base")
        _run("+" + peer_a.resolve().as_uri(), peer_b.resolve().as_uri())

        # Phase 2: introduce a file-vs-directory conflict, sync without canon
        (peer_a / "conflict").write_text("file-content")
        (peer_b / "conflict").mkdir()
        (peer_b / "conflict" / "inner.txt").write_text("dir-content")
        _run(peer_a.resolve().as_uri(), peer_b.resolve().as_uri())

        # 03.36: peer_b directory must be displaced to BAK/
        in_bak = _in_bak(peer_b, "conflict")
        print(f"[03.36] peer_b 'conflict' in BAK: {in_bak}")
        if not in_bak:
            failures.append("03.36: peer_b 'conflict' directory not displaced to BAK/")

        # 03.36: peer_b must no longer have 'conflict' as a directory
        conflict_b = peer_b / "conflict"
        still_dir = conflict_b.exists() and conflict_b.is_dir()
        print(f"[03.36] peer_b 'conflict' still a directory: {still_dir}")
        if still_dir:
            failures.append("03.36: peer_b 'conflict' is still a directory after sync")

        # 03.37: winning file propagated to peer_b
        is_file = conflict_b.exists() and conflict_b.is_file()
        content = conflict_b.read_text() if is_file else None
        print(f"[03.37] peer_b 'conflict' is file: {is_file}, content: {content!r}")
        if not is_file:
            failures.append("03.37: winning file not propagated to peer_b")
        elif content != "file-content":
            failures.append(f"03.37: peer_b 'conflict' has wrong content: {content!r}")

        # ── 03.38: canon peer's type wins ────────────────────────────────────

        peer_c = TMP / "t0338" / "peer_c"  # canon — has 'conflict' as directory
        peer_d = TMP / "t0338" / "peer_d"  # non-canon — has 'conflict' as file
        peer_c.mkdir(parents=True)
        peer_d.mkdir(parents=True)

        (peer_c / "conflict").mkdir()
        (peer_c / "conflict" / "inner.txt").write_text("canon-content")
        (peer_d / "conflict").write_text("file-content-d")

        _run("+" + peer_c.resolve().as_uri(), peer_d.resolve().as_uri())

        # 03.38: non-canon peer_d file must be displaced to BAK/
        in_bak_d = _in_bak(peer_d, "conflict")
        print(f"[03.38] peer_d 'conflict' file in BAK: {in_bak_d}")
        if not in_bak_d:
            failures.append("03.38: peer_d 'conflict' file not displaced to BAK/ when canon has directory")

        # 03.38: peer_d must now have 'conflict' as a directory (canon's type)
        conflict_d = peer_d / "conflict"
        is_dir_d = conflict_d.exists() and conflict_d.is_dir()
        print(f"[03.38] peer_d 'conflict' is directory: {is_dir_d}")
        if not is_dir_d:
            failures.append("03.38: peer_d 'conflict' is not a directory after canon type resolution")

        # 03.38: canon peer_c's 'conflict' directory must remain intact
        conflict_c = peer_c / "conflict"
        canon_intact = conflict_c.exists() and conflict_c.is_dir()
        print(f"[03.38] canon peer_c 'conflict' directory intact: {canon_intact}")
        if not canon_intact:
            failures.append("03.38: canon peer_c 'conflict' directory was unexpectedly displaced")

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
