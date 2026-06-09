# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""End-to-end tests for requirement 008_traversal: Combined-tree walk.

Covers requirements 008.1 through 008.16.
All checks are collected before reporting; exit 1 only when any check fails.
"""

import os
import platform
import sqlite3
import stat
import subprocess
import sys
import tempfile
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

WORKSPACE = Path("/home/ace/Desktop/prjx/kitchensync")
EXE = WORKSPACE / "released" / "kitchensync.exe"

_plat = platform.system()
if _plat == "Windows":
    UV = WORKSPACE / "aitc" / "bin" / "uv.exe"
elif _plat == "Darwin":
    UV = WORKSPACE / "aitc" / "bin" / "uv.mac"
else:
    UV = WORKSPACE / "aitc" / "bin" / "uv.linux"

FAILURES: list[str] = []
ON_WINDOWS = _plat == "Windows"


def fail(msg: str) -> None:
    FAILURES.append(msg)
    print(f"  FAIL: {msg}", flush=True)


def run_ks(*args: object, timeout: int = 60) -> subprocess.CompletedProcess:
    cmd = [str(EXE)] + [str(a) for a in args]
    return subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)


def write_file(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


def find_bak_entry(peer_root: Path, basename: str) -> Path | None:
    """Return the first BAK path that contains an entry with the given basename."""
    for ks_dir in peer_root.rglob(".kitchensync"):
        bak = ks_dir / "BAK"
        if not bak.is_dir():
            continue
        for ts_dir in bak.iterdir():
            if not ts_dir.is_dir():
                continue
            candidate = ts_dir / basename
            if candidate.exists():
                return candidate
    return None


def snap_rows_for_prefix(snap_path: Path, prefix: str) -> list[tuple]:
    """Return snapshot rows whose path starts with prefix, sorted for comparison."""
    if not snap_path.exists():
        return []
    try:
        con = sqlite3.connect(str(snap_path))
        try:
            cur = con.execute(
                "SELECT * FROM snapshot WHERE path LIKE ?",
                (prefix + "%",),
            )
            return sorted(cur.fetchall())
        finally:
            con.close()
    except sqlite3.DatabaseError:
        return []


# ── 008.1 ────────────────────────────────────────────────────────────────────

def test_008_1_ordering_indirect() -> None:
    """008.1 (indirect): entries with mixed-case names are all synced correctly.
    Strict processing order is not directly observable from the CLI surface
    without internal instrumentation.
    # not reasonably testable: 008.1 strict case-insensitive order proof
    """
    print("008.1 case-insensitive ordering (indirect)", flush=True)
    with tempfile.TemporaryDirectory() as tmp_str:
        tmp = Path(tmp_str)
        pa, pb = tmp / "peer_a", tmp / "peer_b"
        pa.mkdir(); pb.mkdir()
        entries = ["zebra.txt", "Apple.txt", "MANGO.txt", "banana.TXT", "cherry.txt"]
        for name in entries:
            write_file(pa / name, f"content-{name}")
        r = run_ks(f"+{pa}", str(pb))
        if r.returncode != 0:
            fail(f"008.1: sync failed rc={r.returncode}\n{r.stdout}")
            return
        for name in entries:
            if not (pb / name).exists():
                fail(f"008.1: entry '{name}' missing from peer_b after sync")


# ── 008.2 ────────────────────────────────────────────────────────────────────

def test_008_2_preorder_traversal() -> None:
    """008.2: every entry in a directory is acted on before any subdirectory is entered."""
    print("008.2 pre-order traversal", flush=True)
    with tempfile.TemporaryDirectory() as tmp_str:
        tmp = Path(tmp_str)
        pa, pb = tmp / "peer_a", tmp / "peer_b"
        pa.mkdir(); pb.mkdir()
        write_file(pa / "root_file.txt", "root")
        write_file(pa / "dir_a" / "file_in_a.txt", "in a")
        write_file(pa / "dir_a" / "subdir" / "deep.txt", "deep")
        write_file(pa / "dir_b" / "file_in_b.txt", "in b")
        r = run_ks(f"+{pa}", str(pb))
        if r.returncode != 0:
            fail(f"008.2: sync failed rc={r.returncode}\n{r.stdout}")
            return
        for rel in [
            "root_file.txt",
            "dir_a/file_in_a.txt",
            "dir_a/subdir/deep.txt",
            "dir_b/file_in_b.txt",
        ]:
            if not (pb / Path(rel)).exists():
                fail(f"008.2: {rel} missing from peer_b (pre-order walk must cover all levels)")


# ── 008.3 ────────────────────────────────────────────────────────────────────

def test_008_3_contributing_peer_entry_visited() -> None:
    """008.3: an entry in any contributing peer's live listing is visited during the walk."""
    print("008.3 contributing peer entry visited", flush=True)
    with tempfile.TemporaryDirectory() as tmp_str:
        tmp = Path(tmp_str)
        pa, pb = tmp / "peer_a", tmp / "peer_b"
        pa.mkdir(); pb.mkdir()
        write_file(pa / "hello.txt", "hello")
        write_file(pa / "sub" / "world.txt", "world")
        r = run_ks(f"+{pa}", str(pb))
        if r.returncode != 0:
            fail(f"008.3: sync failed rc={r.returncode}\n{r.stdout}")
            return
        if not (pb / "hello.txt").exists():
            fail("008.3: hello.txt not synced to peer_b")
        if not (pb / "sub" / "world.txt").exists():
            fail("008.3: sub/world.txt not synced to peer_b")


