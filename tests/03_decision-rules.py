#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Decision rules (03.1–03.92): per-file decisions based on peer states and snapshot rows."""

from __future__ import annotations

import datetime, os, shutil, sqlite3, subprocess, sys, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

TMP = Path(PROJECT) / "tmp" / "testks" / "03_decision-rules"


def _run(*peer_args, timeout=60):
    return subprocess.run(
        [str(UV), "run", "--script", str(BUILD_PY), "invoke-cli", PROJECT, *peer_args],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        text=True, encoding="utf-8",
        timeout=timeout,
    )


def _url(peer_dir: Path) -> str:
    return peer_dir.resolve().as_uri()


def _in_bak(peer_dir: Path, name: str) -> bool:
    bak_root = peer_dir / ".kitchensync" / "BAK"
    if not bak_root.exists():
        return False
    for ts_dir in bak_root.iterdir():
        if (ts_dir / name).exists():
            return True
    return False


def _snap_db(peer_dir: Path) -> Path:
    return peer_dir / ".kitchensync" / "snapshot.db"


def _snap_ts(when: float) -> str:
    dt = datetime.datetime.utcfromtimestamp(when)
    return dt.strftime("%Y-%m-%d_%H-%M-%S_") + f"{dt.microsecond:06d}Z"


def _set_deleted(snap_db: Path, basename: str, deleted_ts: str) -> None:
    with sqlite3.connect(str(snap_db)) as con:
        con.execute(
            "UPDATE snapshot SET deleted_time = ? WHERE basename = ?",
            (deleted_ts, basename),
        )
        con.commit()
        con.execute("PRAGMA wal_checkpoint(TRUNCATE)")


def _set_last_seen_null(snap_db: Path, basename: str) -> None:
    with sqlite3.connect(str(snap_db)) as con:
        con.execute(
            "UPDATE snapshot SET last_seen = NULL WHERE basename = ?",
            (basename,),
        )
        con.commit()
        con.execute("PRAGMA wal_checkpoint(TRUNCATE)")


def _set_last_seen(snap_db: Path, basename: str, last_seen_ts: str) -> None:
    with sqlite3.connect(str(snap_db)) as con:
        con.execute(
            "UPDATE snapshot SET last_seen = ? WHERE basename = ?",
            (last_seen_ts, basename),
        )
        con.commit()
        con.execute("PRAGMA wal_checkpoint(TRUNCATE)")


def _get_snap_row(snap_db: Path, basename: str):
    with sqlite3.connect(str(snap_db)) as con:
        return con.execute(
            "SELECT deleted_time, last_seen FROM snapshot WHERE basename = ?",
            (basename,),
        ).fetchone()


def _get_last_seen(snap_db: Path, basename: str):
    row = _get_snap_row(snap_db, basename)
    return row[1] if row else None


def _utime(path: Path, ts: float) -> None:
    os.utime(path, (ts, ts))


def _peers(*names, base: Path) -> list[Path]:
    dirs = [base / n for n in names]
    for d in dirs:
        d.mkdir(parents=True, exist_ok=True)
    return dirs


