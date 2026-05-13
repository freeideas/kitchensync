#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Existence-based directory decisions (03.9–03.13)."""

from __future__ import annotations

import os, shutil, subprocess, sys
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

BASE = Path(PROJECT) / "tmp" / "testks" / "03_directory-decisions"


def invoke(*args: str) -> subprocess.CompletedProcess:
    return subprocess.run(
        [str(UV), "run", "--script", str(BUILD_PY), "invoke-cli", PROJECT, *args],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        text=True, encoding="utf-8", timeout=60,
    )


def url(p: Path) -> str:
    return p.resolve().as_uri()


def find_in_bak(peer_root: Path, name: str) -> bool:
    """Return True if <name> exists under <peer_root>/.kitchensync/BAK/<any-ts>/."""
    bak = peer_root / ".kitchensync" / "BAK"
    if not bak.is_dir():
        return False
    return any((ts / name).exists() for ts in bak.iterdir())


def main() -> int:
    if BASE.exists():
        shutil.rmtree(BASE)
    BASE.mkdir(parents=True)

    failures: list[str] = []

    # ── 03.9: any contributing peer has a directory → created on every peer that lacks it ──
    a1, a2 = BASE / "09_p1", BASE / "09_p2"
    a1.mkdir(); a2.mkdir()
    (a1 / "subdir_a").mkdir()
    r = invoke(f"+{url(a1)}", url(a2))
    print(f"[03.9] sync exit={r.returncode}")
    ok = (a2 / "subdir_a").is_dir()
    print(f"[03.9] peer2 has subdir_a/: {ok}")
    if not ok:
        failures.append("03.9: directory not created on peer lacking it")

    # ── 03.10: all snapshot-row contributing peers tombstone → remaining peer displaced ──
    # Run 1 with 2 contributing + 1 subordinate peer to establish snapshots for all three.
    # Delete the directory from both contributing peers; keep it on the subordinate.
    # Run 2: both contributing peers tombstone → subordinate's copy displaced to BAK/.
    b1, b2, b3 = BASE / "10_p1", BASE / "10_p2", BASE / "10_p3"
    for d in (b1, b2, b3):
        d.mkdir()
        (d / "subdir_b").mkdir()
    r1 = invoke(f"+{url(b1)}", url(b2), f"-{url(b3)}")
    print(f"[03.10] run1 exit={r1.returncode}")
    shutil.rmtree(b1 / "subdir_b")
    shutil.rmtree(b2 / "subdir_b")
    r2 = invoke(url(b1), url(b2), f"-{url(b3)}")
    print(f"[03.10] run2 exit={r2.returncode}")
    absent = not (b3 / "subdir_b").is_dir()
    in_bak = find_in_bak(b3, "subdir_b")
    print(f"[03.10] subdir_b absent from root: {absent}, in BAK: {in_bak}")
    if not (absent and in_bak):
        failures.append(f"03.10: absent={absent} in_bak={in_bak}")

    # ── 03.11: contributing peer with no snapshot row does not block deletion ──
    # c2 must be a contributing peer (has snapshot.db) with no row for subdir_c.
    # Establish c2's snapshot via a separate sync that does not involve subdir_c,
    # then use c2 alongside c1 (tombstoning subdir_c) to show deletion still proceeds.
    c1 = BASE / "11_c1"
    c2 = BASE / "11_c2"
    c2_aux = BASE / "11_c2_aux"
    c3 = BASE / "11_c3"
    c1.mkdir(); c2.mkdir(); c2_aux.mkdir(); c3.mkdir()
    (c1 / "subdir_c").mkdir()
    (c3 / "subdir_c").mkdir()
    (c2 / "dummy.txt").write_text("x")
    # Give c2 a snapshot.db without any subdir_c row
    r_c2 = invoke(f"+{url(c2)}", url(c2_aux))
    print(f"[03.11] c2-snapshot-setup exit={r_c2.returncode}")
    # Give c1 and c3 snapshots that know subdir_c
    r_c1 = invoke(f"+{url(c1)}", f"-{url(c3)}")
    print(f"[03.11] c1/c3-snapshot-setup exit={r_c1.returncode}")
    shutil.rmtree(c1 / "subdir_c")
    # c1 tombstones; c2 (contributing, no subdir_c row) must not block; c3 displaced
    r = invoke(url(c1), url(c2), f"-{url(c3)}")
    print(f"[03.11] deletion-sync exit={r.returncode}")
    absent = not (c3 / "subdir_c").is_dir()
    in_bak = find_in_bak(c3, "subdir_c")
    print(f"[03.11] subdir_c absent from root: {absent}, in BAK: {in_bak}")
    if not (absent and in_bak):
        failures.append(f"03.11: absent={absent} in_bak={in_bak}")

    # ── 03.12: no contributing peer has directory live or in any snapshot → subordinate displaced ──
    # Canon peer (d1) has no subdir_d and no snapshot at all → no contributing peer has it.
    # Subordinate (d3) has subdir_d → displaced to BAK/.
    d1, d3 = BASE / "12_d1", BASE / "12_d3"
    d1.mkdir(); d3.mkdir()
    (d3 / "subdir_d").mkdir()
    r = invoke(f"+{url(d1)}", f"-{url(d3)}")
    print(f"[03.12] sync exit={r.returncode}")
    absent = not (d3 / "subdir_d").is_dir()
    in_bak = find_in_bak(d3, "subdir_d")
    print(f"[03.12] subdir_d absent from root: {absent}, in BAK: {in_bak}")
    if not (absent and in_bak):
        failures.append(f"03.12: absent={absent} in_bak={in_bak}")

    # ── 03.13: directory mod_times are not used to decide directory existence ──
    # Both peers have subdir_e after run 1; set wildly different mod_times, then sync again.
    # Neither directory should be displaced — mod_time does not determine directory fate.
    e1, e2 = BASE / "13_e1", BASE / "13_e2"
    e1.mkdir(); e2.mkdir()
    (e1 / "subdir_e").mkdir()
    r1 = invoke(f"+{url(e1)}", url(e2))
    print(f"[03.13] run1 exit={r1.returncode}")
    os.utime(e1 / "subdir_e", (946684800.0, 946684800.0))   # 2000-01-01
    os.utime(e2 / "subdir_e", (4102444800.0, 4102444800.0)) # 2099-12-31
    r2 = invoke(url(e1), url(e2))
    print(f"[03.13] run2 exit={r2.returncode}")
    e1_ok = (e1 / "subdir_e").is_dir()
    e2_ok = (e2 / "subdir_e").is_dir()
    print(f"[03.13] peer1 has subdir_e: {e1_ok}, peer2 has subdir_e: {e2_ok}")
    if not (e1_ok and e2_ok):
        failures.append(f"03.13: peer1_has={e1_ok} peer2_has={e2_ok}")

    if failures:
        print("\nFAILURES:")
        for f in failures:
            print(f"  - {f}")
        return 1
    print("\nAll assertions passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
