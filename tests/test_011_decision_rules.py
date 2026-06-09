# /// script
# requires-python = ">=3.9"
# ///

import sys
import os
import platform
import tempfile
import subprocess
import sqlite3
import time
import datetime
import traceback
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

WORKSPACE = Path("/home/ace/Desktop/prjx/kitchensync")
EXE = WORKSPACE / "released" / "kitchensync.exe"


# --------------------------------------------------------------------------- #
# Helpers                                                                      #
# --------------------------------------------------------------------------- #

def run_sync(peer_args, extra_args=None, timeout=60):
    cmd = [str(EXE)] + list(peer_args) + (extra_args or [])
    r = subprocess.run(
        cmd,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=timeout,
    )
    return r.returncode, r.stdout, r.stderr


def make_peer(base):
    d = Path(base)
    d.mkdir(parents=True, exist_ok=True)
    return d


def write_file(peer_dir, rel_path, content, mtime=None):
    p = Path(peer_dir) / rel_path
    p.parent.mkdir(parents=True, exist_ok=True)
    data = content if isinstance(content, bytes) else content.encode("utf-8")
    p.write_bytes(data)
    if mtime is not None:
        os.utime(str(p), (mtime, mtime))
    return p


def read_bytes(peer_dir, rel_path):
    return (Path(peer_dir) / rel_path).read_bytes()


def snapshot_db_path(peer_dir):
    return Path(peer_dir) / ".kitchensync" / "snapshot.db"


def create_empty_snapshot(peer_dir):
    db = snapshot_db_path(peer_dir)
    db.parent.mkdir(parents=True, exist_ok=True)
    conn = sqlite3.connect(str(db))
    conn.execute("""
        CREATE TABLE IF NOT EXISTS snapshot (
            id TEXT PRIMARY KEY,
            parent_id TEXT,
            basename TEXT NOT NULL,
            mod_time TEXT NOT NULL,
            byte_size INTEGER NOT NULL,
            last_seen TEXT,
            deleted_time TEXT
        )
    """)
    conn.commit()
    conn.close()


def get_snapshot_row(peer_dir, basename):
    """Return first snapshot row (id, mod_time, byte_size, last_seen, deleted_time)."""
    db = snapshot_db_path(peer_dir)
    if not db.exists():
        return None
    conn = sqlite3.connect(str(db))
    row = conn.execute(
        "SELECT id, mod_time, byte_size, last_seen, deleted_time "
        "FROM snapshot WHERE basename = ?",
        (basename,),
    ).fetchone()
    conn.close()
    return row


def update_snapshot_row(peer_dir, row_id, **kwargs):
    db = snapshot_db_path(peer_dir)
    conn = sqlite3.connect(str(db))
    for col, val in kwargs.items():
        conn.execute(f"UPDATE snapshot SET {col} = ? WHERE id = ?", (val, row_id))
    conn.commit()
    conn.close()


def fmt_ts(t):
    """Format Unix timestamp as YYYY-MM-DD_HH-MM-SS_ffffffZ (UTC)."""
    dt = datetime.datetime.fromtimestamp(t, tz=datetime.timezone.utc)
    return dt.strftime("%Y-%m-%d_%H-%M-%S_") + f"{dt.microsecond:06d}Z"


def find_in_bak(peer_dir, basename):
    bak = Path(peer_dir) / ".kitchensync" / "BAK"
    if not bak.exists():
        return None
    for f in bak.rglob("*"):
        if f.is_file() and f.name == basename:
            return f
    return None


FAILURES = []


def check(cond, msg):
    if cond:
        print(f"PASS: {msg}")
    else:
        FAILURES.append(msg)
        print(f"FAIL: {msg}")


# --------------------------------------------------------------------------- #
# 011.1 - Canon has file -> copies to all peers including subordinate         #
# --------------------------------------------------------------------------- #

def test_011_1(tmp):
    a = make_peer(tmp / "a")
    b = make_peer(tmp / "b")
    c = make_peer(tmp / "c")
    write_file(a, "hello.txt", "canon content")
    rc, out, _ = run_sync([f"+{a}", str(b), f"-{c}"])
    check(rc == 0, "011.1 sync exits 0")
    check((b / "hello.txt").exists(), "011.1 file copied to normal peer")
    check((c / "hello.txt").exists(), "011.1 file copied to subordinate peer")
    check(read_bytes(b, "hello.txt") == b"canon content", "011.1 normal peer has canon content")
    check(read_bytes(c, "hello.txt") == b"canon content", "011.1 subordinate peer has canon content")


