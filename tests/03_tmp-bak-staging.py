#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""TMP cleanup and BAK displacement: 03.29, 03.31, 03.32, 03.33, 03.34, 03.89."""

from __future__ import annotations

import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = Path(os.environ.get("AITC_PROJECT", "."))

TMP = PROJECT / "tmp" / "testks" / "03_tmp-bak-staging"
TS_RE = re.compile(r"^\d{4}-\d{2}-\d{2}_\d{2}-\d{2}-\d{2}_\d{6}Z$")


def _run(*peer_args: str, timeout: int = 60) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(UV), "run", "--script", str(BUILD_PY), "invoke-cli", str(PROJECT), *peer_args],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        timeout=timeout,
    )


def _write(path: Path, text: str) -> None:
    path.write_text(text, encoding="utf-8", newline="")


def _read(path: Path) -> str | None:
    return path.read_text(encoding="utf-8") if path.exists() else None


def _metadata_names(parent: Path, kind: str) -> list[str]:
    root = parent / ".kitchensync" / kind
    if not root.is_dir():
        return []
    return sorted(path.name for path in root.iterdir() if path.is_dir())


def _bak_entry(parent: Path, name: str) -> Path | None:
    for ts in _metadata_names(parent, "BAK"):
        candidate = parent / ".kitchensync" / "BAK" / ts / name
        if candidate.exists():
            return candidate
    return None


def _has_tmp_uuid_dir(parent: Path) -> bool:
    tmp = parent / ".kitchensync" / "TMP"
    if not tmp.is_dir():
        return False
    for ts_dir in tmp.iterdir():
        if ts_dir.is_dir() and any(child.is_dir() for child in ts_dir.iterdir()):
            return True
    return False


def _peer(*parts: str) -> Path:
    peer = TMP.joinpath(*parts)
    peer.mkdir(parents=True)
    return peer


