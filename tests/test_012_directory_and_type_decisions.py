# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""
End-to-end tests for reqs/012_directory-and-type-decisions.md

Covers every REQ_ID 012.1 through 012.17 using local file:// peers.
All failures are collected; the script exits 1 only when at least one check failed.
"""

import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

WORKSPACE = Path("/home/ace/Desktop/prjx/kitchensync")
EXE = WORKSPACE / "released" / "kitchensync.exe"

_failures: list[str] = []


def _check(condition: bool, msg: str) -> None:
    if condition:
        print(f"PASS: {msg}")
    else:
        _failures.append(msg)
        print(f"FAIL: {msg}")


def _run(*args: str, timeout: int = 90) -> tuple[int, str, str]:
    result = subprocess.run(
        [str(EXE), *args],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=timeout,
    )
    return result.returncode, result.stdout, result.stderr


def _write(path: Path, content: str = "data\n") -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


def _has_bak(peer_root: Path, basename: str) -> bool:
    """Return True if BAK/<timestamp>/<basename> exists under peer_root/.kitchensync/BAK/."""
    bak_root = peer_root / ".kitchensync" / "BAK"
    if not bak_root.is_dir():
        return False
    for ts_dir in bak_root.iterdir():
        if ts_dir.is_dir() and (ts_dir / basename).exists():
            return True
    return False


# ---------------------------------------------------------------------------
# 012.6: Canon peer has directory -> created on every peer that lacks it
# ---------------------------------------------------------------------------
def test_012_6() -> None:
    print("\n--- 012.6: canon has dir, created on every peer that lacks it ---")
    with tempfile.TemporaryDirectory(prefix="ks012_6_") as tmp:
        a, b = Path(tmp) / "A", Path(tmp) / "B"
        a.mkdir()
        b.mkdir()

        # A (canon) has subdir/ with a file; B has nothing
        _write(a / "subdir" / "note.txt", "hello\n")

        rc, out, err = _run(f"+{a}", str(b))
        _check(rc == 0, "012.6: sync exits 0")
        _check((b / "subdir").is_dir(), "012.6: subdir/ created on B by canon A")


# ---------------------------------------------------------------------------
# 012.7: Canon peer lacks directory -> displaced to BAK/ on every peer that has it
# (012.2 displace side: displacement is existence-based, not mod_time-based)
# ---------------------------------------------------------------------------
def test_012_7() -> None:
    print("\n--- 012.7: canon lacks dir, displaced to BAK/ on every peer that has it ---")
    with tempfile.TemporaryDirectory(prefix="ks012_7_") as tmp:
        a, b = Path(tmp) / "A", Path(tmp) / "B"
        a.mkdir()
        b.mkdir()

        # A (canon) has only seed.txt; B has subdir/ with content
        _write(a / "seed.txt", "seed\n")
        _write(b / "subdir" / "notes.txt", "notes\n")

        rc, out, err = _run(f"+{a}", str(b))
        _check(rc == 0, "012.7: sync exits 0")
        _check(not (b / "subdir").exists(), "012.7: subdir/ no longer live on B")
        _check(_has_bak(b, "subdir"), "012.7: subdir/ displaced to BAK/ on B")


# ---------------------------------------------------------------------------
# 012.8 + 012.9: Canon has file at path; conflicting directory displaced; file synced
# ---------------------------------------------------------------------------
def test_012_8_9() -> None:
    print("\n--- 012.8+012.9: canon has file, conflicting dir displaced, file synced ---")
    with tempfile.TemporaryDirectory(prefix="ks012_89_") as tmp:
        a, b = Path(tmp) / "A", Path(tmp) / "B"
        a.mkdir()
        b.mkdir()

        # A (canon) has 'item' as a regular file; B has 'item' as a directory
        _write(a / "item", "canon file content\n")
        (b / "item").mkdir()
        _write(b / "item" / "child.txt", "child\n")

        rc, out, err = _run(f"+{a}", str(b))
        _check(rc == 0, "012.8+9: sync exits 0")
        _check(_has_bak(b, "item"), "012.8: conflicting directory on B displaced to BAK/")
        _check((b / "item").is_file(), "012.9: canon file synced to B")
        if (b / "item").is_file():
            _check(
                (b / "item").read_text(encoding="utf-8") == "canon file content\n",
                "012.9: synced file has correct content on B",
            )


# ---------------------------------------------------------------------------
# 012.10 + 012.11: Canon has directory at path; conflicting file displaced; dir created
# ---------------------------------------------------------------------------
def test_012_10_11() -> None:
    print("\n--- 012.10+012.11: canon has dir, conflicting file displaced, dir created ---")
    with tempfile.TemporaryDirectory(prefix="ks012_1011_") as tmp:
        a, b = Path(tmp) / "A", Path(tmp) / "B"
        a.mkdir()
        b.mkdir()

        # A (canon) has 'item' as a directory; B has 'item' as a regular file
        (a / "item").mkdir()
        _write(a / "item" / "content.txt", "in dir\n")
        _write(b / "item", "wrong type file\n")

        rc, out, err = _run(f"+{a}", str(b))
        _check(rc == 0, "012.10+11: sync exits 0")
        _check(_has_bak(b, "item"), "012.10: conflicting file on B displaced to BAK/")
        _check((b / "item").is_dir(), "012.11: directory 'item' created on B")


# ---------------------------------------------------------------------------
# 012.12: Canon lacks path -> path displaced to BAK/ on every peer that has it
# Tests both a file form (peer B) and a directory form (peer C) simultaneously.
# ---------------------------------------------------------------------------
def test_012_12() -> None:
    print("\n--- 012.12: canon lacks path, displaced on every peer that has it ---")
    with tempfile.TemporaryDirectory(prefix="ks012_12_") as tmp:
        a, b, c = Path(tmp) / "A", Path(tmp) / "B", Path(tmp) / "C"
        a.mkdir()
        b.mkdir()
        c.mkdir()

        # A (canon) has seed.txt only; B has 'item' as file; C has 'item' as directory
        _write(a / "seed.txt", "seed\n")
        _write(b / "item", "file form\n")
        (c / "item").mkdir()
        _write(c / "item" / "sub.txt", "sub\n")

        rc, out, err = _run(f"+{a}", str(b), str(c))
        _check(rc == 0, "012.12: sync exits 0")
        _check(not (b / "item").exists(), "012.12: file 'item' no longer live on B")
        _check(_has_bak(b, "item"), "012.12: file 'item' displaced to BAK/ on B")
        _check(not (c / "item").exists(), "012.12: dir 'item' no longer live on C")
        _check(_has_bak(c, "item"), "012.12: dir 'item' displaced to BAK/ on C")


# ---------------------------------------------------------------------------
# 012.1 + 012.2:
#   012.1: When any contributing peer has a directory live, it is created on
#          every active peer that lacks it.
#   012.2: Directory create and displace outcomes are unchanged by mod_time.
#
# Two-run setup: run 1 creates snapshot history on both peers; run 2 (no
# canon) tests that a contributing peer's live directory triggers creation
# even when that directory has a very old (epoch) mod_time.
# ---------------------------------------------------------------------------
def test_012_1_2() -> None:
    print("\n--- 012.1+012.2: contributing peer has dir -> created; mod_time irrelevant ---")
    with tempfile.TemporaryDirectory(prefix="ks012_12b_") as tmp:
        a, b = Path(tmp) / "A", Path(tmp) / "B"
        a.mkdir()
        b.mkdir()

        # A has seed.txt and data/; B has seed.txt only
        _write(a / "seed.txt", "seed\n")
        _write(a / "data" / "f.txt", "f\n")
        _write(b / "seed.txt", "seed\n")

        # Run 1: +A B -- bootstrap snapshots; B gets data/ created (A is canon)
        rc, _, _ = _run(f"+{a}", str(b))
        if rc != 0:
            _failures.append("012.1+2: run 1 failed to establish snapshot state")
            return

        # Remove data/ from B; set A's data/ mod_time to distant past (Unix epoch)
        shutil.rmtree(b / "data")
        os.utime(a / "data", (0, 0))  # 1970-01-01 -- very old

        # Run 2: A B (no canon); A is contributing with data/ live (old mod_time)
        rc, out, err = _run(str(a), str(b))
        _check(rc == 0, "012.1+2: run 2 exits 0")
        _check(
            (b / "data").is_dir(),
            "012.1: data/ created on B because contributing peer A has it live",
        )
        # 012.2: creation happened despite A's data/ having a Unix-epoch mod_time
        _check(
            (b / "data").is_dir(),
            "012.2: directory creation not blocked by very old mod_time on source dir",
        )


# ---------------------------------------------------------------------------
# 012.3: When no contributing peer has a directory live, at least one has a
# snapshot row for it, and every contributing peer with a row is absent from
# the listing, the directory is displaced to BAK/ on every peer that still has it.
#
# Three-peer setup: A and B are contributing peers that had data/ (rows in
# snapshot) but deleted it. C is a subordinate peer that still has data/.
# ---------------------------------------------------------------------------
def test_012_3() -> None:
    print("\n--- 012.3: dir displaced when all contributing peers with rows absent ---")
    with tempfile.TemporaryDirectory(prefix="ks012_3_") as tmp:
        a, b, c = Path(tmp) / "A", Path(tmp) / "B", Path(tmp) / "C"
        a.mkdir()
        b.mkdir()
        c.mkdir()

        for p in (a, b, c):
            _write(p / "seed.txt", "seed\n")
            _write(p / "data" / "f.txt", "f\n")

        # Run 1: +A B C -- establish snapshots with data/ rows on all three
        rc, _, _ = _run(f"+{a}", str(b), str(c))
        if rc != 0:
            _failures.append("012.3: run 1 failed to establish snapshot state")
            return

        # Delete data/ from both contributing peers A and B; C keeps it
        shutil.rmtree(a / "data")
        shutil.rmtree(b / "data")

        # Run 2: A B -C (no canon)
        # A: contributing, has data/ row, data/ absent from listing
        # B: contributing, has data/ row, data/ absent from listing
        # C: subordinate, still has data/ -> should be displaced
        rc, out, err = _run(str(a), str(b), f"-{c}")
        _check(rc == 0, "012.3: run 2 exits 0")
        _check(
            not (c / "data").exists(),
            "012.3: data/ no longer live on C (all contributing peers with rows absent)",
        )
        _check(
            _has_bak(c, "data"),
            "012.3: data/ displaced to BAK/ on C",
        )


# ---------------------------------------------------------------------------
# 012.4: A contributing peer with no snapshot row for a directory does not
# block displacement of that directory.
#
# Four-peer setup requiring three preliminary runs so that D has a snapshot
# (making it contributing) but has no row for data/ (it was not present in
# the run that introduced data/).
# ---------------------------------------------------------------------------
def test_012_4() -> None:
    print("\n--- 012.4: contributing peer with no snapshot row does not block displacement ---")
    with tempfile.TemporaryDirectory(prefix="ks012_4_") as tmp:
        a, b, c, d = (Path(tmp) / x for x in ("A", "B", "C", "D"))
        for p in (a, b, c, d):
            p.mkdir()

        for p in (a, b, c, d):
            _write(p / "seed.txt", "seed\n")
        # D has seed.txt only -- will never see data/

        # Run 0: +A D (seed.txt only on A) -- establishes D's snapshot without data/ row
        rc, _, _ = _run(f"+{a}", str(d))
        if rc != 0:
            _failures.append("012.4: run 0 failed")
            return

        # Add data/ to A, B, C after Run 0 so D's snapshot never gets a data/ row
        for p in (a, b, c):
            _write(p / "data" / "f.txt", "f\n")

        # Run 1: +A B C -- establishes snapshots on A, B, C with data/ rows
        rc, _, _ = _run(f"+{a}", str(b), str(c))
        if rc != 0:
            _failures.append("012.4: run 1 failed")
            return

        # Delete data/ from A and B; C (to be subordinate) keeps it
        shutil.rmtree(a / "data")
        shutil.rmtree(b / "data")

        # Run 2: A B -C D (no canon)
        # A: contributing, has data/ row, data/ absent -> votes deletion
        # B: contributing, has data/ row, data/ absent -> votes deletion
        # C: subordinate, still has data/ -> should be displaced
        # D: contributing, NO data/ row -> must not block displacement (012.4)
        rc, out, err = _run(str(a), str(b), f"-{c}", str(d))
        _check(rc == 0, "012.4: run 2 exits 0")
        _check(
            not (c / "data").exists(),
            "012.4: data/ displaced on C despite contributing peer D having no snapshot row",
        )
        _check(
            _has_bak(c, "data"),
            "012.4: data/ in BAK/ on C",
        )


# ---------------------------------------------------------------------------
# 012.5: When no contributing peer has a directory live or in snapshot,
# subordinate peers that have it are displaced to BAK/.
#
# Two-run setup: run 1 establishes snapshots for seed.txt only (no orphan/);
# orphan/ is created on B only after run 1, so A has no knowledge of it.
# ---------------------------------------------------------------------------
def test_012_5() -> None:
    print("\n--- 012.5: subordinate dir displaced when no contributing peer knows about it ---")
    with tempfile.TemporaryDirectory(prefix="ks012_5_") as tmp:
        a, b = Path(tmp) / "A", Path(tmp) / "B"
        a.mkdir()
        b.mkdir()

        _write(a / "seed.txt", "seed\n")
        _write(b / "seed.txt", "seed\n")

        # Run 1: +A B -- establish snapshots for seed.txt; neither peer has orphan/
        rc, _, _ = _run(f"+{a}", str(b))
        if rc != 0:
            _failures.append("012.5: run 1 failed to establish snapshot state")
            return

        # Create orphan/ on B after run 1 so that A has no snapshot row for it
        (b / "orphan").mkdir()
        _write(b / "orphan" / "leftover.txt", "leftover\n")

        # Run 2: A -B (A contributing, no orphan/ row or live; B subordinate with orphan/)
        # orphan/ does not exist in the group's view -> B's orphan/ displaced (012.5)
        rc, out, err = _run(str(a), f"-{b}")
        _check(rc == 0, "012.5: run 2 exits 0")
        _check(
            not (b / "orphan").exists(),
            "012.5: orphan/ not live on B (no contributing peer has it live or in snapshot)",
        )
        _check(
            _has_bak(b, "orphan"),
            "012.5: orphan/ displaced to BAK/ on B",
        )


# ---------------------------------------------------------------------------
# 012.13 + 012.14:
#   012.13: Without canon, when contributing peers hold a file and a directory
#           at the same path, the conflicting directory is displaced to BAK/.
#   012.14: After the directory is displaced, the winning file is selected by
#           normal file decision rules and synced to all active peers.
#
# Two-run setup: run 1 establishes snapshots; A then gets 'item' as a file
# and B gets 'item' as a directory before run 2.
# ---------------------------------------------------------------------------
def test_012_13_14() -> None:
    print("\n--- 012.13+012.14: no canon, file wins type conflict, winning file synced ---")
    with tempfile.TemporaryDirectory(prefix="ks012_1314_") as tmp:
        a, b = Path(tmp) / "A", Path(tmp) / "B"
        a.mkdir()
        b.mkdir()

        _write(a / "seed.txt", "seed\n")
        _write(b / "seed.txt", "seed\n")

        # Run 1: +A B -- establish snapshots; neither has 'item'
        rc, _, _ = _run(f"+{a}", str(b))
        if rc != 0:
            _failures.append("012.13+14: run 1 failed to establish snapshot state")
            return

        # A gets 'item' as a file (New); B gets 'item' as a directory (New)
        _write(a / "item", "winning file\n")
        (b / "item").mkdir()
        _write(b / "item" / "child.txt", "dir child\n")

        # Run 2: A B (no canon)
        # Type conflict: A (contributing) has file, B (contributing) has directory.
        # File wins (012.13). B's directory displaced (012.13).
        # A's file is the only file entry -> selected as winner, synced to B (012.14).
        rc, out, err = _run(str(a), str(b))
        _check(rc == 0, "012.13+14: run 2 exits 0")
        _check(
            _has_bak(b, "item"),
            "012.13: conflicting directory on contributing peer B displaced to BAK/",
        )
        _check(
            (b / "item").is_file(),
            "012.14: winning file synced to B after type conflict resolved",
        )
        if (b / "item").is_file():
            _check(
                (b / "item").read_text(encoding="utf-8") == "winning file\n",
                "012.14: synced file content correct on B",
            )


# ---------------------------------------------------------------------------
# 012.15 + 012.16 + 012.17:
#   012.15: A subordinate peer's file does not cause the file to win over a
#           contributing peer's directory at the same path.
#   012.16: After the contributing type decision is made, a subordinate peer
#           whose path has the wrong type is displaced to BAK/.
#   012.17: After the contributing type decision is made, a subordinate peer
#           whose path had the wrong type is conformed to the decided type.
#
# Two-run setup: run 1 establishes snapshots; A (contributing) then gets
# 'item' as a directory, B (subordinate) gets 'item' as a file before run 2.
# ---------------------------------------------------------------------------
def test_012_15_16_17() -> None:
    print("\n--- 012.15+012.16+012.17: subordinate type ignored; displaced and conformed ---")
    with tempfile.TemporaryDirectory(prefix="ks012_151617_") as tmp:
        a, b = Path(tmp) / "A", Path(tmp) / "B"
        a.mkdir()
        b.mkdir()

        _write(a / "seed.txt", "seed\n")
        _write(b / "seed.txt", "seed\n")

        # Run 1: +A B -- establish snapshots; neither has 'item'
        rc, _, _ = _run(f"+{a}", str(b))
        if rc != 0:
            _failures.append("012.15+16+17: run 1 failed to establish snapshot state")
            return

        # A (will be contributing) gets 'item' as a directory
        # B (will be subordinate) gets 'item' as a file
        (a / "item").mkdir()
        _write(a / "item" / "content.txt", "in dir\n")
        _write(b / "item", "wrong type file\n")

        # Run 2: A -B (A contributing, B subordinate)
        # Only A's type (directory) participates in the decision (012.15).
        # B's file (wrong type) is displaced to BAK/ (012.16).
        # B is conformed: directory 'item' created on B (012.17).
        rc, out, err = _run(str(a), f"-{b}")
        _check(rc == 0, "012.15+16+17: run 2 exits 0")
        _check(
            _has_bak(b, "item"),
            "012.16: subordinate B's wrong-type file displaced to BAK/",
        )
        _check(
            (b / "item").is_dir(),
            "012.17: B conformed to directory type decided by contributing peer A",
        )
        # 012.15 evidence: A's directory was not displaced; the file on B (subordinate)
        # had no influence on the type decision.
        _check(
            (a / "item").is_dir(),
            "012.15: contributing A's directory not displaced (subordinate B's file had no influence)",
        )


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
def main() -> None:
    if not EXE.exists():
        print(f"ERROR: executable not found: {EXE}", file=sys.stderr)
        sys.exit(1)

    print(f"Executable: {EXE}")

    test_012_6()
    test_012_7()
    test_012_8_9()
    test_012_10_11()
    test_012_12()
    test_012_1_2()
    test_012_3()
    test_012_4()
    test_012_5()
    test_012_13_14()
    test_012_15_16_17()

    print()
    if _failures:
        print(f"{len(_failures)} failure(s):")
        for f in _failures:
            print(f"  FAIL: {f}")
        sys.exit(1)

    print("All checks passed.")
    sys.exit(0)


if __name__ == "__main__":
    main()
