# /// script
# requires-python = ">=3.9"
# dependencies = []
# ///

import os
import pathlib
import sqlite3
import subprocess
import sys
import tempfile

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

WORKSPACE = pathlib.Path("/home/ace/Desktop/prjx/kitchensync")
EXECUTABLE = WORKSPACE / "released" / "kitchensync.exe"

FAILURES = []


def fail(msg):
    FAILURES.append(msg)
    print(f"FAIL: {msg}", flush=True)


def ok(msg):
    print(f"OK:   {msg}", flush=True)


def run_ks(*args, timeout=30):
    cmd = [str(EXECUTABLE)] + list(args)
    return subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)


def snapshot_row_count(db_path, basename):
    """Count snapshot rows with the given basename."""
    if not db_path.exists():
        return 0
    conn = sqlite3.connect(str(db_path))
    try:
        row = conn.execute(
            "SELECT COUNT(*) FROM snapshot WHERE basename = ?", (basename,)
        ).fetchone()
        return row[0] if row else 0
    finally:
        conn.close()


# ---------------------------------------------------------------------------
# 009.1 – .kitchensync/ directory not copied to peer that lacks it
# ---------------------------------------------------------------------------
def test_009_1_kitchensync_not_copied():
    with tempfile.TemporaryDirectory() as td:
        root = pathlib.Path(td)
        a, b = root / "a", root / "b"
        a.mkdir()
        b.mkdir()
        (a / "readme.txt").write_text("content")
        (a / ".kitchensync").mkdir()
        (a / ".kitchensync" / "user_secret.txt").write_text("secret")

        r = run_ks("+" + str(a), str(b))
        if r.returncode != 0:
            fail(f"009.1: sync exited {r.returncode}; stdout={r.stdout!r}")
            return
        if not (b / "readme.txt").exists():
            fail("009.1: readme.txt not synced to peer_b – sync did not work")
            return
        if (b / ".kitchensync" / "user_secret.txt").exists():
            fail("009.1: .kitchensync/user_secret.txt was copied to peer_b – should be excluded")
        else:
            ok("009.1: .kitchensync/ user content not copied to peer")


# ---------------------------------------------------------------------------
# 009.2 – .git/ directory not copied to peer that lacks it
# ---------------------------------------------------------------------------
def test_009_2_git_not_copied():
    with tempfile.TemporaryDirectory() as td:
        root = pathlib.Path(td)
        a, b = root / "a", root / "b"
        a.mkdir()
        b.mkdir()
        (a / "readme.txt").write_text("content")
        (a / ".git").mkdir()
        (a / ".git" / "config").write_text("[core]\n")

        r = run_ks("+" + str(a), str(b))
        if r.returncode != 0:
            fail(f"009.2: sync exited {r.returncode}; stdout={r.stdout!r}")
            return
        if not (b / "readme.txt").exists():
            fail("009.2: readme.txt not synced to peer_b – sync did not work")
            return
        if (b / ".git").exists():
            fail("009.2: .git/ was copied to peer_b – should be excluded")
        else:
            ok("009.2: .git/ not copied to peer")


# ---------------------------------------------------------------------------
# 009.3 – symbolic link not copied
# not reasonably testable: 009.3 -- testing guidelines prohibit creating
# symlinks in test setup; observable only when a symlink occurs naturally
# ---------------------------------------------------------------------------


# ---------------------------------------------------------------------------
# 009.4 – special file (FIFO) not copied to other peers
# ---------------------------------------------------------------------------
def test_009_4_special_file_not_copied():
    if sys.platform == "win32":
        print("SKIP: 009.4: os.mkfifo not available on Windows", flush=True)
        return

    with tempfile.TemporaryDirectory() as td:
        root = pathlib.Path(td)
        a, b = root / "a", root / "b"
        a.mkdir()
        b.mkdir()
        (a / "readme.txt").write_text("content")
        os.mkfifo(str(a / "my_fifo"))

        r = run_ks("+" + str(a), str(b))
        if r.returncode != 0:
            fail(f"009.4: sync exited {r.returncode}; stdout={r.stdout!r}")
            return
        if not (b / "readme.txt").exists():
            fail("009.4: readme.txt not synced to peer_b – sync did not work")
            return
        if (b / "my_fifo").exists():
            fail("009.4: FIFO was copied to peer_b – special files should be excluded")
        else:
            ok("009.4: FIFO (special file) not copied to peer")


