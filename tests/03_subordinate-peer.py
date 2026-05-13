#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Subordinate peer behavior (03.21–03.27): non-influencing, receives group outcome, snapshot updated."""

from __future__ import annotations

import os, shutil, subprocess, sys, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

TMP = Path(PROJECT) / "tmp" / "testks" / "03_subordinate-peer"


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
        # --- 03.21: Sub peer's files don't influence decisions among normal peers ---
        peer1 = TMP / "t0321" / "peer1"
        peer2 = TMP / "t0321" / "peer2"
        sub   = TMP / "t0321" / "sub"
        peer1.mkdir(parents=True)
        peer2.mkdir(parents=True)
        sub.mkdir(parents=True)

        # Establish snapshots on peer1 and peer2 with matching content.
        (peer1 / "shared.txt").write_text("original")
        (peer2 / "shared.txt").write_text("original")
        _run("+" + peer1.resolve().as_uri(), peer2.resolve().as_uri())

        # peer1 updates to a newer version; sub gets an even newer version that must be ignored.
        (peer1 / "shared.txt").write_text("from-peer1")
        t_newer = time.time() + 60
        os.utime(peer1 / "shared.txt", (t_newer, t_newer))
        (sub / "shared.txt").write_text("from-sub-ignore-me")
        t_even_newer = time.time() + 120
        os.utime(sub / "shared.txt", (t_even_newer, t_even_newer))

        _run(peer1.resolve().as_uri(), peer2.resolve().as_uri(), "-" + sub.resolve().as_uri())

        got_peer2 = (peer2 / "shared.txt").read_text() if (peer2 / "shared.txt").exists() else None
        print(f"[03.21] peer2/shared.txt: {got_peer2!r}")
        if got_peer2 != "from-peer1":
            failures.append(
                f"03.21: expected 'from-peer1' on peer2 (sub's even-newer version must not influence), "
                f"got {got_peer2!r}"
            )

        # --- 03.22 + 03.23: Sub loses extra files to BAK/; gains group-decided files ---
        peer1 = TMP / "t0322" / "peer1"
        sub   = TMP / "t0322" / "sub"
        peer1.mkdir(parents=True)
        sub.mkdir(parents=True)
        (peer1 / "group_file.txt").write_text("group-content")
        (sub / "extra.txt").write_text("sub-only")
        _run("+" + peer1.resolve().as_uri(), "-" + sub.resolve().as_uri())

        # 03.22: extra.txt not in group state → displaced to BAK/
        present = (sub / "extra.txt").exists()
        in_bak  = _in_bak(sub, "extra.txt")
        print(f"[03.22] extra.txt present={present}, in BAK={in_bak}")
        if present:
            failures.append("03.22: extra.txt still present on sub (should be displaced to BAK/)")
        if not in_bak:
            failures.append("03.22: extra.txt not found in sub/.kitchensync/BAK/")

        # 03.23: group_file.txt in group state but missing from sub → copied to sub
        got = (sub / "group_file.txt").read_text() if (sub / "group_file.txt").exists() else None
        print(f"[03.23] sub/group_file.txt: {got!r}")
        if got != "group-content":
            failures.append(f"03.23: expected 'group-content' on sub, got {got!r}")

        # --- 03.24: Peer with no snapshot.db is auto-subordinated (no - prefix required) ---
        peer1 = TMP / "t0324" / "peer1"
        peer2 = TMP / "t0324" / "peer2"
        peer1.mkdir(parents=True)
        peer2.mkdir(parents=True)
        (peer1 / "canonical.txt").write_text("from-canon")
        (peer2 / "extra_no_snap.txt").write_text("should-be-displaced")
        # peer2 has no .kitchensync/snapshot.db and no - prefix → must be auto-subordinated
        _run("+" + peer1.resolve().as_uri(), peer2.resolve().as_uri())

        canonical_present = (peer2 / "canonical.txt").exists()
        extra_present     = (peer2 / "extra_no_snap.txt").exists()
        extra_in_bak      = _in_bak(peer2, "extra_no_snap.txt")
        print(f"[03.24] canonical={canonical_present}, extra present={extra_present}, extra in BAK={extra_in_bak}")
        if not canonical_present:
            failures.append("03.24: canonical.txt not copied to snapshotless peer (auto-subordination failed)")
        if extra_present:
            failures.append("03.24: extra file still present on snapshotless peer (should be displaced as subordinate)")
        if not extra_in_bak:
            failures.append("03.24: extra file not in BAK/ on snapshotless peer (auto-subordination failed)")

        # --- 03.25: Sub peer's snapshot.db is updated and uploaded after run ---
        peer1 = TMP / "t0325" / "peer1"
        sub   = TMP / "t0325" / "sub"
        peer1.mkdir(parents=True)
        sub.mkdir(parents=True)
        (peer1 / "file.txt").write_text("content")
        _run("+" + peer1.resolve().as_uri(), "-" + sub.resolve().as_uri())

        snap = sub / ".kitchensync" / "snapshot.db"
        print(f"[03.25] sub snapshot.db exists: {snap.exists()}")
        if not snap.exists():
            failures.append("03.25: sub/.kitchensync/snapshot.db not present after subordinate sync")

        # --- 03.26: Peer promoted from sub to normal participates bidirectionally ---
        peer1 = TMP / "t0326" / "peer1"
        sub   = TMP / "t0326" / "sub"
        peer1.mkdir(parents=True)
        sub.mkdir(parents=True)
        (peer1 / "file.txt").write_text("v1")
        (sub / "file.txt").write_text("v1")

        # Run 1: sub is explicit subordinate → builds its snapshot record
        _run("+" + peer1.resolve().as_uri(), "-" + sub.resolve().as_uri())

        # sub modifies its file with a timestamp clearly newer than the snapshot
        (sub / "file.txt").write_text("from-sub-promoted")
        t_sub = time.time() + 60
        os.utime(sub / "file.txt", (t_sub, t_sub))

        # Run 2: sub without - → participates as normal bidirectional peer
        _run(peer1.resolve().as_uri(), sub.resolve().as_uri())

        got_peer1 = (peer1 / "file.txt").read_text() if (peer1 / "file.txt").exists() else None
        print(f"[03.26] peer1/file.txt after promoted sub: {got_peer1!r}")
        if got_peer1 != "from-sub-promoted":
            failures.append(
                f"03.26: expected 'from-sub-promoted' on peer1 (sub now normal peer, change must propagate), "
                f"got {got_peer1!r}"
            )

        # --- 03.27: More than one subordinate peer per run is allowed ---
        peer1 = TMP / "t0327" / "peer1"
        sub1  = TMP / "t0327" / "sub1"
        sub2  = TMP / "t0327" / "sub2"
        peer1.mkdir(parents=True)
        sub1.mkdir(parents=True)
        sub2.mkdir(parents=True)
        (peer1 / "group.txt").write_text("group-content")
        (sub1 / "sub1_only.txt").write_text("sub1")
        (sub2 / "sub2_only.txt").write_text("sub2")
        result = _run(
            "+" + peer1.resolve().as_uri(),
            "-" + sub1.resolve().as_uri(),
            "-" + sub2.resolve().as_uri(),
        )
        print(f"[03.27] two-sub run exit code: {result.returncode}")
        if result.returncode != 0:
            failures.append(
                f"03.27: expected exit 0 with two sub peers, got {result.returncode}; "
                f"stderr: {result.stderr!r}"
            )
        got_sub1 = (sub1 / "group.txt").exists()
        got_sub2 = (sub2 / "group.txt").exists()
        print(f"[03.27] sub1 has group.txt={got_sub1}, sub2 has group.txt={got_sub2}")
        if not got_sub1:
            failures.append("03.27: group.txt not copied to sub1 in two-sub run")
        if not got_sub2:
            failures.append("03.27: group.txt not copied to sub2 in two-sub run")

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
