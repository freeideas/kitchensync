#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""TMP, BAK, and tombstone retention (04.1, 04.2, 04.3, 04.4)."""

from __future__ import annotations

import datetime, os, shutil, sqlite3, subprocess, sys
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

TMP = Path(PROJECT) / "tmp" / "testks" / "04_retention"


def _run(*peer_args, timeout=60):
    return subprocess.run(
        [str(UV), "run", "--script", str(BUILD_PY), "invoke-cli", PROJECT, *peer_args],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        text=True, encoding="utf-8",
        timeout=timeout,
    )


def _old_ts(days: int) -> str:
    """Return a database/directory timestamp string DAYS days in the past."""
    t = datetime.datetime.now(datetime.UTC) - datetime.timedelta(days=days)
    return t.strftime("%Y-%m-%d_%H-%M-%S_000000Z")


def _checkpoint(conn: sqlite3.Connection) -> None:
    conn.execute("PRAGMA wal_checkpoint(TRUNCATE)")


def main() -> int:
    if TMP.exists():
        shutil.rmtree(TMP)
    TMP.mkdir(parents=True)

    failures = []

    try:
        # --- 04.1: Tombstone rows (deleted_time IS NOT NULL, old) are purged at startup ---
        p1 = TMP / "t041" / "peer1"
        p2 = TMP / "t041" / "peer2"
        p1.mkdir(parents=True)
        p2.mkdir(parents=True)
        (p1 / "seed.txt").write_text("seed", encoding="utf-8")
        r = _run("+" + p1.resolve().as_uri(), p2.resolve().as_uri())
        if r.returncode != 0:
            failures.append(f"04.1: initial sync failed (exit {r.returncode})")
        else:
            db_path = p1 / ".kitchensync" / "snapshot.db"
            if not db_path.exists():
                failures.append("04.1: snapshot.db not found after initial sync")
            else:
                old = _old_ts(200)
                recent = _old_ts(1)
                with sqlite3.connect(str(db_path)) as conn:
                    conn.execute(
                        "INSERT INTO snapshot(id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time) "
                        "VALUES (?, ?, ?, ?, ?, ?, ?)",
                        ("zombie041aaa", "rootrootroo", "zombie_041.txt", recent, 100, recent, old),
                    )
                    conn.execute(
                        "INSERT INTO snapshot(id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time) "
                        "VALUES (?, ?, ?, ?, ?, ?, ?)",
                        ("fresh041aaaa", "rootrootroo", "fresh_tombstone_041.txt", old, 100, old, recent),
                    )
                    conn.commit()
                    _checkpoint(conn)
                with sqlite3.connect(str(db_path)) as conn:
                    old_inserted = conn.execute(
                        "SELECT id FROM snapshot WHERE id='zombie041aaa'"
                    ).fetchone()
                    recent_inserted = conn.execute(
                        "SELECT id FROM snapshot WHERE id='fresh041aaaa'"
                    ).fetchone()
                if not old_inserted or not recent_inserted:
                    failures.append("04.1: could not insert fake tombstone rows")
                else:
                    r = _run("+" + p1.resolve().as_uri(), p2.resolve().as_uri(), "--td", "10")
                    if r.returncode != 0:
                        failures.append(f"04.1: retention sync failed (exit {r.returncode})")
                    with sqlite3.connect(str(db_path)) as conn:
                        old_row = conn.execute(
                            "SELECT id FROM snapshot WHERE id='zombie041aaa'"
                        ).fetchone()
                        recent_row = conn.execute(
                            "SELECT deleted_time FROM snapshot WHERE id='fresh041aaaa'"
                        ).fetchone()
                    print(f"[04.1] old tombstone row: {old_row}, recent tombstone row: {recent_row}")
                    if old_row is not None:
                        failures.append(
                            "04.1: old tombstone row (deleted_time 200d > --td 10d) was not purged at startup"
                        )
                    if recent_row is None:
                        failures.append(
                            "04.1: recent tombstone row (deleted_time 1d <= --td 10d) was unexpectedly purged"
                        )
                    elif recent_row[0] != recent:
                        failures.append(
                            f"04.1: recent tombstone deleted_time changed from {recent!r} to {recent_row[0]!r}"
                        )
                    if old_row is None and recent_row is not None and recent_row[0] == recent:
                        print("[04.1] PASS")

        # --- 04.2: Stale rows (deleted_time IS NULL, last_seen too old or NULL) purged at startup ---
        p1 = TMP / "t042" / "peer1"
        p2 = TMP / "t042" / "peer2"
        p1.mkdir(parents=True)
        p2.mkdir(parents=True)
        (p1 / "seed.txt").write_text("seed", encoding="utf-8")
        r = _run("+" + p1.resolve().as_uri(), p2.resolve().as_uri())
        if r.returncode != 0:
            failures.append(f"04.2: initial sync failed (exit {r.returncode})")
        else:
            db_path = p1 / ".kitchensync" / "snapshot.db"
            if not db_path.exists():
                failures.append("04.2: snapshot.db not found after initial sync")
            else:
                old = _old_ts(200)
                recent = _old_ts(1)
                with sqlite3.connect(str(db_path)) as conn:
                    # Stale row: deleted_time IS NULL, last_seen 200 days ago
                    conn.execute(
                        "INSERT INTO snapshot(id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time) "
                        "VALUES (?, ?, ?, ?, ?, ?, ?)",
                        ("stale042aaaa", "rootrootroo", "stale_042.txt", recent, 100, old, None),
                    )
                    # Null last_seen row: deleted_time IS NULL, last_seen IS NULL
                    conn.execute(
                        "INSERT INTO snapshot(id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time) "
                        "VALUES (?, ?, ?, ?, ?, ?, ?)",
                        ("nullls042aaa", "rootrootroo", "nullls_042.txt", recent, 100, None, None),
                    )
                    # Recent non-tombstone row: inside --td, so startup purge must retain it.
                    conn.execute(
                        "INSERT INTO snapshot(id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time) "
                        "VALUES (?, ?, ?, ?, ?, ?, ?)",
                        ("fresh042aaaa", "rootrootroo", "fresh_042.txt", old, 100, recent, None),
                    )
                    conn.commit()
                    _checkpoint(conn)
                # Re-sync with --td 10; stale and null-last_seen rows should be purged
                r = _run("+" + p1.resolve().as_uri(), p2.resolve().as_uri(), "--td", "10")
                if r.returncode != 0:
                    failures.append(f"04.2: retention sync failed (exit {r.returncode})")
                with sqlite3.connect(str(db_path)) as conn:
                    r1 = conn.execute(
                        "SELECT id FROM snapshot WHERE id='stale042aaaa'"
                    ).fetchone()
                    r2 = conn.execute(
                        "SELECT id FROM snapshot WHERE id='nullls042aaa'"
                    ).fetchone()
                    r3 = conn.execute(
                        "SELECT last_seen, deleted_time FROM snapshot WHERE id='fresh042aaaa'"
                    ).fetchone()
                print(f"[04.2] stale last_seen row: {r1}, null last_seen row: {r2}, recent row: {r3}")
                if r1 is not None:
                    failures.append(
                        "04.2: stale row (last_seen 200d > --td 10d, deleted_time IS NULL) was not purged"
                    )
                if r2 is not None:
                    failures.append(
                        "04.2: null last_seen row (deleted_time IS NULL, last_seen IS NULL) was not purged"
                    )
                if r3 is None:
                    failures.append(
                        "04.2: recent row (last_seen 1d <= --td 10d, deleted_time IS NULL) was unexpectedly purged"
                    )
                elif r3 != (recent, None):
                    failures.append(
                        f"04.2: recent row changed from last_seen={recent!r}, deleted_time=None to {r3!r}"
                    )
                if r1 is None and r2 is None and r3 == (recent, None):
                    print("[04.2] PASS")

        # --- 04.3: BAK/<old_timestamp>/ older than --bd days removed during traversal ---
        p1 = TMP / "t043" / "peer1"
        p2 = TMP / "t043" / "peer2"
        p1.mkdir(parents=True)
        p2.mkdir(parents=True)
        (p1 / "seed.txt").write_text("seed", encoding="utf-8")
        (p1 / "nested").mkdir()
        (p1 / "nested" / "seed.txt").write_text("nested seed", encoding="utf-8")
        r = _run("+" + p1.resolve().as_uri(), p2.resolve().as_uri())
        if r.returncode != 0:
            failures.append(f"04.3: initial sync failed (exit {r.returncode})")
        else:
            bak_root = p1 / ".kitchensync" / "BAK"
            old_bak_dir = bak_root / _old_ts(200)
            old_bak_dir.mkdir(parents=True)
            (old_bak_dir / "old_backup.txt").write_text("stale", encoding="utf-8")
            recent_bak_dir = bak_root / _old_ts(1)
            recent_bak_dir.mkdir(parents=True)
            (recent_bak_dir / "recent_backup.txt").write_text("recent", encoding="utf-8")
            nested_bak_root = p1 / "nested" / ".kitchensync" / "BAK"
            nested_old_bak_dir = nested_bak_root / _old_ts(200)
            nested_old_bak_dir.mkdir(parents=True)
            (nested_old_bak_dir / "old_nested_backup.txt").write_text("stale", encoding="utf-8")
            nested_recent_bak_dir = nested_bak_root / _old_ts(1)
            nested_recent_bak_dir.mkdir(parents=True)
            (nested_recent_bak_dir / "recent_nested_backup.txt").write_text("recent", encoding="utf-8")
            r = _run("+" + p1.resolve().as_uri(), p2.resolve().as_uri(), "--bd", "10")
            if r.returncode != 0:
                failures.append(f"04.3: retention sync failed (exit {r.returncode})")
            old_exists = old_bak_dir.exists()
            recent_exists = recent_bak_dir.exists()
            nested_old_exists = nested_old_bak_dir.exists()
            nested_recent_exists = nested_recent_bak_dir.exists()
            print(
                f"[04.3] root old={old_exists}, root recent={recent_exists}, "
                f"nested old={nested_old_exists}, nested recent={nested_recent_exists}"
            )
            if old_exists:
                failures.append(
                    "04.3: root BAK/<200d-old-timestamp>/ was not removed during traversal with --bd 10"
                )
            if not recent_exists:
                failures.append(
                    "04.3: root BAK/<1d-old-timestamp>/ was unexpectedly removed with --bd 10"
                )
            if nested_old_exists:
                failures.append(
                    "04.3: nested BAK/<200d-old-timestamp>/ was not removed during traversal with --bd 10"
                )
            if not nested_recent_exists:
                failures.append(
                    "04.3: nested BAK/<1d-old-timestamp>/ was unexpectedly removed with --bd 10"
                )
            if not old_exists and recent_exists and not nested_old_exists and nested_recent_exists:
                print("[04.3] PASS")

        # --- 04.4: TMP/<old_timestamp>/ older than --xd days removed during traversal ---
        p1 = TMP / "t044" / "peer1"
        p2 = TMP / "t044" / "peer2"
        p1.mkdir(parents=True)
        p2.mkdir(parents=True)
        (p1 / "seed.txt").write_text("seed", encoding="utf-8")
        (p1 / "nested").mkdir()
        (p1 / "nested" / "seed.txt").write_text("nested seed", encoding="utf-8")
        r = _run("+" + p1.resolve().as_uri(), p2.resolve().as_uri())
        if r.returncode != 0:
            failures.append(f"04.4: initial sync failed (exit {r.returncode})")
        else:
            tmp_root = p1 / ".kitchensync" / "TMP"
            old_tmp_dir = tmp_root / _old_ts(10)
            old_tmp_dir.mkdir(parents=True)
            (old_tmp_dir / "stale_transfer.txt").write_text("stale staging", encoding="utf-8")
            recent_tmp_dir = tmp_root / _old_ts(1)
            recent_tmp_dir.mkdir(parents=True)
            (recent_tmp_dir / "ongoing_transfer.txt").write_text("in progress", encoding="utf-8")
            nested_tmp_root = p1 / "nested" / ".kitchensync" / "TMP"
            nested_old_tmp_dir = nested_tmp_root / _old_ts(10)
            nested_old_tmp_dir.mkdir(parents=True)
            (nested_old_tmp_dir / "stale_nested_transfer.txt").write_text("stale staging", encoding="utf-8")
            nested_recent_tmp_dir = nested_tmp_root / _old_ts(1)
            nested_recent_tmp_dir.mkdir(parents=True)
            (nested_recent_tmp_dir / "ongoing_nested_transfer.txt").write_text("in progress", encoding="utf-8")
            r = _run("+" + p1.resolve().as_uri(), p2.resolve().as_uri(), "--xd", "5")
            if r.returncode != 0:
                failures.append(f"04.4: retention sync failed (exit {r.returncode})")
            old_exists = old_tmp_dir.exists()
            recent_exists = recent_tmp_dir.exists()
            nested_old_exists = nested_old_tmp_dir.exists()
            nested_recent_exists = nested_recent_tmp_dir.exists()
            print(
                f"[04.4] root old={old_exists}, root recent={recent_exists}, "
                f"nested old={nested_old_exists}, nested recent={nested_recent_exists}"
            )
            if old_exists:
                failures.append(
                    "04.4: root TMP/<10d-old-timestamp>/ was not removed during traversal with --xd 5"
                )
            if not recent_exists:
                failures.append(
                    "04.4: root TMP/<1d-old-timestamp>/ was unexpectedly removed with --xd 5"
                )
            if nested_old_exists:
                failures.append(
                    "04.4: nested TMP/<10d-old-timestamp>/ was not removed during traversal with --xd 5"
                )
            if not nested_recent_exists:
                failures.append(
                    "04.4: nested TMP/<1d-old-timestamp>/ was unexpectedly removed with --xd 5"
                )
            if not old_exists and recent_exists and not nested_old_exists and nested_recent_exists:
                print("[04.4] PASS")

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
