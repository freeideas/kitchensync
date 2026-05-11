#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercises lifecycle requirements 01.1–01.6 for snapshot-db."""

from __future__ import annotations

import json, os, shutil, socket, sqlite3, subprocess, sys, threading, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

TEST_DIR = Path("/tmp/snapshot_db_test_01_lifecycle")

_next_id = 0


def _drain(stream):
    for _ in stream:
        pass


def _launch():
    proc = subprocess.Popen(
        [str(UV), "run", "--script", str(BUILD_PY), "launch-mcp", PROJECT],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        text=True, encoding="utf-8",
    )
    port = None
    deadline = time.time() + 30
    while time.time() < deadline and proc.poll() is None:
        line = proc.stdout.readline()
        if not line:
            continue
        line = line.strip()
        if line.startswith("MCP_PORT="):
            port = int(line.split("=", 1)[1])
            break
    if port is None:
        proc.terminate()
        raise RuntimeError("MCP server did not advertise MCP_PORT")
    threading.Thread(target=_drain, args=(proc.stdout,), daemon=True).start()
    threading.Thread(target=_drain, args=(proc.stderr,), daemon=True).start()
    return proc, port


def _rpc(sock, method, params=None):
    global _next_id
    _next_id += 1
    msg = {"jsonrpc": "2.0", "id": _next_id, "method": method}
    if params is not None:
        msg["params"] = params
    sock.sendall((json.dumps(msg) + "\n").encode("utf-8"))
    buf = b""
    deadline = time.time() + 10
    while time.time() < deadline:
        chunk = sock.recv(8192)
        if not chunk:
            break
        buf += chunk
        if b"\n" in buf:
            break
    line, _, _ = buf.partition(b"\n")
    return json.loads(line.decode("utf-8"))


def call(sock, tool, args):
    return _rpc(sock, "tools/call", {"name": tool, "arguments": args})