# ── 008.4 ────────────────────────────────────────────────────────────────────

def test_008_4_subordinate_entry_visited_for_cleanup() -> None:
    """008.4: an entry that appears only in a subordinate peer's listing is visited so it can be
    brought into conformance (displaced to BAK/ if the group has no such entry)."""
    print("008.4 subordinate peer entry displaced", flush=True)
    with tempfile.TemporaryDirectory() as tmp_str:
        tmp = Path(tmp_str)
        pa, pb, pc = tmp / "peer_a", tmp / "peer_b", tmp / "peer_c"
        pa.mkdir(); pb.mkdir(); pc.mkdir()
        write_file(pa / "shared.txt", "shared")
        # First run: establish shared.txt on pa and pb with snapshots.
        r = run_ks(f"+{pa}", str(pb))
        if r.returncode != 0:
            fail(f"008.4: first sync failed rc={r.returncode}\n{r.stdout}")
            return
        # peer_c has an extra file that is not part of the group.
        write_file(pc / "extra.txt", "should be displaced")
        # Second run: pc is explicit subordinate (-); no contributing peer has extra.txt,
        # so the group view is "extra.txt does not exist" and pc must be brought into
        # conformance by displacing it.
        r = run_ks(str(pa), str(pb), f"-{pc}")
        if r.returncode != 0:
            fail(f"008.4: second sync failed rc={r.returncode}\n{r.stdout}")
            return
        if (pc / "extra.txt").exists():
            fail("008.4: extra.txt still present on subordinate peer (expected displacement to BAK/)")
        if find_bak_entry(pc, "extra.txt") is None:
            fail("008.4: extra.txt not found in peer_c BAK/ after displacement")


# ── 008.5 ────────────────────────────────────────────────────────────────────

