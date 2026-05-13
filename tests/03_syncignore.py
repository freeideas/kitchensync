#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""03_syncignore: .syncignore exclusion rules (03.40–03.46, 03.88, 03.94).
03.89 and 03.95 (read-failure warning/fallback) not tested: require making a file
unreadable, which sabotages the environment and is excluded by testing philosophy."""

from __future__ import annotations

import os, shutil, sqlite3, subprocess, sys
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

TMP = Path(PROJECT) / "tmp" / "testks" / "03_syncignore"


def _run(*peer_args, timeout=60):
    return subprocess.run(
        [str(UV), "run", "--script", str(BUILD_PY), "invoke-cli", PROJECT, *peer_args],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        text=True, encoding="utf-8",
        timeout=timeout,
    )


def _setup(name: str) -> tuple[Path, Path]:
    d = TMP / name
    if d.exists():
        shutil.rmtree(d)
    p1 = d / "peer1"
    p2 = d / "peer2"
    p1.mkdir(parents=True)
    p2.mkdir(parents=True)
    return p1, p2


def _in_bak(peer_dir: Path, name: str) -> bool:
    bak = peer_dir / ".kitchensync" / "BAK"
    if not bak.exists():
        return False
    for ts_dir in bak.iterdir():
        if (ts_dir / name).exists():
            return True
    return False


def _snapshot_mentions(db_path: Path, filename: str) -> bool:
    if not db_path.exists():
        return False
    try:
        with sqlite3.connect(str(db_path)) as conn:
            tables = [r[0] for r in conn.execute(
                "SELECT name FROM sqlite_master WHERE type='table'"
            ).fetchall()]
            for tbl in tables:
                for row in conn.execute(f"SELECT * FROM [{tbl}]").fetchall():
                    for cell in row:
                        if isinstance(cell, str) and filename in cell:
                            return True
    except Exception:
        pass
    return False


def main() -> int:
    if TMP.exists():
        shutil.rmtree(TMP)

    failures = []

    try:
        # --- 03.40: .syncignore file is itself synced using normal decision rules ---
        p1, p2 = _setup("t40")
        (p1 / ".syncignore").write_text("*.log\n")
        (p1 / "data.txt").write_text("hello")
        r = _run("+" + p1.resolve().as_uri(), p2.resolve().as_uri())
        si_synced = (p2 / ".syncignore").exists()
        print(f"[03.40] .syncignore synced to peer2: {si_synced} (exit={r.returncode})")
        if not si_synced:
            failures.append(
                f"03.40: .syncignore not synced to peer2 "
                f"(exit={r.returncode} stdout={r.stdout!r} stderr={r.stderr!r})"
            )

        # --- 03.41: *.ext pattern excludes files with that extension ---
        p1, p2 = _setup("t41")
        (p1 / ".syncignore").write_text("*.log\n")
        (p1 / "foo.log").write_text("logdata")
        (p1 / "foo.txt").write_text("textdata")
        r = _run("+" + p1.resolve().as_uri(), p2.resolve().as_uri())
        txt_ok = (p2 / "foo.txt").exists()
        log_absent = not (p2 / "foo.log").exists()
        print(f"[03.41] *.log excludes foo.log: txt_synced={txt_ok} log_absent={log_absent} (exit={r.returncode})")
        if not txt_ok:
            failures.append(
                f"03.41: foo.txt not synced — sync may not have run "
                f"(exit={r.returncode} stdout={r.stdout!r} stderr={r.stderr!r})"
            )
        if not log_absent:
            failures.append("03.41: foo.log was synced to peer2 despite *.log pattern in .syncignore")

        # --- 03.42: name/ pattern excludes a directory entry by name ---
        p1, p2 = _setup("t42")
        (p1 / ".syncignore").write_text("build/\n")
        (p1 / "build").mkdir()
        (p1 / "build" / "output.jar").write_text("binary")
        (p1 / "src").mkdir()
        (p1 / "src" / "Main.java").write_text("class Main {}")
        r = _run("+" + p1.resolve().as_uri(), p2.resolve().as_uri())
        src_ok = (p2 / "src" / "Main.java").exists()
        build_absent = not (p2 / "build").exists()
        print(f"[03.42] build/ excluded: src_synced={src_ok} build_absent={build_absent} (exit={r.returncode})")
        if not src_ok:
            failures.append(
                f"03.42: src/Main.java not synced — sync may not have run "
                f"(exit={r.returncode} stdout={r.stdout!r} stderr={r.stderr!r})"
            )
        if not build_absent:
            failures.append("03.42: build/ was synced to peer2 despite build/ pattern in .syncignore")

        # --- 03.43: **/name pattern excludes entries with that name in any subdirectory ---
        p1, p2 = _setup("t43")
        (p1 / ".syncignore").write_text("**/secret.txt\n")
        (p1 / "a" / "b").mkdir(parents=True)
        (p1 / "a" / "b" / "secret.txt").write_text("private")
        (p1 / "a" / "b" / "normal.txt").write_text("public")
        r = _run("+" + p1.resolve().as_uri(), p2.resolve().as_uri())
        normal_ok = (p2 / "a" / "b" / "normal.txt").exists()
        secret_absent = not (p2 / "a" / "b" / "secret.txt").exists()
        print(f"[03.43] **/secret.txt excluded: normal_synced={normal_ok} secret_absent={secret_absent} (exit={r.returncode})")
        if not normal_ok:
            failures.append(
                f"03.43: a/b/normal.txt not synced — sync may not have run "
                f"(exit={r.returncode} stdout={r.stdout!r} stderr={r.stderr!r})"
            )
        if not secret_absent:
            failures.append("03.43: a/b/secret.txt was synced despite **/secret.txt pattern in .syncignore")

        # --- 03.44: child .syncignore patterns combine with accumulated parent patterns ---
        p1, p2 = _setup("t44")
        (p1 / ".syncignore").write_text("*.log\n")
        (p1 / "sub").mkdir()
        (p1 / "sub" / ".syncignore").write_text("*.tmp\n")
        (p1 / "sub" / "file.log").write_text("log")
        (p1 / "sub" / "file.tmp").write_text("tmp")
        (p1 / "sub" / "file.txt").write_text("text")
        r = _run("+" + p1.resolve().as_uri(), p2.resolve().as_uri())
        txt_ok = (p2 / "sub" / "file.txt").exists()
        log_absent = not (p2 / "sub" / "file.log").exists()
        tmp_absent = not (p2 / "sub" / "file.tmp").exists()
        print(
            f"[03.44] child+parent rules: txt_synced={txt_ok} "
            f"log_absent={log_absent} tmp_absent={tmp_absent} (exit={r.returncode})"
        )
        if not txt_ok:
            failures.append(
                f"03.44: sub/file.txt not synced — sync may not have run "
                f"(exit={r.returncode} stdout={r.stdout!r} stderr={r.stderr!r})"
            )
        if not log_absent:
            failures.append("03.44: sub/file.log was synced despite root *.log pattern")
        if not tmp_absent:
            failures.append("03.44: sub/file.tmp was synced despite child *.tmp pattern")

        # --- 03.45: ignored entry not copied or displaced on any peer ---
        # "not copied": excluded.log only on peer1 → not on peer2 after sync
        # "not displaced": keep.log only on peer2 → not moved to BAK/ after sync with +peer1
        p1_t45, p2_t45 = _setup("t45")
        (p1_t45 / ".syncignore").write_text("*.log\n")
        (p1_t45 / "anchor.txt").write_text("anchor")
        (p1_t45 / "excluded.log").write_text("logdata")
        (p2_t45 / "keep.log").write_text("peer2-log")
        r = _run("+" + p1_t45.resolve().as_uri(), p2_t45.resolve().as_uri())
        anchor_ok = (p2_t45 / "anchor.txt").exists()
        not_copied = not (p2_t45 / "excluded.log").exists()
        not_displaced = (p2_t45 / "keep.log").exists() and not _in_bak(p2_t45, "keep.log")
        print(
            f"[03.45] not_copied={not_copied} not_displaced={not_displaced} "
            f"anchor_synced={anchor_ok} (exit={r.returncode})"
        )
        if not anchor_ok:
            failures.append(
                f"03.45: anchor.txt not synced — sync may not have run "
                f"(exit={r.returncode} stdout={r.stdout!r} stderr={r.stderr!r})"
            )
        if not not_copied:
            failures.append("03.45: excluded.log was copied to peer2 despite *.log pattern")
        if not not_displaced:
            failures.append("03.45: keep.log was displaced from peer2 despite *.log pattern")

        # --- 03.94: ignored entry produces no snapshot-row on any peer ---
        snap_db = p2_t45 / ".kitchensync" / "snapshot.db"
        ex_absent = not _snapshot_mentions(snap_db, "excluded.log")
        keep_absent = not _snapshot_mentions(snap_db, "keep.log")
        print(f"[03.94] snapshot no row for ignored: excluded_absent={ex_absent} keep_absent={keep_absent}")
        if not ex_absent:
            failures.append("03.94: excluded.log appears in peer2 snapshot.db despite being ignored")
        if not keep_absent:
            failures.append("03.94: keep.log appears in peer2 snapshot.db despite being ignored")

        # --- 03.46: .syncignore is never excluded by accumulated ignore patterns ---
        p1, p2 = _setup("t46")
        # Pattern in .syncignore matches the .syncignore file itself
        (p1 / ".syncignore").write_text(".syncignore\n")
        (p1 / "regular.txt").write_text("hello")
        r = _run("+" + p1.resolve().as_uri(), p2.resolve().as_uri())
        si_synced = (p2 / ".syncignore").exists()
        reg_ok = (p2 / "regular.txt").exists()
        print(f"[03.46] .syncignore never excluded by own pattern: si_synced={si_synced} reg_synced={reg_ok} (exit={r.returncode})")
        if not reg_ok:
            failures.append(
                f"03.46: regular.txt not synced — sync may not have run "
                f"(exit={r.returncode} stdout={r.stdout!r} stderr={r.stderr!r})"
            )
        if not si_synced:
            failures.append(
                "03.46: .syncignore not synced to peer2; it must be synced even when a pattern matches it"
            )

        # --- 03.88: !pattern in child .syncignore un-ignores parent-ignored entries ---
        p1, p2 = _setup("t88")
        (p1 / ".syncignore").write_text("*.log\n")
        (p1 / "sub").mkdir()
        (p1 / "sub" / ".syncignore").write_text("!important.log\n")
        (p1 / "root.log").write_text("root-log")
        (p1 / "sub" / "important.log").write_text("important")
        (p1 / "sub" / "other.log").write_text("other")
        (p1 / "sub" / "file.txt").write_text("text")
        r = _run("+" + p1.resolve().as_uri(), p2.resolve().as_uri())
        important_ok = (p2 / "sub" / "important.log").exists()
        txt_ok = (p2 / "sub" / "file.txt").exists()
        root_absent = not (p2 / "root.log").exists()
        other_absent = not (p2 / "sub" / "other.log").exists()
        print(
            f"[03.88] !pattern un-ignores: important_synced={important_ok} "
            f"txt_synced={txt_ok} root_absent={root_absent} other_absent={other_absent} "
            f"(exit={r.returncode})"
        )
        if not txt_ok:
            failures.append(
                f"03.88: sub/file.txt not synced — sync may not have run "
                f"(exit={r.returncode} stdout={r.stdout!r} stderr={r.stderr!r})"
            )
        if not important_ok:
            failures.append("03.88: sub/important.log not synced despite !important.log in child .syncignore")
        if not root_absent:
            failures.append("03.88: root.log was synced despite *.log pattern in root .syncignore")
        if not other_absent:
            failures.append("03.88: sub/other.log was synced despite *.log pattern (child only un-ignores important.log)")

        # --- 03.89: warning logged when .syncignore read fails ---
        # not reasonably testable: requires making a file unreadable (sabotages environment)

        # --- 03.95: entries filtered by parent rules when .syncignore read fails ---
        # not reasonably testable: requires making a file unreadable (sabotages environment)

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
