# /// script
# requires-python = ">=3.9"
# dependencies = []
# ///
import sys
sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

import subprocess
import tempfile
import shutil
import sqlite3
from pathlib import Path

EXE = Path("/home/ace/Desktop/prjx/kitchensync/released/kitchensync.exe")


def check_schema(db_path: Path, label: str, failures: list) -> None:
    with sqlite3.connect(str(db_path)) as conn:
        cur = conn.cursor()

        # 013.1 -- exactly one table
        cur.execute("SELECT name FROM sqlite_master WHERE type='table'")
        tables = [r[0] for r in cur.fetchall()]
        if len(tables) != 1:
            failures.append(
                f"013.1 [{label}]: expected 1 table, got {len(tables)}: {tables}"
            )

        # 013.2 -- table named 'snapshot'
        if "snapshot" not in tables:
            failures.append(
                f"013.2 [{label}]: no table named 'snapshot'; found: {tables}"
            )

        # 013.3 -- no views
        cur.execute("SELECT count(*) FROM sqlite_master WHERE type='view'")
        n_views = cur.fetchone()[0]
        if n_views != 0:
            failures.append(f"013.3 [{label}]: database contains {n_views} view(s)")

        # Column checks: PRAGMA table_info rows are (cid, name, type, notnull, dflt, pk)
        cur.execute("PRAGMA table_info(snapshot)")
        cols = {
            r[1]: {"type": r[2], "notnull": r[3], "pk": r[5]}
            for r in cur.fetchall()
        }

        def require_col(req: str, name: str, typ: str) -> None:
            if name not in cols:
                failures.append(f"{req} [{label}]: column '{name}' not found")
            elif cols[name]["type"].upper() != typ.upper():
                failures.append(
                    f"{req} [{label}]: '{name}' type='{cols[name]['type']}', expected {typ}"
                )

        def require_notnull(req: str, name: str) -> None:
            if name in cols and cols[name]["notnull"] != 1:
                failures.append(f"{req} [{label}]: '{name}' should be NOT NULL")

        def require_nullable(req: str, name: str) -> None:
            if name in cols and cols[name]["notnull"] != 0:
                failures.append(f"{req} [{label}]: '{name}' should allow NULL")

        # 013.4 -- id is TEXT
        require_col("013.4", "id", "TEXT")
        # 013.5 -- id is the primary key
        if "id" in cols:
            if cols["id"]["pk"] == 0:
                failures.append(f"013.5 [{label}]: 'id' is not marked as primary key")
            extra_pk = [n for n, c in cols.items() if n != "id" and c["pk"] != 0]
            if extra_pk:
                failures.append(
                    f"013.5 [{label}]: unexpected additional PK columns: {extra_pk}"
                )
        # 013.6 -- parent_id is TEXT
        require_col("013.6", "parent_id", "TEXT")
        # 013.7 -- basename is TEXT
        require_col("013.7", "basename", "TEXT")
        # 013.8 -- basename is not null
        require_notnull("013.8", "basename")
        # 013.9 -- mod_time is TEXT
        require_col("013.9", "mod_time", "TEXT")
        # 013.10 -- mod_time is not null
        require_notnull("013.10", "mod_time")
        # 013.11 -- byte_size is INTEGER
        require_col("013.11", "byte_size", "INTEGER")
        # 013.12 -- byte_size is not null
        require_notnull("013.12", "byte_size")
        # 013.15 -- last_seen is TEXT, allows NULL
        require_col("013.15", "last_seen", "TEXT")
        require_nullable("013.15", "last_seen")
        # 013.16 -- deleted_time is TEXT, allows NULL
        require_col("013.16", "deleted_time", "TEXT")
        require_nullable("013.16", "deleted_time")

        # 013.17/013.18/013.19 -- indexes on parent_id, last_seen, deleted_time
        cur.execute("PRAGMA index_list(snapshot)")
        indexed_cols: set = set()
        for idx in cur.fetchall():
            # idx columns: seq, name, unique, origin, partial
            cur.execute(f"PRAGMA index_info({idx[1]})")
            for info in cur.fetchall():
                # info columns: seqno, cid, name
                indexed_cols.add(info[2])

        for req, col in [
            ("013.17", "parent_id"),
            ("013.18", "last_seen"),
            ("013.19", "deleted_time"),
        ]:
            if col not in indexed_cols:
                failures.append(f"{req} [{label}]: no index covers column '{col}'")

        # 013.20 -- at most one row per tracked path
        cur.execute(
            "SELECT id, count(*) n FROM snapshot GROUP BY id HAVING n > 1"
        )
        dupes = cur.fetchall()
        if dupes:
            failures.append(
                f"013.20 [{label}]: duplicate snapshot id(s): {[d[0] for d in dupes]}"
            )


def check_row_data(db_path: Path, file_byte_size: int, failures: list) -> None:
    with sqlite3.connect(str(db_path)) as conn:
        cur = conn.cursor()

        # 013.13 -- file row stores actual byte count
        cur.execute("SELECT byte_size FROM snapshot WHERE basename = 'hello.txt'")
        row = cur.fetchone()
        if row is None:
            failures.append("013.13: 'hello.txt' row not found in snapshot")
        elif row[0] != file_byte_size:
            failures.append(
                f"013.13: 'hello.txt' byte_size={row[0]}, expected {file_byte_size}"
            )

        # 013.14 -- directory row stores -1
        cur.execute("SELECT byte_size FROM snapshot WHERE basename = 'subdir'")
        row = cur.fetchone()
        if row is None:
            failures.append("013.14: 'subdir' row not found in snapshot")
        elif row[0] != -1:
            failures.append(f"013.14: 'subdir' byte_size={row[0]}, expected -1")


def main() -> None:
    failures: list = []

    tmpdir = Path(tempfile.mkdtemp(prefix="ks_013_"))
    try:
        peer_a = tmpdir / "peer_a"
        peer_b = tmpdir / "peer_b"
        peer_a.mkdir()
        peer_b.mkdir()

        # Populate peer_a: one file and one subdirectory with a nested file
        hello_content = b"hello world"
        (peer_a / "hello.txt").write_bytes(hello_content)
        subdir = peer_a / "subdir"
        subdir.mkdir()
        (subdir / "notes.txt").write_bytes(b"some notes here")

        # Run kitchensync -- first run requires a canon peer (+)
        result = subprocess.run(
            [str(EXE), f"+{peer_a}", str(peer_b)],
            capture_output=True,
            text=True,
            timeout=60,
        )
        if result.returncode != 0:
            failures.append(
                f"kitchensync exited {result.returncode}; "
                f"stdout={result.stdout[:500]!r}"
            )

        # Verify schema for both peers' snapshot databases
        for label, peer_dir in [("peer_a", peer_a), ("peer_b", peer_b)]:
            db = peer_dir / ".kitchensync" / "snapshot.db"
            if not db.exists():
                failures.append(f"snapshot.db not found for {label}: {db}")
                continue
            check_schema(db, label, failures)

        # Verify row-level data from peer_a snapshot (file size, directory sentinel)
        db_a = peer_a / ".kitchensync" / "snapshot.db"
        if db_a.exists():
            check_row_data(db_a, len(hello_content), failures)

    finally:
        shutil.rmtree(str(tmpdir), ignore_errors=True)

    if failures:
        print(f"\n{len(failures)} check(s) FAILED:")
        for msg in failures:
            print(f"  FAIL: {msg}")
        sys.exit(1)

    print("All checks passed.")


main()