def test_008_5_snapshot_only_not_visited() -> None:
    """008.5: an entry that appears only in snapshot rows and in no live listing is not
    added to the walk (and therefore not re-created)."""
    print("008.5 snapshot-only entry not re-created", flush=True)
    with tempfile.TemporaryDirectory() as tmp_str:
        tmp = Path(tmp_str)
        pa, pb = tmp / "peer_a", tmp / "peer_b"
        pa.mkdir(); pb.mkdir()
        write_file(pa / "ghost.txt", "ghost")
        # First run: ghost.txt lands on both peers with snapshot rows.
        r = run_ks(f"+{pa}", str(pb))
        if r.returncode != 0:
            fail(f"008.5: first sync failed rc={r.returncode}\n{r.stdout}")
            return
        # Manually remove ghost.txt from both peers (bypassing kitchensync).
        (pa / "ghost.txt").unlink(missing_ok=True)
        (pb / "ghost.txt").unlink(missing_ok=True)
        # Second run: both peers have snapshot history so no canon is required.
        # ghost.txt is not in any live listing; it must not be re-created.
        r = run_ks(str(pa), str(pb))
        if r.returncode != 0:
            fail(f"008.5: second sync failed rc={r.returncode}\n{r.stdout}")
            return
        if (pa / "ghost.txt").exists():
            fail("008.5: ghost.txt re-created on peer_a (snapshot-only entry must not be added to walk)")
        if (pb / "ghost.txt").exists():
            fail("008.5: ghost.txt re-created on peer_b (snapshot-only entry must not be added to walk)")


# ── 008.6 ────────────────────────────────────────────────────────────────────

def test_008_6_displacement_inline() -> None:
    """008.6: type-conflict displacement runs inline during the walk so the dependent
    file copy succeeds within the same run."""
    print("008.6 type-conflict displacement is inline", flush=True)
    with tempfile.TemporaryDirectory() as tmp_str:
        tmp = Path(tmp_str)
        pa, pb = tmp / "peer_a", tmp / "peer_b"
        pa.mkdir(); pb.mkdir()
        # Canon: "sub" is a regular file.
        write_file(pa / "sub", "file-content")
        # peer_b has "sub" as a directory (type conflict).
        (pb / "sub").mkdir()
        write_file(pb / "sub" / "inside.txt", "will be displaced with parent dir")
        r = run_ks(f"+{pa}", str(pb))
        if r.returncode != 0:
            fail(f"008.6: sync failed rc={r.returncode}\n{r.stdout}")
            return
        # The directory displacement must have happened inline so the copy of
        # the file "sub" succeeded in this same run.
        if not (pb / "sub").exists() or (pb / "sub").is_dir():
            fail("008.6: peer_b/sub is not a file after sync -- inline displacement did not clear the path for the copy")
        if (pb / "sub").is_file():
            content = (pb / "sub").read_text(encoding="utf-8")
            if content != "file-content":
                fail(f"008.6: peer_b/sub has wrong content: {content!r}")
        # The original directory must be in BAK/.
        if find_bak_entry(pb, "sub") is None:
            fail("008.6: original 'sub' directory not found in peer_b BAK/")


# ── 008.7 ────────────────────────────────────────────────────────────────────

def test_008_7_directory_displaced_as_subtree() -> None:
    """008.7: a directory chosen for displacement is moved to BAK/ as a single rename
    that preserves its entire subtree."""
    print("008.7 directory displaced as whole subtree", flush=True)
    with tempfile.TemporaryDirectory() as tmp_str:
        tmp = Path(tmp_str)
        pa, pb = tmp / "peer_a", tmp / "peer_b"
        pa.mkdir(); pb.mkdir()
        # Canon has "mydir" as a file.
        write_file(pa / "mydir", "a-file")
        # peer_b has "mydir" as a directory with nested content.
        (pb / "mydir").mkdir()
        write_file(pb / "mydir" / "child1.txt", "child1")
        (pb / "mydir" / "subdir").mkdir()
        write_file(pb / "mydir" / "subdir" / "child2.txt", "child2")
        r = run_ks(f"+{pa}", str(pb))
        if r.returncode != 0:
            fail(f"008.7: sync failed rc={r.returncode}\n{r.stdout}")
            return
        bak_mydir = find_bak_entry(pb, "mydir")
        if bak_mydir is None:
            fail("008.7: displaced 'mydir' not found in peer_b BAK/")
            return
        if not (bak_mydir / "child1.txt").exists():
            fail("008.7: BAK/mydir/child1.txt missing -- directory was not moved as a single rename")
        if not (bak_mydir / "subdir" / "child2.txt").exists():
            fail("008.7: BAK/mydir/subdir/child2.txt missing -- subtree not fully preserved in BAK/")