# --------------------------------------------------------------------------- #
# 011.2 - Canon lacks file -> delete from all other peers                     #
# --------------------------------------------------------------------------- #

def test_011_2(tmp):
    a = make_peer(tmp / "a")
    b = make_peer(tmp / "b")
    c = make_peer(tmp / "c")
    # Establish all three with the file and snapshots via canon A
    for d in [a, b, c]:
        write_file(d, "gone.txt", "to be removed")
    rc1, _, _ = run_sync([f"+{a}", str(b), str(c)])
    check(rc1 == 0, "011.2 setup sync exits 0")
    # Remove from canon, sync again
    (a / "gone.txt").unlink()
    rc2, out2, _ = run_sync([f"+{a}", str(b), str(c)])
    check(rc2 == 0, "011.2 deletion sync exits 0")
    check(not (b / "gone.txt").exists(), "011.2 file removed from peer B")
    check(not (c / "gone.txt").exists(), "011.2 file removed from peer C")
    displaced = (find_in_bak(b, "gone.txt") is not None
                 or find_in_bak(c, "gone.txt") is not None)
    check(displaced, "011.2 displaced file found in BAK on at least one peer")


# --------------------------------------------------------------------------- #
# 011.3 - All contributing unchanged and matching -> no copy                  #
# --------------------------------------------------------------------------- #

def test_011_3(tmp):
    a = make_peer(tmp / "a")
    b = make_peer(tmp / "b")
    t0 = time.time() - 30
    write_file(a, "stable.txt", "same content", mtime=t0)
    write_file(b, "stable.txt", "same content", mtime=t0)
    # Establish snapshots via canon sync
    rc1, _, _ = run_sync([f"+{a}", str(b)])
    check(rc1 == 0, "011.3 setup sync exits 0")
    # Second sync: both unchanged and matching -> no copy
    rc2, out2, _ = run_sync([str(a), str(b)])
    check(rc2 == 0, "011.3 second sync exits 0")
    check("C stable.txt" not in out2, "011.3 no copy line for unchanged matching file")
    check(read_bytes(a, "stable.txt") == b"same content", "011.3 A content unchanged")
    check(read_bytes(b, "stable.txt") == b"same content", "011.3 B content unchanged")


# --------------------------------------------------------------------------- #
# 011.4 - All contributing unchanged matching -> copy to lacking subordinate  #
# --------------------------------------------------------------------------- #

def test_011_4(tmp):
    a = make_peer(tmp / "a")
    b = make_peer(tmp / "b")
    c = make_peer(tmp / "c")
    write_file(a, "shared.txt", "shared content")
    write_file(b, "shared.txt", "shared content")
    # Establish A and B only (C excluded from first sync)
    rc1, _, _ = run_sync([f"+{a}", str(b)])
    check(rc1 == 0, "011.4 setup sync exits 0")
    # Now include C as explicit subordinate (no snapshot -> auto-subordinated anyway)
    rc2, out2, _ = run_sync([str(a), str(b), f"-{c}"])
    check(rc2 == 0, "011.4 second sync exits 0")
    check((c / "shared.txt").exists(), "011.4 file copied to subordinate peer that lacked it")
    check(read_bytes(c, "shared.txt") == b"shared content", "011.4 subordinate has correct content")


# --------------------------------------------------------------------------- #
# 011.5 - Modified versions -> newest mod_time wins                           #
# --------------------------------------------------------------------------- #

def test_011_5(tmp):
    a = make_peer(tmp / "a")
    b = make_peer(tmp / "b")
    t0 = time.time() - 120
    write_file(a, "mod.txt", "initial", mtime=t0)
    write_file(b, "mod.txt", "initial", mtime=t0)
    rc1, _, _ = run_sync([f"+{a}", str(b)])
    check(rc1 == 0, "011.5 setup sync exits 0")
    ta = time.time() - 10   # A is newer
    tb = time.time() - 40   # B is 30s behind A (>5s gap) -> loses
    write_file(a, "mod.txt", "version_A", mtime=ta)
    write_file(b, "mod.txt", "version_B", mtime=tb)
    rc2, _, _ = run_sync([str(a), str(b)])
    check(rc2 == 0, "011.5 sync exits 0")
    check(read_bytes(a, "mod.txt") == b"version_A", "011.5 A keeps newer version")
    check(read_bytes(b, "mod.txt") == b"version_A", "011.5 B gets A's newer version")


