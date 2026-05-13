#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercises 02_upsert-observed: upsert_observed inserts and updates path records."""

from __future__ import annotations

import json, os, socket, subprocess, sys, threading, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = Path(os.environ.get("AITC_PROJECT", "."))

TMP = PROJECT / "tmp" / "testks" / "02-upsert-observed"
DB_PATH = TMP / "test.db"

TS1 = "2024-01-15_12-00-00_000000Z"
TS2 = "2024-06-20_08-30-00_000000Z"
TS3 = "2024-09-01_00-00-00_000000Z"


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


_rpc_id = 0


def _rpc(sock, method, params=None):
    global _rpc_id
    _rpc_id += 1
    msg = {"jsonrpc": "2.0", "id": _rpc_id, "method": method}
    if params is not None:
        msg["params"] = params
    sock.sendall((json.dumps(msg) + "\n").encode("utf-8"))
    buf = b""
    deadline = time.time() + 10
    while time.time() < deadline:
        chunk = sock.recv(65536)
        if not chunk:
            break
        buf += chunk
        if b"\n" in buf:
            break
    line, _, _ = buf.partition(b"\n")
    return json.loads(line.decode("utf-8"))


def call(sock, tool, args):
    return _rpc(sock, "tools/call", {"name": tool, "arguments": args})


def unwrap(resp):
    """Extract the value from a tools/call response (text content → parsed JSON)."""
    result = resp.get("result")
    if not isinstance(result, dict):
        return result
    content = result.get("content", [])
    if content and content[0].get("type") == "text":
        text = content[0]["text"]
        try:
            return json.loads(text)
        except (json.JSONDecodeError, TypeError):
            return text
    return result