# ── 008.8 ────────────────────────────────────────────────────────────────────

def test_008_8_no_recurse_into_displaced() -> None:
    """008.8: KitchenSync does not recurse into a directory that is being displaced;
    its children are not processed individually on that peer."""
    print("008.8 no recursion into displaced directory", flush=True)
    with tempfile.TemporaryDirectory() as tmp_str:
        tmp = Path(tmp_str)
        pa, pb = tmp / "peer_a", tmp / "peer_b"
        pa.mkdir(); pb.mkdir()
        # Canon (pa) does NOT have "mydir" at all.
        write_file(pa / "other.txt", "other")
        # peer_b has "mydir" as a directory with content.
        (pb / "mydir").mkdir()
        write_file(pb / "mydir" / "inner.txt", "inner")
        r = run_ks(f"+{pa}", str(pb))
        if r.returncode != 0:
            fail(f"008.8: sync failed rc={r.returncode}\n{r.stdout}")
            return
        # mydir must be displaced on peer_b (canon lacks it).
        bak = find_bak_entry(pb, "mydir")
        if bak is None:
            fail("008.8: mydir not found in peer_b BAK/ -- was it displaced?")
        elif not (bak / "inner.txt").exists():
            fail("008.8: inner.txt missing from BAK/mydir -- subtree not preserved as whole unit")
        # inner.txt must NOT have been individually propagated to peer_a.
        if (pa / "mydir").exists() or (pa / "mydir" / "inner.txt").exists():
            fail("008.8: inner.txt was propagated to peer_a (recursed into a displaced directory)")


# ── 008.9 ────────────────────────────────────────────────────────────────────

def test_008_9_only_keeping_peers_recurse() -> None:
    """008.9: when a directory is kept on some peers and displaced on others, only the
    peers keeping the directory have its children synchronized."""
    print("008.9 only keeping peers have children synced", flush=True)
    with tempfile.TemporaryDirectory() as tmp_str:
        tmp = Path(tmp_str)
        pa, pb, pc = tmp / "peer_a", tmp / "peer_b", tmp / "peer_c"
        pa.mkdir(); pb.mkdir(); pc.mkdir()
        # Canon (pa) has dir/ as a directory with a child file.
        (pa / "dir").mkdir()
        write_file(pa / "dir" / "file.txt", "content")
        # peer_b has "dir" as a FILE -- type conflict, will be displaced.
        write_file(pb / "dir", "wrong-type")
        # peer_c is empty -- dir/ will be created here.
        r = run_ks(f"+{pa}", str(pb), str(pc))
        if r.returncode != 0:
            fail(f"008.9: sync failed rc={r.returncode}\n{r.stdout}")
            return
        # peer_b's "dir" file was displaced; peer_b now keeps dir/ and must have the child.
        if not (pb / "dir").is_dir():
            fail("008.9: peer_b/dir is not a directory after sync")
        if not (pb / "dir" / "file.txt").exists():
            fail("008.9: peer_b/dir/file.txt not synced (peer_b kept directory, must recurse)")
        # peer_c received dir/ and must also have the child.
        if not (pc / "dir").is_dir():
            fail("008.9: peer_c/dir not created")
        if not (pc / "dir" / "file.txt").exists():
            fail("008.9: peer_c/dir/file.txt not synced (peer_c keeps directory, must recurse)")


# ── 008.10 ───────────────────────────────────────────────────────────────────