# ---------------------------------------------------------------------------
# 009.5 – -x path not copied to peer that lacks it
# ---------------------------------------------------------------------------
def test_009_5_cli_exclude_not_copied():
    with tempfile.TemporaryDirectory() as td:
        root = pathlib.Path(td)
        a, b = root / "a", root / "b"
        a.mkdir()
        b.mkdir()
        (a / "excluded.txt").write_text("excluded")
        (a / "regular.txt").write_text("regular")

        r = run_ks("+" + str(a), str(b), "-x", "excluded.txt")
        if r.returncode != 0:
            fail(f"009.5: sync exited {r.returncode}; stdout={r.stdout!r}")
            return
        if not (b / "regular.txt").exists():
            fail("009.5: regular.txt not synced to peer_b – sync did not work")
        if (b / "excluded.txt").exists():
            fail("009.5: excluded.txt copied to peer_b despite -x flag")
        else:
            ok("009.5: -x excluded file not copied to peer")


# ---------------------------------------------------------------------------
# 009.6 – command-line -x excludes take effect alongside built-in excludes
# ---------------------------------------------------------------------------
def test_009_6_cli_and_builtin_excludes_combined():
    with tempfile.TemporaryDirectory() as td:
        root = pathlib.Path(td)
        a, b = root / "a", root / "b"
        a.mkdir()
        b.mkdir()
        (a / "regular.txt").write_text("regular")
        (a / "custom_exclude.txt").write_text("custom")
        (a / ".git").mkdir()
        (a / ".git" / "HEAD").write_text("ref: refs/heads/main\n")

        r = run_ks("+" + str(a), str(b), "-x", "custom_exclude.txt")
        if r.returncode != 0:
            fail(f"009.6: sync exited {r.returncode}; stdout={r.stdout!r}")
            return
        if not (b / "regular.txt").exists():
            fail("009.6: regular.txt not synced – sync did not work")

        ok_builtin = not (b / ".git").exists()
        ok_cli = not (b / "custom_exclude.txt").exists()
        if not ok_builtin:
            fail("009.6: .git/ (built-in exclude) was copied in the same run as -x")
        if not ok_cli:
            fail("009.6: custom_exclude.txt (-x) was copied in the same run as built-in exclude")
        if ok_builtin and ok_cli:
            ok("009.6: both built-in and -x excludes applied in the same run")


# ---------------------------------------------------------------------------
# 009.7 – excluded entry already on a peer is left in place (not displaced)
# ---------------------------------------------------------------------------
def test_009_7_excluded_entry_not_displaced():
    with tempfile.TemporaryDirectory() as td:
        root = pathlib.Path(td)
        a, b = root / "a", root / "b"
        a.mkdir()
        b.mkdir()
        # peer_a is the canon source; only regular file
        (a / "file.txt").write_text("content")
        # peer_b already has a .git/ and a file that will be -x excluded;
        # neither should be displaced even though peer_a lacks both
        (b / ".git").mkdir()
        (b / ".git" / "config").write_text("[core]\n")
        (b / "keep_me.txt").write_text("keep this")

        r = run_ks("+" + str(a), str(b), "-x", "keep_me.txt")
        if r.returncode != 0:
            fail(f"009.7: sync exited {r.returncode}; stdout={r.stdout!r}")
            return

        if not (b / ".git").exists():
            fail("009.7: .git/ on peer_b was displaced – built-in excluded entry must be left in place")
        else:
            ok("009.7a: .git/ on peer_b left in place (built-in exclude)")

        if not (b / "keep_me.txt").exists():
            fail("009.7: keep_me.txt on peer_b was displaced – -x excluded entry must be left in place")
        else:
            ok("009.7b: -x excluded file on peer_b left in place")