# --------------------------------------------------------------------------- #
# 011.6 - New file on contributing peers -> newest mod_time wins              #
# --------------------------------------------------------------------------- #

def test_011_6(tmp):
    a = make_peer(tmp / "a")
    b = make_peer(tmp / "b")
    # Establish snapshots for both peers using an anchor file
    write_file(a, "anchor.txt", "anchor")
    write_file(b, "anchor.txt", "anchor")
    rc1, _, _ = run_sync([f"+{a}", str(b)])
    check(rc1 == 0, "011.6 setup sync exits 0")
    # Add brand-new file after sync -> no snapshot row -> New classification
    ta = time.time() - 5    # A is newer
    tb = time.time() - 25   # B is 20s behind A (>5s) -> loses
    write_file(a, "brand_new.txt", "from_A", mtime=ta)
    write_file(b, "brand_new.txt", "from_B", mtime=tb)
    rc2, _, _ = run_sync([str(a), str(b)])
    check(rc2 == 0, "011.6 sync exits 0")
    check(read_bytes(a, "brand_new.txt") == b"from_A", "011.6 A keeps newer new file")
    check(read_bytes(b, "brand_new.txt") == b"from_A", "011.6 B gets A's newer version")


# --------------------------------------------------------------------------- #
# 011.7 - Multiple deleters -> use most recent deletion estimate              #
# --------------------------------------------------------------------------- #

def test_011_7(tmp):
    # B's deletion estimate is within 5s of mod_time (file would win if B alone deleted)
    # C's deletion estimate exceeds mod_time by >5s (deletion wins if C alone deleted)
    # Rule 011.7: use most recent estimate (C's) -> deletion wins
    a = make_peer(tmp / "a")
    b = make_peer(tmp / "b")
    c = make_peer(tmp / "c")
    t_file = time.time() - 30
    for d in [a, b, c]:
        write_file(d, "mdel.txt", "content", mtime=t_file)
    rc1, _, _ = run_sync([f"+{a}", str(b), str(c)])
    check(rc1 == 0, "011.7 setup sync exits 0")
    b_row = get_snapshot_row(b, "mdel.txt")
    c_row = get_snapshot_row(c, "mdel.txt")
    if not b_row or not c_row:
        FAILURES.append("011.7 snapshot rows missing after setup sync")
        return
    # B: deleted_time = t_file + 2s (within 5s of t_file -> alone: file wins per 011.9)
    update_snapshot_row(b, b_row[0],
        last_seen=fmt_ts(t_file + 2),
        deleted_time=fmt_ts(t_file + 2))
    # C: deleted_time = t_file + 15s (>5s after t_file -> alone: deletion wins per 011.8)
    update_snapshot_row(c, c_row[0],
        last_seen=fmt_ts(t_file + 15),
        deleted_time=fmt_ts(t_file + 15))
    (b / "mdel.txt").unlink()
    (c / "mdel.txt").unlink()
    rc2, out2, _ = run_sync([str(a), str(b), str(c)])
    check(rc2 == 0, "011.7 sync exits 0")
    # Most recent estimate = C's (t_file+15s) > t_file+5s -> deletion wins
    check(not (a / "mdel.txt").exists(),
          "011.7 file displaced from A using most recent deletion estimate among deleters")
    check(find_in_bak(a, "mdel.txt") is not None,
          "011.7 displaced file in A's BAK")


# --------------------------------------------------------------------------- #
# 011.8 - Deletion estimate > mod_time + 5s -> remove file                   #
# --------------------------------------------------------------------------- #

def test_011_8(tmp):
    a = make_peer(tmp / "a")
    b = make_peer(tmp / "b")
    # File's mod_time is far in the past; after first sync last_seen >= now >> mod_time+5s
    t_old = time.time() - 120
    write_file(a, "old.txt", "old data", mtime=t_old)
    write_file(b, "old.txt", "old data", mtime=t_old)
    rc1, _, _ = run_sync([f"+{a}", str(b)])
    check(rc1 == 0, "011.8 setup sync exits 0")
    # B deletes file; absent-unconfirmed; last_seen ~= now >> t_old+5s -> deletion vote
    (b / "old.txt").unlink()
    rc2, out2, _ = run_sync([str(a), str(b)])
    check(rc2 == 0, "011.8 sync exits 0")
    check(not (a / "old.txt").exists(),
          "011.8 file displaced from A (deletion estimate >> mod_time)")
    check(find_in_bak(a, "old.txt") is not None, "011.8 displaced file in A's BAK")
    check("X old.txt" in out2, "011.8 X line emitted for displaced file")