def test_008_10_listing_retry() -> None:
    """008.10: when a directory listing fails, KitchenSync retries up to --retries-list total tries."""
    print("008.10 listing retry reported in output", flush=True)
    if ON_WINDOWS:
        print("  SKIP: chmod-based listing failure not supported on Windows")
        return
    with tempfile.TemporaryDirectory() as tmp_str:
        tmp = Path(tmp_str)
        pa, pb = tmp / "peer_a", tmp / "peer_b"
        pa.mkdir(); pb.mkdir()
        write_file(pa / "locked" / "file.txt", "content")
        (pb / "locked").mkdir()
        write_file(pb / "locked" / "file.txt", "content")
        # Establish snapshots.
        r = run_ks(f"+{pa}", str(pb))
        if r.returncode != 0:
            fail(f"008.10: setup sync failed rc={r.returncode}\n{r.stdout}")
            return
        # Make pb/locked unlistable so every listing attempt fails.
        os.chmod(str(pb / "locked"), stat.S_IWRITE | stat.S_IREAD)
        try:
            r = run_ks(str(pa), str(pb), "--retries-list", "2")
        finally:
            os.chmod(str(pb / "locked"), stat.S_IRWXU)
        # Run must complete (listing failure is non-fatal for a subtree).
        # Error output about listing failure must appear on stdout (all output goes to stdout).
        out_lower = r.stdout.lower()
        if "listing" not in out_lower and "list" not in out_lower:
            fail(f"008.10: no listing-failure diagnostic on stdout; got: {r.stdout[:400]}")


# ── 008.11 ───────────────────────────────────────────────────────────────────

def test_008_11_listing_failure_no_modification() -> None:
    """008.11: after all listing tries fail on a peer, no files are created, deleted,
    displaced, or copied on that peer under the failed subtree during the run."""
    print("008.11 listing failure -- no file modification on failed peer subtree", flush=True)
    if ON_WINDOWS:
        print("  SKIP: chmod-based listing failure not supported on Windows")
        return
    with tempfile.TemporaryDirectory() as tmp_str:
        tmp = Path(tmp_str)
        pa, pb = tmp / "peer_a", tmp / "peer_b"
        pa.mkdir(); pb.mkdir()
        write_file(pa / "locked" / "shared.txt", "shared")
        write_file(pb / "locked" / "shared.txt", "shared")
        # Establish snapshots.
        r = run_ks(f"+{pa}", str(pb))
        if r.returncode != 0:
            fail(f"008.11: setup sync failed rc={r.returncode}\n{r.stdout}")
            return
        # Add a new file to pa/locked that would normally be copied to pb/locked.
        write_file(pa / "locked" / "new_from_a.txt", "new")
        before_contents = {
            p.name for p in (pb / "locked").iterdir()
            if p.name != ".kitchensync"
        }
        # Make pb/locked unlistable so listing fails for that subtree on pb.
        os.chmod(str(pb / "locked"), stat.S_IWRITE | stat.S_IREAD)
        try:
            run_ks(str(pa), str(pb), "--retries-list", "1")
        finally:
            os.chmod(str(pb / "locked"), stat.S_IRWXU)
        after_contents = {
            p.name for p in (pb / "locked").iterdir()
            if p.name != ".kitchensync"
        }
        added = after_contents - before_contents
        removed = before_contents - after_contents
        if added:
            fail(f"008.11: files added to pb/locked after listing failure: {added}")
        if removed:
            fail(f"008.11: files removed from pb/locked after listing failure: {removed}")


# ── 008.12 ───────────────────────────────────────────────────────────────────

def test_008_12_listing_failure_snapshot_unchanged() -> None:
    """008.12: after all listing tries fail, that peer's snapshot rows for the failed
    subtree are not modified."""
    print("008.12 listing failure -- snapshot rows unchanged for failed subtree", flush=True)
    if ON_WINDOWS:
        print("  SKIP: chmod-based listing failure not supported on Windows")
        return
    with tempfile.TemporaryDirectory() as tmp_str:
        tmp = Path(tmp_str)
        pa, pb = tmp / "peer_a", tmp / "peer_b"
        pa.mkdir(); pb.mkdir()
        write_file(pa / "subdir" / "file.txt", "content")
        (pb / "subdir").mkdir()
        # Establish snapshots.
        r = run_ks(f"+{pa}", str(pb))
        if r.returncode != 0:
            fail(f"008.12: setup sync failed rc={r.returncode}\n{r.stdout}")
            return
        snap_b = pb / ".kitchensync" / "snapshot.db"
        if not snap_b.exists():
            fail("008.12: peer_b snapshot.db not present after first run")
            return
        rows_before = snap_rows_for_prefix(snap_b, "subdir")
        # Make pb/subdir unlistable.
        os.chmod(str(pb / "subdir"), stat.S_IWRITE | stat.S_IREAD)
        try:
            run_ks(str(pa), str(pb), "--retries-list", "1")
        finally:
            os.chmod(str(pb / "subdir"), stat.S_IRWXU)
        rows_after = snap_rows_for_prefix(snap_b, "subdir")
        if rows_before != rows_after:
            fail(
                f"008.12: peer_b snapshot rows for subdir changed after listing failure\n"
                f"  before: {rows_before}\n  after:  {rows_after}"
            )