# ---------------------------------------------------------------------------
# 009.8 – excluded directory and all descendants skipped
# ---------------------------------------------------------------------------
def test_009_8_excluded_directory_and_descendants_skipped():
    with tempfile.TemporaryDirectory() as td:
        root = pathlib.Path(td)
        a, b = root / "a", root / "b"
        a.mkdir()
        b.mkdir()
        (a / "regular.txt").write_text("regular")
        excl = a / "excluded_dir"
        excl.mkdir()
        (excl / "file1.txt").write_text("file1")
        sub = excl / "subdir"
        sub.mkdir()
        (sub / "deep_file.txt").write_text("deep")

        r = run_ks("+" + str(a), str(b), "-x", "excluded_dir")
        if r.returncode != 0:
            fail(f"009.8: sync exited {r.returncode}; stdout={r.stdout!r}")
            return
        if not (b / "regular.txt").exists():
            fail("009.8: regular.txt not synced – sync did not work")

        problems = []
        if (b / "excluded_dir").exists():
            problems.append("excluded_dir itself")
        if (b / "excluded_dir" / "file1.txt").exists():
            problems.append("excluded_dir/file1.txt")
        if (b / "excluded_dir" / "subdir" / "deep_file.txt").exists():
            problems.append("excluded_dir/subdir/deep_file.txt")
        if problems:
            fail(
                "009.8: excluded directory content found on peer_b: "
                + ", ".join(problems)
            )
        else:
            ok("009.8: excluded directory and all descendants skipped on peer_b")


# ---------------------------------------------------------------------------
# 009.9 – no snapshot row created or updated for excluded paths
# ---------------------------------------------------------------------------
def test_009_9_no_snapshot_row_for_excluded():
    with tempfile.TemporaryDirectory() as td:
        root = pathlib.Path(td)
        a, b = root / "a", root / "b"
        a.mkdir()
        b.mkdir()
        (a / "regular.txt").write_text("regular")
        (a / "excluded.txt").write_text("excluded")
        (a / ".git").mkdir()
        (a / ".git" / "config").write_text("[core]\n")

        r = run_ks("+" + str(a), str(b), "-x", "excluded.txt")
        if r.returncode != 0:
            fail(f"009.9: sync exited {r.returncode}; stdout={r.stdout!r}")
            return
        if not (b / "regular.txt").exists():
            fail("009.9: regular.txt not synced – sync did not work")
            return

        db_a = a / ".kitchensync" / "snapshot.db"
        db_b = b / ".kitchensync" / "snapshot.db"

        for label, db in [("peer_a", db_a), ("peer_b", db_b)]:
            if not db.exists():
                fail(f"009.9: {label} snapshot.db missing after sync")
                continue

            n = snapshot_row_count(db, "excluded.txt")
            if n > 0:
                fail(
                    f"009.9: {label} snapshot has {n} row(s) for excluded.txt "
                    f"– excluded path must not produce snapshot rows"
                )
            else:
                ok(f"009.9a ({label}): no snapshot row for -x excluded file")

            n = snapshot_row_count(db, ".git")
            if n > 0:
                fail(
                    f"009.9: {label} snapshot has {n} row(s) for .git "
                    f"– built-in excluded path must not produce snapshot rows"
                )
            else:
                ok(f"009.9b ({label}): no snapshot row for .git (built-in exclude)")


# ---------------------------------------------------------------------------
# Run all tests
# ---------------------------------------------------------------------------
test_009_1_kitchensync_not_copied()
test_009_2_git_not_copied()
# not reasonably testable: 009.3 -- testing guidelines prohibit creating symlinks in test setup
test_009_4_special_file_not_copied()
test_009_5_cli_exclude_not_copied()
test_009_6_cli_and_builtin_excludes_combined()
test_009_7_excluded_entry_not_displaced()
test_009_8_excluded_directory_and_descendants_skipped()
test_009_9_no_snapshot_row_for_excluded()

if FAILURES:
    print(f"\n{len(FAILURES)} failure(s):", flush=True)
    for f in FAILURES:
        print(f"  - {f}", flush=True)
    sys.exit(1)

print("\nAll checks passed.", flush=True)
sys.exit(0)