# --------------------------------------------------------------------------- #
# 011.9 - Deletion estimate within 5s of mod_time -> keep file               #
# --------------------------------------------------------------------------- #

def test_011_9(tmp):
    a = make_peer(tmp / "a")
    b = make_peer(tmp / "b")
    t_file = time.time() - 30
    write_file(a, "keep.txt", "keep this", mtime=t_file)
    write_file(b, "keep.txt", "keep this", mtime=t_file)
    rc1, _, _ = run_sync([f"+{a}", str(b)])
    check(rc1 == 0, "011.9 setup sync exits 0")
    b_row = get_snapshot_row(b, "keep.txt")
    if not b_row:
        FAILURES.append("011.9 snapshot row missing after setup sync")
        return
    # Set B as Deleted with estimate = t_file + 2s (within 5s of t_file -> file wins)
    update_snapshot_row(b, b_row[0],
        last_seen=fmt_ts(t_file + 2),
        deleted_time=fmt_ts(t_file + 2))
    (b / "keep.txt").unlink()
    rc2, out2, _ = run_sync([str(a), str(b)])
    check(rc2 == 0, "011.9 sync exits 0")
    check((a / "keep.txt").exists(),
          "011.9 file kept on A (deletion estimate within 5s of mod_time)")
    check((b / "keep.txt").exists(), "011.9 file re-copied to B")
    check(find_in_bak(a, "keep.txt") is None, "011.9 A's file not displaced")
    check("C keep.txt" in out2, "011.9 C line for copy to B")


# --------------------------------------------------------------------------- #
# 011.10 - Absent-unconfirmed: last_seen > max mod_time + 5s -> deletion     #
# --------------------------------------------------------------------------- #

def test_011_10(tmp):
    a = make_peer(tmp / "a")
    b = make_peer(tmp / "b")
    # File mod_time well in the past; first sync sets last_seen ~= now >> mod_time+5s
    t_old = time.time() - 120
    write_file(a, "absentold.txt", "old data", mtime=t_old)
    write_file(b, "absentold.txt", "old data", mtime=t_old)
    rc1, _, _ = run_sync([f"+{a}", str(b)])
    check(rc1 == 0, "011.10 setup sync exits 0")
    # Delete B's file WITHOUT another sync: B absent-unconfirmed (last_seen set, deleted_time NULL)
    # last_seen ~= now >> t_old+5s -> rule 4b: counts as deletion estimate -> deletion wins
    (b / "absentold.txt").unlink()
    rc2, out2, _ = run_sync([str(a), str(b)])
    check(rc2 == 0, "011.10 sync exits 0")
    check(not (a / "absentold.txt").exists(),
          "011.10 A's file displaced (last_seen >> max mod_time by >5s)")
    check(find_in_bak(a, "absentold.txt") is not None, "011.10 displaced file in A's BAK")


# --------------------------------------------------------------------------- #
# 011.11 - Absent-unconfirmed: last_seen <= max mod_time + 5s -> re-copy     #
# --------------------------------------------------------------------------- #

def test_011_11(tmp):
    a = make_peer(tmp / "a")
    b = make_peer(tmp / "b")
    # File mod_time = now; first sync sets last_seen ~= now (within a few seconds)
    # last_seen - mod_time < 5s -> rule 4b: no deletion vote, re-copy
    # This test assumes the two local syncs together complete within 4 seconds.
    t_now = time.time()
    write_file(a, "absentnew.txt", "new data", mtime=t_now)
    write_file(b, "absentnew.txt", "new data", mtime=t_now)
    rc1, _, _ = run_sync([f"+{a}", str(b)])
    check(rc1 == 0, "011.11 setup sync exits 0")
    # Delete B's file immediately; B is absent-unconfirmed
    (b / "absentnew.txt").unlink()
    rc2, out2, _ = run_sync([str(a), str(b)])
    check(rc2 == 0, "011.11 sync exits 0")
    check((b / "absentnew.txt").exists(),
          "011.11 file re-copied to B (no deletion vote: last_seen within 5s of mod_time)")
    check((a / "absentnew.txt").exists(), "011.11 A's file untouched")
    check("C absentnew.txt" in out2, "011.11 C line for re-copy to B")