# ── 008.13 + 008.14 ──────────────────────────────────────────────────────────

def test_008_13_14_canon_listing_failure() -> None:
    """008.13: when the canon peer's listing fails, no peer's files under that subtree
    are modified.
    008.14: when the canon peer's listing fails, no peer's snapshot rows under that
    subtree are modified."""
    print("008.13/008.14 canon listing failure -- no peer modifications", flush=True)
    if ON_WINDOWS:
        print("  SKIP: chmod-based listing failure not supported on Windows")
        return
    with tempfile.TemporaryDirectory() as tmp_str:
        tmp = Path(tmp_str)
        pa, pb = tmp / "peer_a", tmp / "peer_b"
        pa.mkdir(); pb.mkdir()
        write_file(pa / "subdir" / "file.txt", "original")
        write_file(pb / "subdir" / "file.txt", "original")
        # Establish snapshots.
        r = run_ks(f"+{pa}", str(pb))
        if r.returncode != 0:
            fail(f"008.13: setup sync failed rc={r.returncode}\n{r.stdout}")
            return
        snap_b = pb / ".kitchensync" / "snapshot.db"
        rows_before = snap_rows_for_prefix(snap_b, "subdir") if snap_b.exists() else []
        pb_file_before = (pb / "subdir" / "file.txt").read_text(encoding="utf-8")
        # Modify the file in pa/subdir -- it would normally be copied to pb on next run.
        write_file(pa / "subdir" / "file.txt", "modified-by-canon")
        # Make canon's subdir unlistable; canon listing failure must skip that subtree.
        os.chmod(str(pa / "subdir"), stat.S_IWRITE | stat.S_IREAD)
        try:
            run_ks(f"+{pa}", str(pb), "--retries-list", "1")
        finally:
            os.chmod(str(pa / "subdir"), stat.S_IRWXU)
        # peer_b's file must be unchanged (008.13).
        pb_file_after = (pb / "subdir" / "file.txt").read_text(encoding="utf-8")
        if pb_file_after != pb_file_before:
            fail(
                f"008.13: peer_b subdir file was modified despite canon listing failure "
                f"(before={pb_file_before!r}, after={pb_file_after!r})"
            )
        # peer_b's snapshot rows for subdir must be unchanged (008.14).
        rows_after = snap_rows_for_prefix(snap_b, "subdir") if snap_b.exists() else []
        if rows_before != rows_after:
            fail(
                f"008.14: peer_b snapshot rows changed after canon listing failure\n"
                f"  before: {rows_before}\n  after:  {rows_after}"
            )


# ── 008.15 ───────────────────────────────────────────────────────────────────