def main() -> int:
    if TMP.exists():
        shutil.rmtree(TMP)
    TMP.mkdir(parents=True)

    failures: list[str] = []

    try:
        # 03.28: The transient write to .kitchensync/TMP/<timestamp>/<uuid>/<basename>
        # is not reasonably testable through the CLI: successful copies remove the
        # per-transfer staging directory, and observing it during transfer would be
        # timing-dependent instrumentation the CLI does not expose.
        #
        # 03.30: Same-filesystem atomic rename from TMP is also not reasonably
        # testable through the CLI. After a successful run, a direct write and a
        # TMP-then-rename have the same observable final file.
        peer1 = _peer("t0389", "peer1")
        peer2 = _peer("t0389", "peer2")
        _write(peer1 / "alpha.txt", "copied-content")
        r = _run("+" + peer1.resolve().as_uri(), peer2.resolve().as_uri())
        copied = _read(peer2 / "alpha.txt")
        has_tmp_uuid = _has_tmp_uuid_dir(peer2)
        print(f"[03.89] copied={copied!r} tmp_uuid_dir_remains={has_tmp_uuid} exit={r.returncode}")
        if copied != "copied-content":
            failures.append(
                f"03.89 setup: expected copied-content at peer2/alpha.txt, got {copied!r} "
                f"(exit={r.returncode} stdout={r.stdout!r} stderr={r.stderr!r})"
            )
        if has_tmp_uuid:
            failures.append("03.89: per-transfer TMP <timestamp>/<uuid>/ directory remains after copy")

        # 03.29: Pre-existing destination file is displaced to BAK before replacement.
        peer1 = _peer("t0329", "peer1")
        peer2 = _peer("t0329", "peer2")
        _write(peer2 / "data.txt", "old-content")
        old_t = time.time() - 200
        os.utime(peer2 / "data.txt", (old_t, old_t))
        _write(peer1 / "data.txt", "new-content")
        r = _run("+" + peer1.resolve().as_uri(), peer2.resolve().as_uri())
        dest_content = _read(peer2 / "data.txt")
        bak = _bak_entry(peer2, "data.txt")
        bak_content = _read(bak) if bak else None
        print(f"[03.29] dest={dest_content!r} bak={bak_content!r} exit={r.returncode}")
        if dest_content != "new-content":
            failures.append(
                f"03.29: destination not replaced with winning file, got {dest_content!r} "
                f"(exit={r.returncode} stdout={r.stdout!r} stderr={r.stderr!r})"
            )
        if bak_content != "old-content":
            failures.append(f"03.29: displaced destination content not recoverable from BAK, got {bak_content!r}")

        # 03.31: Destination mod_time is set to the winning mod_time.
        peer1 = _peer("t0331", "peer1")
        peer2 = _peer("t0331", "peer2")
        _write(peer1 / "timed.txt", "timed-content")
        target_mtime = 1_700_000_000.0
        os.utime(peer1 / "timed.txt", (target_mtime, target_mtime))
        r = _run("+" + peer1.resolve().as_uri(), peer2.resolve().as_uri())
        dest = peer2 / "timed.txt"
        got_mtime = dest.stat().st_mtime if dest.exists() else None
        print(f"[03.31] dest_mtime={got_mtime} expected={target_mtime} exit={r.returncode}")
        if got_mtime is None:
            failures.append(f"03.31: timed.txt not copied (exit={r.returncode} stderr={r.stderr!r})")
        elif abs(got_mtime - target_mtime) > 1.0:
            failures.append(f"03.31: destination mtime {got_mtime} differs from winning mtime {target_mtime}")

        # 03.32: BAK is created at the affected entry's parent, not the sync root.
        # The analogous TMP parent is not reasonably testable through the CLI for
        # the same reason as 03.28: successful copies clean up the staging path.
        peer1 = _peer("t0332", "peer1")
        peer2 = _peer("t0332", "peer2")
        (peer1 / "sub").mkdir()
        (peer2 / "sub").mkdir()
        _write(peer1 / "sub" / "file.txt", "new-version")
        _write(peer2 / "sub" / "file.txt", "old-version")
        old_t = time.time() - 200
        os.utime(peer2 / "sub" / "file.txt", (old_t, old_t))
        r = _run("+" + peer1.resolve().as_uri(), peer2.resolve().as_uri())
        sub_bak = _bak_entry(peer2 / "sub", "file.txt")
        root_bak = _bak_entry(peer2, "file.txt")
        print(f"[03.32] sub_bak={sub_bak is not None} root_bak={root_bak is not None} exit={r.returncode}")
        if sub_bak is None:
            failures.append(
                f"03.32: displaced nested file not found under sub/.kitchensync/BAK/ "
                f"(exit={r.returncode} stdout={r.stdout!r} stderr={r.stderr!r})"
            )
        if root_bak is not None:
            failures.append("03.32: displaced nested file was aggregated at root .kitchensync/BAK/")

        # 03.33: Observable BAK timestamp directories use the required UTC format.
        # TMP timestamp directories created by successful copies are not reasonably
        # testable because 03.89 requires their per-transfer children to be cleaned up.
        bak_names = _metadata_names(peer2, "BAK") + _metadata_names(peer2 / "sub", "BAK")
        tmp_names = _metadata_names(peer2, "TMP") + _metadata_names(peer2 / "sub", "TMP")
        print(f"[03.33] bak_ts={bak_names} tmp_ts_observable_after_cleanup={tmp_names}")
        if not bak_names:
            failures.append("03.33: no BAK timestamp directory was produced to check")
        for name in bak_names:
            if not TS_RE.fullmatch(name):
                failures.append(f"03.33: BAK timestamp {name!r} does not match YYYY-MM-DD_HH-mm-ss_ffffffZ")
        for name in tmp_names:
            if not TS_RE.fullmatch(name):
                failures.append(f"03.33: TMP timestamp {name!r} does not match YYYY-MM-DD_HH-mm-ss_ffffffZ")

        # 03.34: The displaced directory's full subtree is preserved in BAK.
        # Whether the move was implemented as one rename is not reasonably
        # distinguishable from a recursive copy/delete through the CLI after the run.
        peer1 = _peer("t0334", "peer1")
        peer2 = _peer("t0334", "peer2")
        (peer2 / "mydir" / "sub").mkdir(parents=True)
        _write(peer2 / "mydir" / "a.txt", "file-a")
        _write(peer2 / "mydir" / "sub" / "b.txt", "file-b")
        r = _run("+" + peer1.resolve().as_uri(), peer2.resolve().as_uri())
        bak_dir = _bak_entry(peer2, "mydir")
        live_dir_exists = (peer2 / "mydir").exists()
        bak_a = _read(bak_dir / "a.txt") if bak_dir else None
        bak_b = _read(bak_dir / "sub" / "b.txt") if bak_dir else None
        print(
            f"[03.34] live_dir_exists={live_dir_exists} bak_dir={bak_dir is not None} "
            f"a={bak_a!r} sub_b={bak_b!r} exit={r.returncode}"
        )
        if live_dir_exists:
            failures.append("03.34: displaced directory still exists at original path")
        if bak_dir is None:
            failures.append(
                f"03.34: displaced directory not found in BAK "
                f"(exit={r.returncode} stdout={r.stdout!r} stderr={r.stderr!r})"
            )
        else:
            if bak_a != "file-a":
                failures.append(f"03.34: BAK/mydir/a.txt missing or wrong, got {bak_a!r}")
            if bak_b != "file-b":
                failures.append(f"03.34: BAK/mydir/sub/b.txt missing or wrong, got {bak_b!r}")

        # 03.35: Transfer-failure TMP cleanup and destination preservation are not
        # reasonably testable here. Triggering a mid-transfer failure requires
        # sabotaging I/O, permissions, or a transport stream during the copy, which
        # is outside the CLI's exposed controls and prohibited by the test philosophy.
    finally:
        shutil.rmtree(TMP, ignore_errors=True)

    if failures:
        print("\nFAILURES:")
        for failure in failures:
            print(f"  - {failure}")
        return 1
    print("\nAll assertions passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
