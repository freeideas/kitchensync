#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Built-in exclusions: .kitchensync/, symlinks, special files, .git/ default, !.git/ override."""

from __future__ import annotations

import os, shutil, subprocess, sys
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

TMP = Path(PROJECT) / "tmp" / "testks" / "03_builtin-excludes"


def sync(peer1: Path, peer2: Path) -> subprocess.CompletedProcess:
    url1 = "+" + peer1.resolve().as_uri()
    url2 = peer2.resolve().as_uri()
    return subprocess.run(
        [str(UV), "run", "--script", str(BUILD_PY), "invoke-cli", PROJECT, url1, url2],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        text=True, encoding="utf-8",
        timeout=60,
    )


def setup(name: str) -> tuple[Path, Path]:
    d = TMP / name
    if d.exists():
        shutil.rmtree(d)
    peer1 = d / "peer1"
    peer2 = d / "peer2"
    peer1.mkdir(parents=True)
    peer2.mkdir(parents=True)
    return peer1, peer2


def main() -> int:
    if TMP.exists():
        shutil.rmtree(TMP)

    failures = []

    # --- 03.47: .kitchensync/ never synced, cannot be re-enabled by .syncignore ---
    peer1, peer2 = setup("t47")
    (peer1 / "regular.txt").write_text("hello")
    (peer1 / ".syncignore").write_text("!.kitchensync/\n")
    (peer1 / ".kitchensync").mkdir()
    (peer1 / ".kitchensync" / "sentinel.txt").write_text("should not sync")
    (peer1 / "sub").mkdir()
    (peer1 / "sub" / ".kitchensync").mkdir()
    (peer1 / "sub" / ".kitchensync" / "sentinel.txt").write_text("should not sync")
    r = sync(peer1, peer2)
    synced = (peer2 / "regular.txt").exists()
    root_leaked = (peer2 / ".kitchensync" / "sentinel.txt").exists()
    nested_leaked = (peer2 / "sub" / ".kitchensync" / "sentinel.txt").exists()
    print(
        f"[03.47] .kitchensync/ not synced: regular_synced={synced} "
        f"root_sentinel_leaked={root_leaked} nested_sentinel_leaked={nested_leaked} exit={r.returncode}"
    )
    if not synced:
        failures.append(f"03.47: regular.txt not synced — sync may not have run (exit={r.returncode} stdout={r.stdout!r} stderr={r.stderr!r})")
    if root_leaked:
        failures.append("03.47: .kitchensync/sentinel.txt was synced to peer2 (must never sync, even with !.kitchensync/ in .syncignore)")
    if nested_leaked:
        failures.append("03.47: sub/.kitchensync/sentinel.txt was synced to peer2 (must never sync, even with !.kitchensync/ in .syncignore)")

    # --- 03.48: Symlinks never synced, cannot be re-enabled by .syncignore ---
    peer1, peer2 = setup("t48")
    (peer1 / "regular.txt").write_text("hello")
    (peer1 / "target.txt").write_text("target content")
    (peer1 / "targetdir").mkdir()
    (peer1 / "targetdir" / "nested.txt").write_text("target content")
    (peer1 / "mylink").symlink_to("target.txt")
    (peer1 / "dirlink").symlink_to("targetdir", target_is_directory=True)
    (peer1 / ".syncignore").write_text("!mylink\n!dirlink\n")
    r = sync(peer1, peer2)
    synced = (peer2 / "regular.txt").exists()
    file_link_leaked = (peer2 / "mylink").is_symlink() or (peer2 / "mylink").exists()
    dir_link_leaked = (peer2 / "dirlink").is_symlink() or (peer2 / "dirlink").exists()
    print(
        f"[03.48] symlinks not synced: regular_synced={synced} "
        f"file_link_leaked={file_link_leaked} dir_link_leaked={dir_link_leaked} exit={r.returncode}"
    )
    if not synced:
        failures.append(f"03.48: regular.txt not synced — sync may not have run (exit={r.returncode} stdout={r.stdout!r} stderr={r.stderr!r})")
    if file_link_leaked:
        failures.append("03.48: symlink 'mylink' was synced to peer2 (must never sync, even with !mylink in .syncignore)")
    if dir_link_leaked:
        failures.append("03.48: directory symlink 'dirlink' was synced to peer2 (must never sync, even with !dirlink in .syncignore)")

    # --- 03.49: Special files (FIFO) never synced, cannot be re-enabled by .syncignore ---
    peer1, peer2 = setup("t49")
    (peer1 / "regular.txt").write_text("hello")
    os.mkfifo(peer1 / "myfifo")
    (peer1 / ".syncignore").write_text("!myfifo\n")
    r = sync(peer1, peer2)
    synced = (peer2 / "regular.txt").exists()
    leaked = (peer2 / "myfifo").exists()
    print(f"[03.49] FIFO not synced: regular_synced={synced} fifo_leaked={leaked} exit={r.returncode}")
    if not synced:
        failures.append(f"03.49: regular.txt not synced — sync may not have run (exit={r.returncode} stdout={r.stdout!r} stderr={r.stderr!r})")
    if leaked:
        failures.append("03.49: FIFO 'myfifo' was synced to peer2 (must never sync, even with !myfifo in .syncignore)")

    # --- 03.50: .git/ excluded by default ---
    peer1, peer2 = setup("t50")
    (peer1 / "regular.txt").write_text("hello")
    (peer1 / ".git").mkdir()
    (peer1 / ".git" / "config").write_text("[core]\n\trepositoryformatversion = 0\n")
    (peer1 / "sub").mkdir()
    (peer1 / "sub" / ".git").mkdir()
    (peer1 / "sub" / ".git" / "config").write_text("[core]\n\trepositoryformatversion = 0\n")
    r = sync(peer1, peer2)
    synced = (peer2 / "regular.txt").exists()
    root_git_leaked = (peer2 / ".git").exists()
    nested_git_leaked = (peer2 / "sub" / ".git").exists()
    print(
        f"[03.50] .git/ excluded by default: regular_synced={synced} "
        f"root_git_leaked={root_git_leaked} nested_git_leaked={nested_git_leaked} exit={r.returncode}"
    )
    if not synced:
        failures.append(f"03.50: regular.txt not synced — sync may not have run (exit={r.returncode} stdout={r.stdout!r} stderr={r.stderr!r})")
    if root_git_leaked:
        failures.append("03.50: .git/ was synced to peer2 (must be excluded by default)")
    if nested_git_leaked:
        failures.append("03.50: sub/.git/ was synced to peer2 (must be excluded by default)")

    # --- 03.51: !.git/ in .syncignore re-enables .git/ sync ---
    peer1, peer2 = setup("t51")
    (peer1 / "regular.txt").write_text("hello")
    (peer1 / ".syncignore").write_text("!.git/\n")
    (peer1 / ".git").mkdir()
    (peer1 / ".git" / "config").write_text("[core]\n\trepositoryformatversion = 0\n")
    (peer1 / "sub").mkdir()
    (peer1 / "sub" / ".git").mkdir()
    (peer1 / "sub" / ".git" / "config").write_text("[core]\n\trepositoryformatversion = 0\n")
    r = sync(peer1, peer2)
    synced = (peer2 / "regular.txt").exists()
    git_synced = (peer2 / ".git" / "config").exists()
    nested_git_synced = (peer2 / "sub" / ".git" / "config").exists()
    print(
        f"[03.51] !.git/ re-enables .git/ sync: regular_synced={synced} "
        f"git_synced={git_synced} nested_git_synced={nested_git_synced} exit={r.returncode}"
    )
    if not synced:
        failures.append(f"03.51: regular.txt not synced — sync may not have run (exit={r.returncode} stdout={r.stdout!r} stderr={r.stderr!r})")
    if not git_synced:
        failures.append(f"03.51: .git/config not synced despite !.git/ in .syncignore (stdout={r.stdout!r} stderr={r.stderr!r})")
    if not nested_git_synced:
        failures.append(f"03.51: sub/.git/config not synced despite parent !.git/ in .syncignore (stdout={r.stdout!r} stderr={r.stderr!r})")

    if failures:
        print("\nFAILURES:")
        for f in failures:
            print(f"  - {f}")
        return 1
    print("\nAll assertions passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