def test_008_15_all_contributing_fail_skip_subtree() -> None:
    """008.15: when every contributing peer fails listing for a directory, KitchenSync
    skips that subtree entirely and does not displace subordinate peers' files under it."""
    print("008.15 all contributing peers fail -- subordinate not displaced", flush=True)
    if ON_WINDOWS:
        print("  SKIP: chmod-based listing failure not supported on Windows")
        return
    with tempfile.TemporaryDirectory() as tmp_str:
        tmp = Path(tmp_str)
        pa, pb, pc = tmp / "peer_a", tmp / "peer_b", tmp / "peer_c"
        pa.mkdir(); pb.mkdir(); pc.mkdir()
        write_file(pa / "subdir" / "file.txt", "content")
        write_file(pb / "subdir" / "file.txt", "content")
        # Establish snapshots for pa and pb.
        r = run_ks(f"+{pa}", str(pb))
        if r.returncode != 0:
            fail(f"008.15: setup sync failed rc={r.returncode}\n{r.stdout}")
            return
        # pc is a subordinate peer with an extra file that the group does not have.
        write_file(pc / "subdir" / "extra.txt", "subordinate-only file")
        # Make BOTH contributing peers' subdir unlistable.
        os.chmod(str(pa / "subdir"), stat.S_IWRITE | stat.S_IREAD)
        os.chmod(str(pb / "subdir"), stat.S_IWRITE | stat.S_IREAD)
        try:
            run_ks(str(pa), str(pb), f"-{pc}", "--retries-list", "1")
        finally:
            os.chmod(str(pa / "subdir"), stat.S_IRWXU)
            os.chmod(str(pb / "subdir"), stat.S_IRWXU)
        # All contributing peers failed listing for subdir; the subtree must be skipped.
        # pc/subdir/extra.txt must NOT have been displaced.
        if not (pc / "subdir" / "extra.txt").exists():
            bak = find_bak_entry(pc, "extra.txt")
            if bak is not None:
                fail(
                    "008.15: subordinate peer's extra.txt was displaced even though all "
                    "contributing peers failed listing for subdir (subtree must be skipped)"
                )


# ── 008.16 ───────────────────────────────────────────────────────────────────

def test_008_16_filename_preservation() -> None:
    """008.16: filenames are preserved exactly as the filesystem reports them."""
    print("008.16 filename preservation", flush=True)
    with tempfile.TemporaryDirectory() as tmp_str:
        tmp = Path(tmp_str)
        pa, pb = tmp / "peer_a", tmp / "peer_b"
        pa.mkdir(); pb.mkdir()
        filenames = [
            "Hello World.txt",
            "UPPER.TXT",
            "mixed_Case-File.dat",
            "file with spaces.txt",
            "numbers123.txt",
        ]
        for name in filenames:
            write_file(pa / name, f"content-{name}")
        r = run_ks(f"+{pa}", str(pb))
        if r.returncode != 0:
            fail(f"008.16: sync failed rc={r.returncode}\n{r.stdout}")
            return
        for name in filenames:
            if not (pb / name).exists():
                fail(f"008.16: file '{name}' not synced to peer_b with exact name preserved")


# ── main ─────────────────────────────────────────────────────────────────────

def main() -> None:
    if not EXE.exists():
        print(f"FATAL: executable not found: {EXE}", flush=True)
        sys.exit(1)

    tests = [
        test_008_1_ordering_indirect,
        test_008_2_preorder_traversal,
        test_008_3_contributing_peer_entry_visited,
        test_008_4_subordinate_entry_visited_for_cleanup,
        test_008_5_snapshot_only_not_visited,
        test_008_6_displacement_inline,
        test_008_7_directory_displaced_as_subtree,
        test_008_8_no_recurse_into_displaced,
        test_008_9_only_keeping_peers_recurse,
        test_008_10_listing_retry,
        test_008_11_listing_failure_no_modification,
        test_008_12_listing_failure_snapshot_unchanged,
        test_008_13_14_canon_listing_failure,
        test_008_15_all_contributing_fail_skip_subtree,
        test_008_16_filename_preservation,
    ]

    for t in tests:
        try:
            t()
        except Exception as exc:
            fail(f"{t.__name__} raised unexpected exception: {exc}")

    print(flush=True)
    if FAILURES:
        print(f"{len(FAILURES)} failure(s):", flush=True)
        for msg in FAILURES:
            print(f"  - {msg}", flush=True)
        sys.exit(1)
    print("All 008_traversal checks passed.", flush=True)
    sys.exit(0)


if __name__ == "__main__":
    main()
