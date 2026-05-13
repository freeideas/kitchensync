#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Global option defaults take effect when their flags are omitted (01.24)."""

from __future__ import annotations

import datetime
import os
import shutil
import sqlite3
import subprocess
import sys
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

TMP = (Path(PROJECT) / "tmp" / "testks" / "01_cli-grammar").resolve()


def invoke(args: list[str], timeout: int = 60) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(UV), "run", "--script", str(BUILD_PY), "invoke-cli", PROJECT] + args,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        timeout=timeout,
    )


def old_ts(days: int, hours: int = 0) -> str:
    t = datetime.datetime.now(datetime.timezone.utc) - datetime.timedelta(
        days=days, hours=hours
    )
    return t.strftime("%Y-%m-%d_%H-%M-%S_000000Z")


def make_file_peers(tag: str) -> tuple[Path, Path]:
    p1 = TMP / tag / "peer1"
    p2 = TMP / tag / "peer2"
    p1.mkdir(parents=True, exist_ok=True)
    p2.mkdir(parents=True, exist_ok=True)
    (p1 / "seed.txt").write_text("seed", encoding="utf-8")
    return p1, p2


def sync_file_peers(p1: Path, p2: Path) -> subprocess.CompletedProcess[str]:
    return invoke(["+" + p1.as_uri(), p2.as_uri()])


def checkpoint(conn: sqlite3.Connection) -> None:
    conn.execute("PRAGMA wal_checkpoint(TRUNCATE)")