# --------------------------------------------------------------------------- #
# 011.12 - Same mod_time, different byte_size -> larger file wins             #
# --------------------------------------------------------------------------- #

def test_011_12(tmp):
    a = make_peer(tmp / "a")
    b = make_peer(tmp / "b")
    t0 = time.time() - 60
    write_file(a, "sized.txt", "initial", mtime=t0)
    write_file(b, "sized.txt", "initial", mtime=t0)
    rc1, _, _ = run_sync([f"+{a}", str(b)])
    check(rc1 == 0, "011.12 setup sync exits 0")
    same_t = time.time() - 10
    write_file(a, "sized.txt", "small",          mtime=same_t)   # 5 bytes
    write_file(b, "sized.txt", "larger_content", mtime=same_t)   # 14 bytes
    rc2, _, _ = run_sync([str(a), str(b)])
    check(rc2 == 0, "011.12 sync exits 0")
    check(read_bytes(a, "sized.txt") == b"larger_content",
          "011.12 A gets the larger version")
    check(read_bytes(b, "sized.txt") == b"larger_content",
          "011.12 B keeps its larger version")


# --------------------------------------------------------------------------- #
# 011.13 + 011.14 - Peer with no snapshot row: doesn't vote, receives winner #
# --------------------------------------------------------------------------- #

def test_011_13_14(tmp):
    a = make_peer(tmp / "a")
    b = make_peer(tmp / "b")
    c = make_peer(tmp / "c")
    t0 = time.time() - 60
    write_file(a, "vote.txt", "initial", mtime=t0)
    write_file(b, "vote.txt", "initial", mtime=t0)
    # Establish A and B only; C is not synced so it has no snapshot row for vote.txt
    rc1, _, _ = run_sync([f"+{a}", str(b)])
    check(rc1 == 0, "011.13/14 setup sync exits 0")
    # Give C an empty snapshot.db (has history but no row for vote.txt -> doesn't vote)
    create_empty_snapshot(c)
    # Modify: A newer (wins), B older (loses); >5s gap ensures clear winner
    ta = time.time() - 10
    tb = time.time() - 40   # 30s behind A -> loses
    write_file(a, "vote.txt", "A_wins",  mtime=ta)
    write_file(b, "vote.txt", "B_loses", mtime=tb)
    rc2, _, _ = run_sync([str(a), str(b), str(c)])
    check(rc2 == 0, "011.13/14 sync exits 0")
    check(read_bytes(a, "vote.txt") == b"A_wins",
          "011.13 A keeps winning version (C's absence from vote didn't change outcome)")
    check(read_bytes(b, "vote.txt") == b"A_wins",
          "011.13 B gets winner regardless of C having no snapshot row")
    check((c / "vote.txt").exists(),
          "011.14 C receives winning file despite having no snapshot row")
    check(read_bytes(c, "vote.txt") == b"A_wins",
          "011.14 C gets the correct winning content")


# --------------------------------------------------------------------------- #
# 011.15 - Winning file already on peer -> no copy to that peer               #
# --------------------------------------------------------------------------- #

def test_011_15(tmp):
    a = make_peer(tmp / "a")
    b = make_peer(tmp / "b")
    c = make_peer(tmp / "c")
    t_old = time.time() - 120
    for d in [a, b, c]:
        write_file(d, "nocopy.txt", "initial", mtime=t_old)
    rc1, _, _ = run_sync([f"+{a}", str(b), str(c)])
    check(rc1 == 0, "011.15 setup sync exits 0")
    # Update A and B to same new content at same mod_time; C stays on old version
    new_t = time.time() - 10
    new_content = b"new_version_content_xyz"
    (a / "nocopy.txt").write_bytes(new_content)
    (b / "nocopy.txt").write_bytes(new_content)
    os.utime(str(a / "nocopy.txt"), (new_t, new_t))
    os.utime(str(b / "nocopy.txt"), (new_t, new_t))
    rc2, out2, _ = run_sync([str(a), str(b), str(c)])
    check(rc2 == 0, "011.15 sync exits 0")
    check(read_bytes(b, "nocopy.txt") == new_content,
          "011.15 B still has winning content (not unnecessarily overwritten)")
    check(read_bytes(c, "nocopy.txt") == new_content,
          "011.15 C received winning content")
    # A and B already match the winner -> at most one C line (for C's copy only)
    c_lines = [l for l in out2.splitlines() if l == "C nocopy.txt"]
    check(len(c_lines) <= 1,
          "011.15 no redundant copy lines for peers that already match the winner")


