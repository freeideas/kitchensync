# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""
End-to-end test for 017_snapshot-updates.

Verifies per-peer snapshot row transitions during sync runs:
  - confirmed-present upserts (017.1-017.4)
  - push-decision + completed-copy state (017.8-017.10, 017.12)
  - confirmed-absent tombstoning (017.5-017.6)
  - tombstone idempotency (017.7)
  - directory-creation last_seen (017.13)
  - displacement sets deleted_time (017.15)
  - displacement cascade (017.16-017.17)
  - cascade skips pre-existing tombstones (017.18)
  - per-peer cascade isolation (017.19-017.20)

Not reasonably testable end-to-end:
  017.11 -- last_seen=NULL between push-decision and copy-complete is transient;
            not observable in a completed run
  017.14 -- snapshot unchanged when inline op fails requires engineering a
            filesystem failure, which tests must not do
  017.21 -- deleted_time=NULL on interrupted copy requires a mid-run process kill
  017.22 -- last_seen unchanged on interrupted copy; same as 017.21
"""

import re
import shutil
import sqlite3
import subprocess
import sys
import tempfile
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

WORKSPACE = Path("/home/ace/Desktop/prjx/kitchensync")
EXE = WORKSPACE / "released" / "kitchensync.exe"
TIMESTAMP_RE = re.compile(r"^\d{4}-\d{2}-\d{2}_\d{2}-\d{2}-\d{2}_\d{6}Z$")

_failures: list[str] = []


def _fail(msg: str) -> None:
    _failures.append(msg)
    print(f"  FAIL: {msg}")


def _ks(*args: str, timeout: int = 90) -> subprocess.CompletedProcess:
    return subprocess.run(
        [str(EXE), *args],
        capture_output=True,
        text=True,
        timeout=timeout,
    )


def _rows(peer_dir: Path, basename: str) -> list[dict]:
    db = peer_dir / ".kitchensync" / "snapshot.db"
    if not db.exists():
        return []
    with sqlite3.connect(str(db)) as conn:
        conn.row_factory = sqlite3.Row
        cur = conn.execute("SELECT * FROM snapshot WHERE basename=?", (basename,))
        return [dict(r) for r in cur.fetchall()]


def _one(peer_dir: Path, basename: str, ctx: str) -> "dict | None":
    rs = _rows(peer_dir, basename)
    if not rs:
        _fail(f"{ctx}: no snapshot row for {basename!r}")
        return None
    if len(rs) > 1:
        _fail(f"{ctx}: {len(rs)} rows for {basename!r}, expected 1")
        return None
    return rs[0]


def _is_ts(val: object) -> bool:
    return isinstance(val, str) and bool(TIMESTAMP_RE.match(val))


# ---------------------------------------------------------------------------
# Scenario A: confirmed-present (017.1-017.4) and push+copy (017.8-017.10, 017.12)
# ---------------------------------------------------------------------------

def _scenario_a(tmp: Path) -> None:
    print("Scenario A: confirmed-present (017.1-017.4) and push+copy (017.8-017.10, 017.12)")
    src = tmp / "a_src"
    dst = tmp / "a_dst"
    src.mkdir()
    dst.mkdir()

    content = b"kitchensync snapshot test content - alpha"
    (src / "alpha.txt").write_bytes(content)
    expected_size = len(content)

    r = _ks(f"+{src}", str(dst))
    if r.returncode != 0:
        _fail(f"A: kitchensync exited {r.returncode}; stdout={r.stdout!r}")
        return

    # 017.1-017.4: source (listed, confirmed-present) snapshot row
    row_src = _one(src, "alpha.txt", "A/src")
    if row_src:
        if not _is_ts(row_src["mod_time"]):
            _fail(f"017.1: src alpha.txt mod_time not a valid timestamp: {row_src['mod_time']!r}")
        if row_src["byte_size"] != expected_size:
            _fail(
                f"017.2: src alpha.txt byte_size={row_src['byte_size']}, "
                f"expected {expected_size}"
            )
        if not _is_ts(row_src["last_seen"]):
            _fail(f"017.3: src alpha.txt last_seen not set: {row_src['last_seen']!r}")
        if row_src["deleted_time"] is not None:
            _fail(
                f"017.4: src alpha.txt deleted_time={row_src['deleted_time']!r}, "
                "expected NULL"
            )

    # 017.8-017.10, 017.12: destination snapshot (push decision + completed copy)
    row_dst = _one(dst, "alpha.txt", "A/dst")
    if row_dst:
        if not _is_ts(row_dst["mod_time"]):
            _fail(f"017.8: dst alpha.txt mod_time not a valid timestamp: {row_dst['mod_time']!r}")
        if row_dst["byte_size"] != expected_size:
            _fail(
                f"017.9: dst alpha.txt byte_size={row_dst['byte_size']}, "
                f"expected {expected_size}"
            )
        if row_dst["deleted_time"] is not None:
            _fail(
                f"017.10: dst alpha.txt deleted_time={row_dst['deleted_time']!r}, "
                "expected NULL"
            )
        if not _is_ts(row_dst["last_seen"]):
            _fail(
                f"017.12: dst alpha.txt last_seen not set after completed copy: "
                f"{row_dst['last_seen']!r}"
            )

    # winning mod_time propagated intact to destination (017.8)
    if (
        row_src and row_dst
        and _is_ts(row_src["mod_time"])
        and _is_ts(row_dst["mod_time"])
    ):
        if row_src["mod_time"] != row_dst["mod_time"]:
            _fail(
                f"017.8: dst mod_time {row_dst['mod_time']!r} != "
                f"src mod_time {row_src['mod_time']!r}"
            )


# ---------------------------------------------------------------------------
# Scenario B+C: confirmed-absent (017.5-017.6) and tombstone idempotent (017.7)
#
# Uses a canon peer so the deletion decision is unambiguous:
#   Run 1 -- canon has beta.txt, dst gets it; both have snapshots.
#   Run 2 -- canon loses beta.txt; dst's copy is displaced; tombstone written.
#   Run 3 -- beta.txt still absent; tombstone must be left unchanged (017.7).
# ---------------------------------------------------------------------------

def _scenario_bc(tmp: Path) -> None:
    print("Scenario B/C: confirmed-absent (017.5-017.6) and tombstone idempotent (017.7)")
    src = tmp / "bc_src"
    dst = tmp / "bc_dst"
    src.mkdir()
    dst.mkdir()

    beta = src / "beta.txt"
    beta.write_bytes(b"beta file for confirmed-absent test")

    # Run 1: establish snapshot rows on both peers.
    r = _ks(f"+{src}", str(dst))
    if r.returncode != 0:
        _fail(f"B: run1 exited {r.returncode}")
        return

    row_dst_r1 = _one(dst, "beta.txt", "B/run1")
    if row_dst_r1 is None:
        return
    if not _is_ts(row_dst_r1["last_seen"]):
        _fail(f"B: run1 dst beta.txt last_seen not a timestamp; cannot continue")
        return
    ls_dst_r1 = row_dst_r1["last_seen"]

    # Remove beta.txt from canon; dst still has it.
    beta.unlink()

    # Run 2: canon lacks beta.txt → displace from dst.
    r = _ks(f"+{src}", str(dst))
    if r.returncode != 0:
        _fail(f"B: run2 exited {r.returncode}")
        return

    row_dst_r2 = _one(dst, "beta.txt", "B/run2")
    if row_dst_r2 is None:
        return

    # 017.5: deleted_time must equal the row's last_seen at the time of displacement.
    if row_dst_r2["deleted_time"] != ls_dst_r1:
        _fail(
            f"017.5: dst beta.txt deleted_time={row_dst_r2['deleted_time']!r}, "
            f"expected row's last_seen from run1={ls_dst_r1!r}"
        )
    # 017.6: last_seen must not be updated.
    if row_dst_r2["last_seen"] != ls_dst_r1:
        _fail(
            f"017.6: dst beta.txt last_seen changed from {ls_dst_r1!r} "
            f"to {row_dst_r2['last_seen']!r} (must remain unchanged)"
        )

    dt_r2 = row_dst_r2["deleted_time"]
    ls_r2 = row_dst_r2["last_seen"]

    # Run 3: tombstone already set; row must be left unchanged (017.7).
    r = _ks(f"+{src}", str(dst))
    if r.returncode != 0:
        _fail(f"C: run3 exited {r.returncode}")
        return

    row_dst_r3 = _one(dst, "beta.txt", "C/run3")
    if row_dst_r3 is None:
        return
    if row_dst_r3["deleted_time"] != dt_r2:
        _fail(
            f"017.7: dst beta.txt deleted_time changed from {dt_r2!r} "
            f"to {row_dst_r3['deleted_time']!r} on run3 (must be idempotent)"
        )
    if row_dst_r3["last_seen"] != ls_r2:
        _fail(
            f"017.7: dst beta.txt last_seen changed from {ls_r2!r} "
            f"to {row_dst_r3['last_seen']!r} on run3 (must be idempotent)"
        )


# ---------------------------------------------------------------------------
# Scenario D: directory-creation last_seen (017.13)
# ---------------------------------------------------------------------------

def _scenario_d(tmp: Path) -> None:
    print("Scenario D: directory-creation last_seen (017.13)")
    src = tmp / "d_src"
    dst = tmp / "d_dst"
    src.mkdir()
    dst.mkdir()

    subdir = src / "syncdir"
    subdir.mkdir()
    (subdir / "content.txt").write_bytes(b"directory test content")

    r = _ks(f"+{src}", str(dst))
    if r.returncode != 0:
        _fail(f"D: kitchensync exited {r.returncode}")
        return

    # 017.13: after inline directory creation succeeds, dst row has last_seen set.
    row_dir = _one(dst, "syncdir", "D")
    if row_dir:
        if row_dir["byte_size"] != -1:
            _fail(
                f"017.13: dst syncdir byte_size={row_dir['byte_size']}, "
                "expected -1 for directory"
            )
        if not _is_ts(row_dir["last_seen"]):
            _fail(
                f"017.13: dst syncdir last_seen not set after directory creation: "
                f"{row_dir['last_seen']!r}"
            )
        if row_dir["deleted_time"] is not None:
            _fail(
                f"017.13: dst syncdir deleted_time={row_dir['deleted_time']!r}, "
                "expected NULL"
            )


# ---------------------------------------------------------------------------
# Scenario E: displacement sets deleted_time (017.15) and cascade (017.16-017.17)
#
# Run 1: sync treedir/ (ea.txt, eb.txt) and unrelated.txt to dst.
# Run 2: canon loses treedir/ → displacement + cascade on dst.
# ---------------------------------------------------------------------------

def _scenario_e(tmp: Path) -> None:
    print("Scenario E: displacement (017.15) and cascade (017.16-017.17)")
    src = tmp / "e_src"
    dst = tmp / "e_dst"
    src.mkdir()
    dst.mkdir()

    treedir = src / "treedir"
    treedir.mkdir()
    (treedir / "ea.txt").write_bytes(b"cascade descendant ea")
    (treedir / "eb.txt").write_bytes(b"cascade descendant eb")
    (src / "unrelated.txt").write_bytes(b"unrelated - must not be touched by cascade")

    # Run 1: sync everything to dst.
    r = _ks(f"+{src}", str(dst))
    if r.returncode != 0:
        _fail(f"E: run1 exited {r.returncode}")
        return

    row_dir_r1 = _one(dst, "treedir", "E/run1-dir")
    if row_dir_r1 is None:
        return
    if not _is_ts(row_dir_r1["last_seen"]):
        _fail(f"E: run1 dst treedir last_seen not a timestamp; cannot continue")
        return
    dir_ls = row_dir_r1["last_seen"]

    # Remove treedir/ entirely from canon.
    shutil.rmtree(str(treedir))

    # Run 2: canon lacks treedir/ → displace from dst + cascade descendants.
    r = _ks(f"+{src}", str(dst))
    if r.returncode != 0:
        _fail(f"E: run2 exited {r.returncode}")
        return

    # 017.15: displaced directory row gets deleted_time = its last_seen.
    row_dir_r2 = _one(dst, "treedir", "E/run2-dir")
    if row_dir_r2:
        if row_dir_r2["deleted_time"] != dir_ls:
            _fail(
                f"017.15: dst treedir deleted_time={row_dir_r2['deleted_time']!r}, "
                f"expected treedir's last_seen={dir_ls!r}"
            )

    # 017.16: descendant rows get deleted_time set by cascade; cascade reuses
    # the displaced directory's deletion estimate (its last_seen).
    for basename in ("ea.txt", "eb.txt"):
        row_child = _one(dst, basename, f"E/run2-{basename}")
        if row_child:
            if row_child["deleted_time"] is None:
                _fail(
                    f"017.16: dst {basename} deleted_time is NULL "
                    "after displacement cascade (expected set)"
                )
            elif row_child["deleted_time"] != dir_ls:
                _fail(
                    f"017.16: dst {basename} cascade deleted_time="
                    f"{row_child['deleted_time']!r}, expected {dir_ls!r} "
                    "(displaced directory's last_seen)"
                )

    # 017.17: unrelated row must not be touched.
    row_unrel = _one(dst, "unrelated.txt", "E/run2-unrelated")
    if row_unrel:
        if row_unrel["deleted_time"] is not None:
            _fail(
                f"017.17: dst unrelated.txt deleted_time={row_unrel['deleted_time']!r}, "
                "expected NULL (cascade must only reach descendants of displaced entry)"
            )


# ---------------------------------------------------------------------------
# Scenario F: cascade must not overwrite pre-existing tombstoned descendants (017.18)
#
# Run 1: sync cascdir/ (ca.txt, cb.txt) to dst.
# Run 2: canon loses cb.txt → cb.txt tombstoned on dst with some deleted_time.
# Run 3: canon loses cascdir/ → cascade runs; ca.txt is tombstoned, cb.txt unchanged.
# ---------------------------------------------------------------------------

def _scenario_f(tmp: Path) -> None:
    print("Scenario F: cascade does not overwrite pre-tombstoned descendants (017.18)")
    src = tmp / "f_src"
    dst = tmp / "f_dst"
    src.mkdir()
    dst.mkdir()

    cascdir = src / "cascdir"
    cascdir.mkdir()
    (cascdir / "ca.txt").write_bytes(b"cascade file ca - will be cascade-tombstoned")
    (cascdir / "cb.txt").write_bytes(b"cascade file cb - pre-tombstoned before dir delete")

    # Run 1: sync cascdir/ with both files to dst.
    r = _ks(f"+{src}", str(dst))
    if r.returncode != 0:
        _fail(f"F: run1 exited {r.returncode}")
        return

    # Remove cb.txt from canon; run 2 displaces it from dst.
    (cascdir / "cb.txt").unlink()

    r = _ks(f"+{src}", str(dst))
    if r.returncode != 0:
        _fail(f"F: run2 exited {r.returncode}")
        return

    row_cb_r2 = _one(dst, "cb.txt", "F/run2-cb")
    if row_cb_r2 is None:
        return
    if row_cb_r2["deleted_time"] is None:
        _fail("F: run2 dst cb.txt not tombstoned; cannot test 017.18")
        return
    dt_cb_before_cascade = row_cb_r2["deleted_time"]

    # Record cascdir's last_seen after run2; cascade in run3 will use this as
    # deleted_time for non-tombstoned descendants.
    row_dir_r2 = _one(dst, "cascdir", "F/run2-dir")
    if row_dir_r2 is None:
        return
    if not _is_ts(row_dir_r2["last_seen"]):
        _fail(f"F: run2 dst cascdir last_seen not a timestamp; cannot continue")
        return
    dir_ls_r2 = row_dir_r2["last_seen"]

    # Remove entire cascdir/ from canon; run 3 triggers displacement + cascade.
    shutil.rmtree(str(cascdir))

    r = _ks(f"+{src}", str(dst))
    if r.returncode != 0:
        _fail(f"F: run3 exited {r.returncode}")
        return

    # ca.txt had no prior tombstone; cascade must tombstone it using dir's last_seen.
    row_ca_r3 = _one(dst, "ca.txt", "F/run3-ca")
    if row_ca_r3:
        if row_ca_r3["deleted_time"] is None:
            _fail("F: dst ca.txt not tombstoned by cascade (expected set)")
        elif row_ca_r3["deleted_time"] != dir_ls_r2:
            _fail(
                f"F: dst ca.txt cascade deleted_time={row_ca_r3['deleted_time']!r}, "
                f"expected {dir_ls_r2!r} (cascdir's last_seen at displacement)"
            )

    # 017.18: cb.txt had deleted_time set; cascade must not overwrite it.
    row_cb_r3 = _one(dst, "cb.txt", "F/run3-cb")
    if row_cb_r3:
        if row_cb_r3["deleted_time"] != dt_cb_before_cascade:
            _fail(
                f"017.18: dst cb.txt deleted_time changed from "
                f"{dt_cb_before_cascade!r} to {row_cb_r3['deleted_time']!r} "
                "by cascade (must not overwrite existing tombstone)"
            )


# ---------------------------------------------------------------------------
# Scenario G: per-peer cascade isolation (017.19-017.20)
#
# Three peers: canon src, peerB, peerC.
# Run 1: msdir/ (with ma.txt) synced to peerB and peerC; both get snapshots.
# Run 2: canon loses msdir/ → displace on peerB and peerC; each peer's cascade
#         runs against its own snapshot.db and uses its own last_seen.
# ---------------------------------------------------------------------------

def _scenario_g(tmp: Path) -> None:
    print("Scenario G: per-peer cascade isolation (017.19-017.20)")
    src = tmp / "g_src"
    pb  = tmp / "g_b"
    pc  = tmp / "g_c"
    src.mkdir()
    pb.mkdir()
    pc.mkdir()

    msdir = src / "msdir"
    msdir.mkdir()
    (msdir / "ma.txt").write_bytes(b"multi-peer cascade isolation test file")

    # Run 1: sync to peerB and peerC (both auto-subordinate on first run).
    r = _ks(f"+{src}", str(pb), str(pc))
    if r.returncode != 0:
        _fail(f"G: run1 exited {r.returncode}")
        return

    row_b_r1 = _one(pb, "msdir", "G/run1-b-dir")
    row_c_r1 = _one(pc, "msdir", "G/run1-c-dir")
    if row_b_r1 is None or row_c_r1 is None:
        return

    dir_ls_b = row_b_r1["last_seen"]
    dir_ls_c = row_c_r1["last_seen"]
    if not (_is_ts(dir_ls_b) and _is_ts(dir_ls_c)):
        _fail(
            f"G: run1 msdir last_seen not valid timestamps: "
            f"peerB={dir_ls_b!r} peerC={dir_ls_c!r}"
        )
        return

    # Remove msdir/ from canon.
    shutil.rmtree(str(msdir))

    # Run 2: canon lacks msdir/ → displace from peerB and peerC; cascade in each DB.
    r = _ks(f"+{src}", str(pb), str(pc))
    if r.returncode != 0:
        _fail(f"G: run2 exited {r.returncode}")
        return

    # 017.19, 017.20: peerB's cascade ran against peerB's own DB, using peerB's
    #                  last_seen as the deletion estimate.
    row_dir_b = _one(pb, "msdir", "G/run2-b-dir")
    if row_dir_b:
        if row_dir_b["deleted_time"] != dir_ls_b:
            _fail(
                f"017.20: peerB msdir deleted_time={row_dir_b['deleted_time']!r}, "
                f"expected peerB's own last_seen={dir_ls_b!r}"
            )
    row_ma_b = _one(pb, "ma.txt", "G/run2-b-ma")
    if row_ma_b:
        if row_ma_b["deleted_time"] is None:
            _fail("017.20: peerB ma.txt not cascade-tombstoned in peerB's DB")
        elif row_ma_b["deleted_time"] != dir_ls_b:
            # 017.19: peerB's cascade must use peerB's own dir last_seen, not peerC's.
            _fail(
                f"017.19: peerB ma.txt cascade deleted_time="
                f"{row_ma_b['deleted_time']!r}, expected peerB's dir "
                f"last_seen={dir_ls_b!r} (not peerC's {dir_ls_c!r})"
            )

    # peerC's cascade must have run against peerC's own DB, using peerC's last_seen.
    row_dir_c = _one(pc, "msdir", "G/run2-c-dir")
    if row_dir_c:
        if row_dir_c["deleted_time"] != dir_ls_c:
            _fail(
                f"017.20: peerC msdir deleted_time={row_dir_c['deleted_time']!r}, "
                f"expected peerC's own last_seen={dir_ls_c!r}"
            )
    row_ma_c = _one(pc, "ma.txt", "G/run2-c-ma")
    if row_ma_c:
        if row_ma_c["deleted_time"] is None:
            _fail("017.20: peerC ma.txt not cascade-tombstoned in peerC's DB")
        elif row_ma_c["deleted_time"] != dir_ls_c:
            # 017.19: peerC's cascade must use peerC's own dir last_seen, not peerB's.
            _fail(
                f"017.19: peerC ma.txt cascade deleted_time="
                f"{row_ma_c['deleted_time']!r}, expected peerC's dir "
                f"last_seen={dir_ls_c!r} (not peerB's {dir_ls_b!r})"
            )


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main() -> None:
    if not EXE.exists():
        print(f"ERROR: executable not found: {EXE}", file=sys.stderr)
        sys.exit(1)

    tmp = Path(tempfile.mkdtemp(prefix="ks017_"))
    try:
        _scenario_a(tmp)
        _scenario_bc(tmp)
        _scenario_d(tmp)
        _scenario_e(tmp)
        _scenario_f(tmp)
        _scenario_g(tmp)
    finally:
        shutil.rmtree(str(tmp), ignore_errors=True)

    print()
    if _failures:
        print(f"{len(_failures)} check(s) FAILED:")
        for msg in _failures:
            print(f"  - {msg}")
        sys.exit(1)
    else:
        print("All checks passed.")
        sys.exit(0)


if __name__ == "__main__":
    main()