def main() -> int:
    if TMP.exists():
        shutil.rmtree(TMP)
    TMP.mkdir(parents=True)

    failures: list[str] = []

    try:
        # 01.24: -vl defaults to info. Info copy lines must appear when -vl is
        # omitted, while trace-only pool lines must not appear at the default.
        # The spec defines no debug-only messages, so info and debug are not
        # distinguishable through CLI output.
        p1, p2 = make_file_peers("verbosity")
        print("[01.24 -vl] omitted -vl emits info copy output")
        r = sync_file_peers(p1, p2)
        if r.returncode != 0:
            failures.append(
                f"01.24 -vl: sync with omitted -vl failed (exit {r.returncode})\n"
                f"  stdout: {r.stdout!r}\n  stderr: {r.stderr!r}"
            )
        if "C seed.txt" not in r.stdout:
            failures.append(
                f"01.24 -vl: omitted -vl did not emit the info-level copy line\n"
                f"  stdout: {r.stdout!r}"
            )
        if "endpoint=" in r.stdout or "connections=" in r.stdout:
            failures.append(
                f"01.24 -vl: omitted -vl emitted trace-level pool output\n"
                f"  stdout: {r.stdout!r}"
            )

        # 01.24: --xd defaults to 2 days. A TMP directory older than 2 days is
        # stale; one younger than 2 days is retained.
        p1, p2 = make_file_peers("xd")
        r = sync_file_peers(p1, p2)
        if r.returncode != 0:
            failures.append(f"01.24 --xd setup: initial sync failed (exit {r.returncode})")
        else:
            tmp_root = p1 / ".kitchensync" / "TMP"
            stale_tmp = tmp_root / old_ts(2, 1)
            fresh_tmp = tmp_root / old_ts(1, 23)
            stale_tmp.mkdir(parents=True)
            fresh_tmp.mkdir(parents=True)
            (stale_tmp / "stale.txt").write_text("stale", encoding="utf-8")
            (fresh_tmp / "fresh.txt").write_text("fresh", encoding="utf-8")
            r = sync_file_peers(p1, p2)
            print(f"[01.24 --xd] stale exists={stale_tmp.exists()} fresh exists={fresh_tmp.exists()}")
            if r.returncode != 0:
                failures.append(f"01.24 --xd: default-retention sync failed (exit {r.returncode})")
            if stale_tmp.exists():
                failures.append("01.24 --xd: 49-hour-old TMP directory was not removed by default --xd 2")
            if not fresh_tmp.exists():
                failures.append("01.24 --xd: 47-hour-old TMP directory was removed by default --xd 2")

        # 01.24: --bd defaults to 90 days. A BAK directory older than 90 days
        # is stale; one younger than 90 days is retained.
        p1, p2 = make_file_peers("bd")
        r = sync_file_peers(p1, p2)
        if r.returncode != 0:
            failures.append(f"01.24 --bd setup: initial sync failed (exit {r.returncode})")
        else:
            bak_root = p1 / ".kitchensync" / "BAK"
            stale_bak = bak_root / old_ts(90, 1)
            fresh_bak = bak_root / old_ts(89, 23)
            stale_bak.mkdir(parents=True)
            fresh_bak.mkdir(parents=True)
            (stale_bak / "stale.txt").write_text("stale", encoding="utf-8")
            (fresh_bak / "fresh.txt").write_text("fresh", encoding="utf-8")
            r = sync_file_peers(p1, p2)
            print(f"[01.24 --bd] stale exists={stale_bak.exists()} fresh exists={fresh_bak.exists()}")
            if r.returncode != 0:
                failures.append(f"01.24 --bd: default-retention sync failed (exit {r.returncode})")
            if stale_bak.exists():
                failures.append("01.24 --bd: 90-day-1-hour-old BAK directory was not removed by default --bd 90")
            if not fresh_bak.exists():
                failures.append("01.24 --bd: 89-day-23-hour-old BAK directory was removed by default --bd 90")

        # 01.24: --td defaults to 180 days. A tombstone older than 180 days is
        # purged; one younger than 180 days is retained.
        p1, p2 = make_file_peers("td")
        r = sync_file_peers(p1, p2)
        db_path = p1 / ".kitchensync" / "snapshot.db"
        if r.returncode != 0:
            failures.append(f"01.24 --td setup: initial sync failed (exit {r.returncode})")
        elif not db_path.exists():
            failures.append("01.24 --td setup: snapshot.db was not created")
        else:
            recent = old_ts(1)
            with sqlite3.connect(str(db_path)) as conn:
                conn.execute(
                    "INSERT INTO snapshot(id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time) "
                    "VALUES (?, ?, ?, ?, ?, ?, ?)",
                    ("tdstale0001", "rootrootroo", "td-stale.txt", recent, 1, recent, old_ts(180, 1)),
                )
                conn.execute(
                    "INSERT INTO snapshot(id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time) "
                    "VALUES (?, ?, ?, ?, ?, ?, ?)",
                    ("tdfresh0001", "rootrootroo", "td-fresh.txt", recent, 1, recent, old_ts(179, 23)),
                )
                conn.commit()
                checkpoint(conn)
            r = sync_file_peers(p1, p2)
            if r.returncode != 0:
                failures.append(f"01.24 --td: default-retention sync failed (exit {r.returncode})")
            with sqlite3.connect(str(db_path)) as conn:
                stale = conn.execute("SELECT id FROM snapshot WHERE id='tdstale0001'").fetchone()
                fresh = conn.execute("SELECT id FROM snapshot WHERE id='tdfresh0001'").fetchone()
            print(f"[01.24 --td] stale row={stale} fresh row={fresh}")
            if stale is not None:
                failures.append("01.24 --td: 180-day-1-hour-old tombstone was not purged by default --td 180")
            if fresh is None:
                failures.append("01.24 --td: 179-day-23-hour-old tombstone was purged by default --td 180")

        # 01.24: --mc 10 is not reasonably testable here. The default is only
        # observable through successful SFTP pool behavior, and this test must
        # not depend on a configured localhost SFTP account.
        #
        # 01.24: --ct 30 is not reasonably testable here. The exact default is
        # only observable by making an SSH handshake hang and distinguishing a
        # roughly-30-second timeout, which is a slow timing test.
        #
        # 01.24: --ka 30 is not reasonably testable here. The exact default is
        # only observable by watching idle SFTP connection expiry/reuse across
        # the 30-second TTL; the CLI does not expose the configured value.

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