def main() -> int:
    if TMP.exists():
        shutil.rmtree(TMP)
    TMP.mkdir(parents=True)

    failures: list[str] = []
    now = time.time()

    try:
        # ── 03.1: All contributing peers agree → no copy enqueued ──────────────
        p1, p2 = _peers("t01/p1", "t01/p2", base=TMP)
        (p1 / "same.txt").write_text("identical", encoding="utf-8")
        (p2 / "same.txt").write_text("identical", encoding="utf-8")
        _utime(p1 / "same.txt", now - 20)
        _utime(p2 / "same.txt", now - 20)
        _run("+" + _url(p1), _url(p2))          # establish snapshots
        r = _run(_url(p1), _url(p2))             # bidirectional — should be no-op
        p1c = (p1 / "same.txt").read_text(encoding="utf-8") if (p1 / "same.txt").exists() else None
        p2c = (p2 / "same.txt").read_text(encoding="utf-8") if (p2 / "same.txt").exists() else None
        print(f"[03.1] p1={p1c!r} p2={p2c!r} exit={r.returncode}")
        if p1c != "identical":
            failures.append("03.1: p1/same.txt content changed unexpectedly")
        if p2c != "identical":
            failures.append("03.1: p2/same.txt content changed unexpectedly")
        if _in_bak(p1, "same.txt") or _in_bak(p2, "same.txt"):
            failures.append("03.1: unexpected BAK entry — a copy was enqueued when peers agreed")

        # ── 03.2: Newer mod_time wins ────────────────────────────────────────────
        p1, p2 = _peers("t02/p1", "t02/p2", base=TMP)
        (p1 / "ver.txt").write_text("content-v1", encoding="utf-8")
        (p2 / "ver.txt").write_text("content-v1", encoding="utf-8")
        _utime(p1 / "ver.txt", now - 10)
        _utime(p2 / "ver.txt", now - 10)
        _run("+" + _url(p1), _url(p2))           # establish snapshots
        (p1 / "ver.txt").write_text("p1-updated", encoding="utf-8")
        (p2 / "ver.txt").write_text("p2-newer", encoding="utf-8")
        _utime(p1 / "ver.txt", now + 10)
        _utime(p2 / "ver.txt", now + 30)          # p2 is 20s newer than p1
        _run(_url(p1), _url(p2))
        got = (p1 / "ver.txt").read_text(encoding="utf-8") if (p1 / "ver.txt").exists() else None
        print(f"[03.2] p1/ver.txt (p2 newer wins): {got!r}")
        if got != "p2-newer":
            failures.append(f"03.2: expected 'p2-newer' on p1, got {got!r}")

        # ── 03.3: New file on one peer → copied to peers that lack it ───────────
        p1, p2 = _peers("t03/p1", "t03/p2", base=TMP)
        _run("+" + _url(p1), _url(p2))            # establish empty snapshots
        (p1 / "brand-new.txt").write_text("fresh-data", encoding="utf-8")
        _utime(p1 / "brand-new.txt", now)
        _run(_url(p1), _url(p2))
        got = (p2 / "brand-new.txt").read_text(encoding="utf-8") if (p2 / "brand-new.txt").exists() else None
        print(f"[03.3] p2/brand-new.txt (new file propagated): {got!r}")
        if got != "fresh-data":
            failures.append(f"03.3: expected 'fresh-data' on p2, got {got!r}")

        # ── 03.4: Deletion estimate > mod_time + 5s → file displaced everywhere ─
        # Setup: file with old mod_time (now-60). After first sync last_seen ≈ now.
        # Setting deleted_time = last_seen gives estimate ≈ now >> (now-60)+5s → deletion wins.
        p1, p2 = _peers("t04/p1", "t04/p2", base=TMP)
        (p1 / "del04.txt").write_text("will-be-displaced", encoding="utf-8")
        (p2 / "del04.txt").write_text("will-be-displaced", encoding="utf-8")
        _utime(p1 / "del04.txt", now - 60)
        _utime(p2 / "del04.txt", now - 60)
        _run("+" + _url(p1), _url(p2))
        (p2 / "del04.txt").unlink()
        p2_ls = _get_last_seen(_snap_db(p2), "del04.txt") or _snap_ts(now)
        _set_deleted(_snap_db(p2), "del04.txt", p2_ls)   # tombstone with estimate ≈ now
        _run(_url(p1), _url(p2))
        p1_has = (p1 / "del04.txt").exists()
        p1_bak = _in_bak(p1, "del04.txt")
        print(f"[03.4] p1/del04.txt present={p1_has} in-BAK={p1_bak}")
        if p1_has:
            failures.append("03.4: p1/del04.txt still present (deletion should have won)")
        if not p1_bak:
            failures.append("03.4: p1/del04.txt not in BAK/ after deletion-wins displacement")

        # ── 03.14: Deletion estimate ≤ mod_time + 5s → existing file wins ───────
        # The tombstone is 3 seconds after the live file's mod_time, inside tolerance.
        p1, p2 = _peers("t14/p1", "t14/p2", base=TMP)
        t14_file = now + 200
        (p1 / "del14.txt").write_text("survives-deletion", encoding="utf-8")
        (p2 / "del14.txt").write_text("survives-deletion", encoding="utf-8")
        _utime(p1 / "del14.txt", t14_file)
        _utime(p2 / "del14.txt", t14_file)
        _run("+" + _url(p1), _url(p2))
        (p2 / "del14.txt").unlink()
        _set_deleted(_snap_db(p2), "del14.txt", _snap_ts(t14_file + 3))
        _run(_url(p1), _url(p2))
        got = (p2 / "del14.txt").read_text(encoding="utf-8") if (p2 / "del14.txt").exists() else None
        p1_bak = _in_bak(p1, "del14.txt")
        print(f"[03.14] p2/del14.txt (file wins over deletion): {got!r}, p1 in BAK={p1_bak}")
        if got != "survives-deletion":
            failures.append(f"03.14: expected p2 to receive the file back, got {got!r}")
        if p1_bak:
            failures.append("03.14: p1 file incorrectly displaced (file should have won)")

        # ── 03.5: Absent-unconfirmed, last_seen ≤ max_mod_time+5s → re-copy ─────
        # p2's last_seen is 3 seconds after p1's mod_time, inside tolerance.
        p1, p2 = _peers("t05/p1", "t05/p2", base=TMP)
        (p1 / "recp.txt").write_text("initial", encoding="utf-8")
        (p2 / "recp.txt").write_text("initial", encoding="utf-8")
        _utime(p1 / "recp.txt", now - 10)
        _utime(p2 / "recp.txt", now - 10)
        _run("+" + _url(p1), _url(p2))
        (p1 / "recp.txt").write_text("updated-content", encoding="utf-8")
        _utime(p1 / "recp.txt", now + 100)
        (p2 / "recp.txt").unlink()
        _set_last_seen(_snap_db(p2), "recp.txt", _snap_ts(now + 103))
        _run(_url(p1), _url(p2))
        got = (p2 / "recp.txt").read_text(encoding="utf-8") if (p2 / "recp.txt").exists() else None
        print(f"[03.5] p2/recp.txt (absent-unconfirmed, re-copy): {got!r}")
        if got != "updated-content":
            failures.append(f"03.5: expected re-copy 'updated-content' on p2, got {got!r}")

        p1, p2 = _peers("t05-null/p1", "t05-null/p2", base=TMP)
        (p1 / "null-seen.txt").write_text("source-with-null-last-seen", encoding="utf-8")
        (p2 / "null-seen.txt").write_text("old-target", encoding="utf-8")
        _utime(p1 / "null-seen.txt", now + 120)
        _utime(p2 / "null-seen.txt", now + 120)
        _run("+" + _url(p1), _url(p2))
        (p2 / "null-seen.txt").unlink()
        _set_last_seen_null(_snap_db(p2), "null-seen.txt")
        _run(_url(p1), _url(p2))
        got = (p2 / "null-seen.txt").read_text(encoding="utf-8") if (p2 / "null-seen.txt").exists() else None
        print(f"[03.5] p2/null-seen.txt (last_seen NULL, re-copy): {got!r}")
        if got != "source-with-null-last-seen":
            failures.append(f"03.5: expected re-copy when last_seen is NULL, got {got!r}")

        # ── 03.18: Absent-unconfirmed, last_seen > max_mod_time+5s → displace ───
        # File has old mod_time (now-60). last_seen ≈ now >> (now-60)+5s → deletion via 4b.
        p1, p2 = _peers("t18/p1", "t18/p2", base=TMP)
        (p1 / "disp.txt").write_text("will-be-displaced", encoding="utf-8")
        (p2 / "disp.txt").write_text("will-be-displaced", encoding="utf-8")
        _utime(p1 / "disp.txt", now - 60)
        _utime(p2 / "disp.txt", now - 60)
        _run("+" + _url(p1), _url(p2))           # last_seen ≈ now >> (now-60)+5s
        (p2 / "disp.txt").unlink()               # absent-unconfirmed; last_seen > max_mod_time+5s
        _run(_url(p1), _url(p2))
        p1_has = (p1 / "disp.txt").exists()
        p1_bak = _in_bak(p1, "disp.txt")
        print(f"[03.18] p1/disp.txt present={p1_has} in-BAK={p1_bak}")
        if p1_has:
            failures.append("03.18: p1/disp.txt still present (absent-unconfirmed deletion should propagate)")
        if not p1_bak:
            failures.append("03.18: p1/disp.txt not in BAK/ after absent-unconfirmed displacement")

        # ── 03.6: Same mod_time, different sizes → larger file wins ─────────────
        p1, p2 = _peers("t06/p1", "t06/p2", base=TMP)
        _run("+" + _url(p1), _url(p2))            # establish empty snapshots
        (p1 / "size.txt").write_bytes(b"s")
        (p2 / "size.txt").write_bytes(b"larger data")
        _utime(p1 / "size.txt", now)
        _utime(p2 / "size.txt", now)              # identical mod_time
        _run(_url(p1), _url(p2))
        p1_sz = len((p1 / "size.txt").read_bytes()) if (p1 / "size.txt").exists() else -1
        p2_sz = len((p2 / "size.txt").read_bytes()) if (p2 / "size.txt").exists() else -1
        print(f"[03.6] p1 size={p1_sz} p2 size={p2_sz} (larger=11 should win)")
        if p1_sz != 11:
            failures.append(f"03.6: p1 should have 11-byte file (larger wins), got {p1_sz}")
        if p2_sz != 11:
            failures.append(f"03.6: p2 should have 11-byte file, got {p2_sz}")

        # ── 03.7: ±5s tolerance — peers within window treated as tied ────────────
        # p1 older (T0) but larger; p2 slightly newer (T0+3s, within 5s) but smaller.
        # Without tolerance: p2 wins (newer). With tolerance: tied → larger (p1) wins.
        p1, p2 = _peers("t07/p1", "t07/p2", base=TMP)
        _run("+" + _url(p1), _url(p2))
        (p1 / "tol.txt").write_bytes(b"larger content!!")   # 16 bytes
        (p2 / "tol.txt").write_bytes(b"s")                   # 1 byte
        _utime(p1 / "tol.txt", now)
        _utime(p2 / "tol.txt", now + 3)            # 3s newer — within 5s tolerance → tied
        _run(_url(p1), _url(p2))
        p1_sz = len((p1 / "tol.txt").read_bytes()) if (p1 / "tol.txt").exists() else -1
        p2_sz = len((p2 / "tol.txt").read_bytes()) if (p2 / "tol.txt").exists() else -1
        print(f"[03.7] p1 size={p1_sz} p2 size={p2_sz} (tolerance: tied, larger=16 wins)")
        if p1_sz != 16:
            failures.append(f"03.7: p1 should be 16 bytes (within-5s tied, larger wins), got {p1_sz}")
        if p2_sz != 16:
            failures.append(f"03.7: p2 should be 16 bytes (got larger file), got {p2_sz}")

        # ── 03.8: No contributing peer has/had entry → no copy enqueued ──────────
        # Subordinate peer has a file that contributing peer has never seen.
        p_contrib, p_sub = _peers("t08/contrib", "t08/sub", base=TMP)
        (p_contrib / "anchor.txt").write_text("anchor", encoding="utf-8")
        (p_sub / "anchor.txt").write_text("anchor", encoding="utf-8")
        _run("+" + _url(p_contrib), _url(p_sub))  # establish snapshots
        (p_sub / "sub-only.txt").write_text("subordinate-data", encoding="utf-8")
        _run(_url(p_contrib), "-" + _url(p_sub))
        contrib_has = (p_contrib / "sub-only.txt").exists()
        sub_bak = _in_bak(p_sub, "sub-only.txt")
        print(f"[03.8] contrib has sub-only={contrib_has}, sub in BAK={sub_bak}")
        if contrib_has:
            failures.append("03.8: contributing peer received file that no contributing peer had/has")
        if not sub_bak:
            failures.append("03.8: subordinate-only file not displaced from sub peer")

        # ── 03.85: Multiple deletions → most recent estimate used ─────────────────
        # T_file = now+100 (future). p2 tombstone = T_file+2s (≤ T_file+5s → file would win).
        # p3 tombstone = T_file+10s (> T_file+5s → deletion wins). Most recent (p3) used → delete.
        p1, p2, p3 = _peers("t85/p1", "t85/p2", "t85/p3", base=TMP)
        T_file = now + 100
        for p in (p1, p2, p3):
            (p / "multi-del.txt").write_text("multi-delete-test", encoding="utf-8")
            _utime(p / "multi-del.txt", T_file)
        _run("+" + _url(p1), _url(p2), _url(p3))
        (p2 / "multi-del.txt").unlink()
        (p3 / "multi-del.txt").unlink()
        _set_deleted(_snap_db(p2), "multi-del.txt", _snap_ts(T_file + 2))   # within 5s → alone, file wins
        _set_deleted(_snap_db(p3), "multi-del.txt", _snap_ts(T_file + 10))  # >5s → alone, deletion wins
        _run(_url(p1), _url(p2), _url(p3))
        p1_has = (p1 / "multi-del.txt").exists()
        p1_bak = _in_bak(p1, "multi-del.txt")
        print(f"[03.85] p1/multi-del present={p1_has} in-BAK={p1_bak} (most-recent deletion wins)")
        if p1_has:
            failures.append("03.85: p1 file still present (most-recent deletion estimate should have won)")
        if not p1_bak:
            failures.append("03.85: p1 file not in BAK/ (deletion should have propagated from most-recent estimate)")

        # ── 03.91+03.19: Resurrection classified as modified; deleted_time cleared ─
        p1, p2 = _peers("t91/p1", "t91/p2", base=TMP)
        t91_original = now
        t91_resurrected = now + 40
        (p1 / "resur.txt").write_text("original", encoding="utf-8")
        (p2 / "resur.txt").write_text("original", encoding="utf-8")
        _utime(p1 / "resur.txt", now)
        _utime(p2 / "resur.txt", now)
        _run("+" + _url(p1), _url(p2))
        # Simulate resurrection: both peers have tombstones; p1 now has a newer live file.
        (p2 / "resur.txt").unlink()
        _set_deleted(_snap_db(p1), "resur.txt", _snap_ts(t91_original + 10))
        _set_deleted(_snap_db(p2), "resur.txt", _snap_ts(t91_original + 10))
        (p1 / "resur.txt").write_text("alive-again", encoding="utf-8")
        _utime(p1 / "resur.txt", t91_resurrected)
        _run(_url(p1), _url(p2))
        p2_content = (p2 / "resur.txt").read_text(encoding="utf-8") if (p2 / "resur.txt").exists() else None
        print(f"[03.91] p2/resur.txt after resurrection sync: {p2_content!r}")
        if p2_content != "alive-again":
            failures.append(f"03.91: resurrection not classified as modified; p2 got {p2_content!r}")
        row = _get_snap_row(_snap_db(p1), "resur.txt")
        p1_del = row[0] if row else "NO_ROW"
        print(f"[03.19] p1 snapshot deleted_time after resurrection: {p1_del!r}")
        if row is None:
            failures.append("03.19: p1 snapshot row missing after resurrection sync")
        elif p1_del is not None:
            failures.append(f"03.19: p1 snapshot deleted_time not cleared after resurrection, got {p1_del!r}")

        # ── 03.92: Destination already has winning file → no file copy performed ──
        # p1 wins (T0, 16B). p2 matches within tolerance (T0-2s, 16B) → no copy to p2.
        # p3 is old (T0-20s, 4B) → receives a copy.  Verify p2 not displaced (no copy happened).
        p1, p2, p3 = _peers("t92/p1", "t92/p2", "t92/p3", base=TMP)
        _run("+" + _url(p1), _url(p2), _url(p3))   # establish empty snapshots
        T0 = now
        (p1 / "win.txt").write_bytes(b"matching content")   # 16B, newest
        (p2 / "win.txt").write_bytes(b"matching content")   # 16B, T0-2s (within 5s → already matches)
        (p3 / "win.txt").write_bytes(b"old!")               # 4B, T0-20s → needs copy
        _utime(p1 / "win.txt", T0)
        _utime(p2 / "win.txt", T0 - 2)
        _utime(p3 / "win.txt", T0 - 20)
        _run(_url(p1), _url(p2), _url(p3))
        p3_content = (p3 / "win.txt").read_text(encoding="utf-8") if (p3 / "win.txt").exists() else None
        p2_bak = _in_bak(p2, "win.txt")
        p2_row = _get_snap_row(_snap_db(p2), "win.txt")
        print(f"[03.92] p3 got winning content={p3_content!r}, p2 displaced={p2_bak}, p2 snapshot={p2_row!r}")
        if p3_content != "matching content":
            failures.append(f"03.92: p3 should have received winning file, got {p3_content!r}")
        if p2_bak:
            failures.append("03.92: p2 was displaced (copy performed) but p2 already matched the winner")
        if p2_row is None or p2_row[0] is not None:
            failures.append(f"03.92: p2 matching winner should have a live snapshot row, got {p2_row!r}")

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