def main() -> int:
    TMP.mkdir(parents=True, exist_ok=True)
    for path in (DB_PATH, Path(str(DB_PATH) + "-wal"), Path(str(DB_PATH) + "-shm")):
        path.unlink(missing_ok=True)

    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []

            h = unwrap(call(s, "open", {"file": str(DB_PATH)}))["handle"]

            # --- 02.1: upsert_observed inserts a new row when no row exists ---
            call(s, "upsert-observed", {
                "handle": h, "path": "docs/readme.txt",
                "mod_time": TS1, "byte_size": 1024, "is_dir": False, "now": TS1,
            })
            row = unwrap(call(s, "lookup", {"handle": h, "path": "docs/readme.txt"})).get("record")
            print(f"[02.1] lookup after first upsert_observed returned row: {row is not None}")
            if row is None:
                failures.append("02.1: lookup returned None — row was not inserted")

            # --- 02.2: stored mod_time == supplied, last_seen == now, deleted_time null ---
            ok_mt = row.get("mod_time") == TS1 if row else False
            ok_ls = row.get("last_seen") == TS1 if row else False
            ok_dt = row.get("deleted_time") is None if row else False
            print(f"[02.2] mod_time match={ok_mt}, last_seen==now match={ok_ls}, deleted_time null={ok_dt}")
            if not ok_mt:
                failures.append(f"02.2: mod_time expected {TS1!r} got {row.get('mod_time') if row else None!r}")
            if not ok_ls:
                failures.append(f"02.2: last_seen expected {TS1!r} got {row.get('last_seen') if row else None!r}")
            if not ok_dt:
                failures.append(f"02.2: deleted_time expected null got {row.get('deleted_time') if row else None!r}")

            # --- 02.3: second upsert_observed updates (not inserts) the existing row ---
            saved_id = row.get("id") if row else None
            call(s, "upsert-observed", {
                "handle": h, "path": "docs/readme.txt",
                "mod_time": TS2, "byte_size": 2048, "is_dir": False, "now": TS2,
            })
            row2 = unwrap(call(s, "lookup", {"handle": h, "path": "docs/readme.txt"})).get("record")
            same_id = row2.get("id") == saved_id if (row2 and saved_id) else False
            updated_mt = row2.get("mod_time") == TS2 if row2 else False
            updated_bs = row2.get("byte_size") == 2048 if row2 else False
            updated_ls = row2.get("last_seen") == TS2 if row2 else False
            print(f"[02.3] same id={same_id}, mod_time updated={updated_mt}, byte_size updated={updated_bs}, last_seen updated={updated_ls}")
            if not same_id:
                failures.append(f"02.3: row id changed — a new row was inserted instead of updating")
            if not updated_mt:
                failures.append(f"02.3: mod_time not updated: expected {TS2!r} got {row2.get('mod_time') if row2 else None!r}")
            if not updated_bs:
                failures.append(f"02.3: byte_size not updated: expected 2048 got {row2.get('byte_size') if row2 else None!r}")
            if not updated_ls:
                failures.append(f"02.3: last_seen not updated: expected {TS2!r} got {row2.get('last_seen') if row2 else None!r}")

            # --- 02.4: upsert_observed on a tombstoned row clears deleted_time ---
            call(s, "mark-subtree-deleted", {
                "handle": h, "path": "docs/readme.txt", "deleted_time": TS2,
            })
            tombstoned = unwrap(call(s, "lookup", {"handle": h, "path": "docs/readme.txt"})).get("record")
            was_tombstoned = tombstoned.get("deleted_time") is not None if tombstoned else False
            call(s, "upsert-observed", {
                "handle": h, "path": "docs/readme.txt",
                "mod_time": TS3, "byte_size": 512, "is_dir": False, "now": TS3,
            })
            revived = unwrap(call(s, "lookup", {"handle": h, "path": "docs/readme.txt"})).get("record")
            dt_cleared = revived.get("deleted_time") is None if revived else False
            print(f"[02.4] was tombstoned={was_tombstoned}, deleted_time cleared after upsert={dt_cleared}")
            if not was_tombstoned:
                failures.append("02.4: precondition failed — mark_subtree_deleted did not set deleted_time")
            if revived is None:
                failures.append("02.4: row missing after upsert_observed on tombstoned row")
            elif not dt_cleared:
                failures.append(f"02.4: deleted_time not cleared: {revived.get('deleted_time')!r}")

            # --- 02.5a: is_dir=true stores byte_size=-1 regardless of supplied value ---
            call(s, "upsert-observed", {
                "handle": h, "path": "docs/subdir",
                "mod_time": TS1, "byte_size": 999, "is_dir": True, "now": TS1,
            })
            dir_row = unwrap(call(s, "lookup", {"handle": h, "path": "docs/subdir"})).get("record")
            dir_bs = dir_row.get("byte_size") if dir_row else None
            print(f"[02.5a] is_dir=true byte_size={dir_bs} (expected -1)")
            if dir_row is None:
                failures.append("02.5a: directory row not found after upsert_observed")
            elif dir_bs != -1:
                failures.append(f"02.5a: is_dir=true should store byte_size=-1, got {dir_bs!r}")

            # --- 02.5b: is_dir=false stores the supplied byte_size ---
            call(s, "upsert-observed", {
                "handle": h, "path": "docs/notes.txt",
                "mod_time": TS1, "byte_size": 555, "is_dir": False, "now": TS1,
            })
            file_row = unwrap(call(s, "lookup", {"handle": h, "path": "docs/notes.txt"})).get("record")
            file_bs = file_row.get("byte_size") if file_row else None
            print(f"[02.5b] is_dir=false byte_size={file_bs} (expected 555)")
            if file_row is None:
                failures.append("02.5b: file row not found after upsert_observed")
            elif file_bs != 555:
                failures.append(f"02.5b: is_dir=false should store supplied byte_size 555, got {file_bs!r}")

            # --- 02.6: basename equals the final path component ---
            call(s, "upsert-observed", {
                "handle": h, "path": "a/b/myfile.dat",
                "mod_time": TS1, "byte_size": 100, "is_dir": False, "now": TS1,
            })
            bn_row = unwrap(call(s, "lookup", {"handle": h, "path": "a/b/myfile.dat"})).get("record")
            basename = bn_row.get("basename") if bn_row else None
            print(f"[02.6] basename={basename!r} (expected 'myfile.dat')")
            if bn_row is None:
                failures.append("02.6: row not found for a/b/myfile.dat")
            elif basename != "myfile.dat":
                failures.append(f"02.6: basename expected 'myfile.dat' got {basename!r}")

            # --- 02.7: parent_id equals identify(parent_directory_of_path) ---
            expected_parent_id = unwrap(call(s, "identify", {"path": "a/b"}))
            actual_parent_id = bn_row.get("parent_id") if bn_row else None
            print(f"[02.7] parent_id={actual_parent_id!r} expected identify('a/b')={expected_parent_id!r}")
            if not actual_parent_id:
                failures.append("02.7: parent_id missing on row for a/b/myfile.dat")
            elif actual_parent_id != expected_parent_id:
                failures.append(f"02.7: parent_id expected {expected_parent_id!r} got {actual_parent_id!r}")

            # --- 02.8: top-level entry has parent_id equal to root-sentinel identity ---
            call(s, "upsert-observed", {
                "handle": h, "path": "toplevel.txt",
                "mod_time": TS1, "byte_size": 10, "is_dir": False, "now": TS1,
            })
            top_row = unwrap(call(s, "lookup", {"handle": h, "path": "toplevel.txt"})).get("record")
            root_sentinel = unwrap(call(s, "identify", {"path": ""}))
            top_parent_id = top_row.get("parent_id") if top_row else None
            print(f"[02.8] top-level parent_id={top_parent_id!r} root_sentinel={root_sentinel!r}")
            if top_row is None:
                failures.append("02.8: top-level row not found after upsert_observed")
            elif top_parent_id != root_sentinel:
                failures.append(f"02.8: parent_id expected root sentinel {root_sentinel!r} got {top_parent_id!r}")

            call(s, "close", {"handle": h})

            if failures:
                print("\nFAILURES:")
                for f in failures:
                    print(f"  - {f}")
                return 1
            print("\nAll assertions passed.")
            return 0
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()


if __name__ == "__main__":
    sys.exit(main())