def main() -> int:
    # Idempotent: wipe test directory before each run
    if TEST_DIR.exists():
        shutil.rmtree(TEST_DIR)
    TEST_DIR.mkdir(parents=True)

    proc, port = _launch()
    failures = []
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:

            # ── 01.1: open at non-existent path creates the file ─────────────
            db1 = str(TEST_DIR / "new.db")
            r = call(s, "db-open", {"path": db1})
            created = Path(db1).exists()
            handle1 = (r.get("result") or {}).get("handle")
            print(f"[01.1] open at new path creates file: {created}")
            if not created:
                failures.append("01.1: file not created at given path")
            if handle1 is None:
                failures.append(f"01.1: db-open returned no handle: {r}")

            # ── 01.2: snapshot table contains documented columns ──────────────
            expected_cols = {"id", "parent_id", "basename", "mod_time",
                             "byte_size", "last_seen", "deleted_time"}
            try:
                with sqlite3.connect(db1) as con:
                    actual_cols = {row[1] for row in con.execute("PRAGMA table_info('snapshot')")}
            except Exception as exc:
                actual_cols = set()
                failures.append(f"01.2: could not inspect schema: {exc}")
            missing = expected_cols - actual_cols
            print(f"[01.2] snapshot table has documented columns (missing={missing or 'none'}): {not missing}")
            if missing:
                failures.append(f"01.2: snapshot table missing columns: {missing}")

            # Also verify an insert through the MCP API works on a fresh db
            ts = "2024-01-01_00-00-00_000000Z"
            upsert_r = call(s, "upsert-confirmed-row", {
                "handle": handle1,
                "path": "rootfile.txt",
                "basename": "rootfile.txt",
                "mod_time": ts,
                "byte_size": 42,
                "last_seen": ts,
            })
            upsert_ok = "error" not in upsert_r
            print(f"[01.2] upsert via documented columns succeeds on fresh db: {upsert_ok}")
            if not upsert_ok:
                failures.append(f"01.2: upsert on fresh db failed: {upsert_r.get('error')}")

            # ── 01.3: reopen existing db reads prior rows and writes new ones ─
            call(s, "db-close", {"handle": handle1})
            r2 = call(s, "db-open", {"path": db1})
            handle2 = (r2.get("result") or {}).get("handle")

            hash_r = call(s, "hash-path", {"path": "rootfile.txt"})
            row_id = (hash_r.get("result") or {}).get("id")
            lookup_r = call(s, "lookup-row", {"handle": handle2, "id": row_id})
            row_found = "error" not in lookup_r and (lookup_r.get("result") or {}).get("row") is not None
            print(f"[01.3] reopened handle reads prior row: {row_found}")
            if not row_found:
                failures.append(f"01.3: prior row not found after reopen: {lookup_r}")

            ts2 = "2024-01-01_00-00-01_000000Z"
            upsert2_r = call(s, "upsert-confirmed-row", {
                "handle": handle2,
                "path": "second.txt",
                "basename": "second.txt",
                "mod_time": ts2,
                "byte_size": 7,
                "last_seen": ts2,
            })
            write_ok = "error" not in upsert2_r
            print(f"[01.3] reopened handle writes new row: {write_ok}")
            if not write_ok:
                failures.append(f"01.3: new row write via reopened handle failed: {upsert2_r.get('error')}")

            call(s, "db-close", {"handle": handle2})

            # ── 01.4: WAL journal mode ────────────────────────────────────────
            with sqlite3.connect(db1) as con:
                mode = con.execute("PRAGMA journal_mode").fetchone()[0]
            wal_ok = mode == "wal"
            print(f"[01.4] journal_mode=wal (got '{mode}'): {wal_ok}")
            if not wal_ok:
                failures.append(f"01.4: expected WAL journal mode, got '{mode}'")

            # ── 01.5: foreign-key enforcement enabled ─────────────────────────
            # The schema defines parent_id as a FK to id in the same table.
            # Inserting a row whose parent directory has no existing row in the
            # snapshot table (parent_id points to a non-existent id) must fail
            # when FK enforcement is on. Root-level entries use the root sentinel
            # which is a pre-existing row; only non-root orphans trigger FK errors.
            db2 = str(TEST_DIR / "fk.db")
            r3 = call(s, "db-open", {"path": db2})
            handle3 = (r3.get("result") or {}).get("handle")
            ts3 = "2024-01-01_00-00-02_000000Z"
            fk_r = call(s, "upsert-confirmed-row", {
                "handle": handle3,
                "path": "ghost_dir/orphan.txt",
                "basename": "orphan.txt",
                "mod_time": ts3,
                "byte_size": 1,
                "last_seen": ts3,
            })
            fk_enforced = "error" in fk_r
            print(f"[01.5] FK enforcement rejects insert with non-existent parent: {fk_enforced}")
            if not fk_enforced:
                failures.append("01.5: FK enforcement not active — orphan-parent insert succeeded")
            call(s, "db-close", {"handle": handle3})

            # ── 01.6: close flushes writes; reopen observes them ─────────────
            db3 = str(TEST_DIR / "flush.db")
            r4 = call(s, "db-open", {"path": db3})
            handle4 = (r4.get("result") or {}).get("handle")
            ts4 = "2024-01-01_00-00-03_000000Z"
            call(s, "upsert-confirmed-row", {
                "handle": handle4,
                "path": "persisted.txt",
                "basename": "persisted.txt",
                "mod_time": ts4,
                "byte_size": 99,
                "last_seen": ts4,
            })
            call(s, "db-close", {"handle": handle4})

            r5 = call(s, "db-open", {"path": db3})
            handle5 = (r5.get("result") or {}).get("handle")
            hash5_r = call(s, "hash-path", {"path": "persisted.txt"})
            id5 = (hash5_r.get("result") or {}).get("id")
            lookup5_r = call(s, "lookup-row", {"handle": handle5, "id": id5})
            flushed = "error" not in lookup5_r and (lookup5_r.get("result") or {}).get("row") is not None
            print(f"[01.6] close flushes writes: row visible after reopen: {flushed}")
            if not flushed:
                failures.append(f"01.6: row not found after close+reopen: {lookup5_r}")
            call(s, "db-close", {"handle": handle5})

    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()

    if failures:
        print("\nFAILURES:")
        for f in failures:
            print(f"  - {f}")
        return 1
    print("\nAll assertions passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