# --------------------------------------------------------------------------- #
# 011.16 - mod_time within 5s -> treated as tied -> larger byte_size wins    #
# --------------------------------------------------------------------------- #

def test_011_16(tmp):
    a = make_peer(tmp / "a")
    b = make_peer(tmp / "b")
    t0 = time.time() - 60
    write_file(a, "tied.txt", "initial", mtime=t0)
    write_file(b, "tied.txt", "initial", mtime=t0)
    rc1, _, _ = run_sync([f"+{a}", str(b)])
    check(rc1 == 0, "011.16 setup sync exits 0")
    t_max = time.time() - 20
    t_tied = t_max - 3   # 3s behind max -> within 5s tolerance -> tied on time
    write_file(a, "tied.txt", "small",         mtime=t_max)    # A: newer time, 5 bytes
    write_file(b, "tied.txt", "bigger_content", mtime=t_tied)  # B: within 5s, 14 bytes
    rc2, _, _ = run_sync([str(a), str(b)])
    check(rc2 == 0, "011.16 sync exits 0")
    # Tied on time -> rule 5: larger wins -> B's 14-byte version wins
    check(read_bytes(a, "tied.txt") == b"bigger_content",
          "011.16 A gets larger version (B's) when mod_times are within 5s")
    check(read_bytes(b, "tied.txt") == b"bigger_content",
          "011.16 B keeps larger version")


# --------------------------------------------------------------------------- #
# 011.17 - mod_time more than 5s behind maximum -> loses                      #
# --------------------------------------------------------------------------- #

def test_011_17(tmp):
    a = make_peer(tmp / "a")
    b = make_peer(tmp / "b")
    t0 = time.time() - 60
    write_file(a, "newer.txt", "initial", mtime=t0)
    write_file(b, "newer.txt", "initial", mtime=t0)
    rc1, _, _ = run_sync([f"+{a}", str(b)])
    check(rc1 == 0, "011.17 setup sync exits 0")
    ta = time.time() - 10
    tb = ta - 20   # B is 20s behind A -> >5s -> B loses
    write_file(a, "newer.txt", "A_newer", mtime=ta)
    write_file(b, "newer.txt", "B_older", mtime=tb)
    rc2, _, _ = run_sync([str(a), str(b)])
    check(rc2 == 0, "011.17 sync exits 0")
    check(read_bytes(a, "newer.txt") == b"A_newer",
          "011.17 A keeps its newer version")
    check(read_bytes(b, "newer.txt") == b"A_newer",
          "011.17 B gets A's version (B's mod_time was >5s behind)")


# --------------------------------------------------------------------------- #
# Main                                                                         #
# --------------------------------------------------------------------------- #

def main():
    with tempfile.TemporaryDirectory() as td:
        base = Path(td)
        tests = [
            ("011.1",      test_011_1),
            ("011.2",      test_011_2),
            ("011.3",      test_011_3),
            ("011.4",      test_011_4),
            ("011.5",      test_011_5),
            ("011.6",      test_011_6),
            ("011.7",      test_011_7),
            ("011.8",      test_011_8),
            ("011.9",      test_011_9),
            ("011.10",     test_011_10),
            ("011.11",     test_011_11),
            ("011.12",     test_011_12),
            ("011.13+14",  test_011_13_14),
            ("011.15",     test_011_15),
            ("011.16",     test_011_16),
            ("011.17",     test_011_17),
        ]
        for name, fn in tests:
            print(f"\n--- {name} ---")
            try:
                fn(base / name.replace("+", "_").replace(".", "_"))
            except Exception as e:
                FAILURES.append(f"{name} raised: {e}")
                print(f"ERROR in {name}: {e}")
                traceback.print_exc()

    if FAILURES:
        print(f"\n{len(FAILURES)} failure(s):")
        for f in FAILURES:
            print(f"  FAIL: {f}")
        sys.exit(1)
    print("\nAll checks passed.")
    sys.exit(0)


if __name__ == "__main__":
    main()
