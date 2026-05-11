#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercises purge: expired tombstones and orphaned rows deleted; recent rows preserved."""

from __future__ import annotations

import json, os, socket, subprocess, sys, tempfile, threading, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")


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


def _rpc(sock, method, params=None, rpc_id=1):
    msg = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
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


def _call(sock, tool, arguments=None, rpc_id=1):
    resp = _rpc(sock, "tools/call", {"name": tool, "arguments": arguments or {}}, rpc_id=rpc_id)
    if "error" in resp:
        raise RuntimeError(f"Tool '{tool}' RPC error: {resp['error']}")
    content = (resp.get("result") or {}).get("content", [])
    text = next((c["text"] for c in content if c.get("type") == "text"), None)
    if text is None:
        raise RuntimeError(f"Tool '{tool}' returned no text content; resp={resp}")
    return json.loads(text)


def _find_tool(tool_names, *candidates):
    for c in candidates:
        if c in tool_names:
            return c
    return None


def _get(obj, *keys):
    for k in keys:
        if isinstance(obj, dict) and k in obj:
            return obj[k]
    return None


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []
            _id = [0]

            def next_id():
                _id[0] += 1
                return _id[0]

            tl = _rpc(s, "tools/list", rpc_id=next_id())
            tools = (tl.get("result") or {}).get("tools", [])
            tool_names = {t["name"] for t in tools}

            t_open     = _find_tool(tool_names, "open", "openDb", "open_db", "open_snapshot")
            t_close    = _find_tool(tool_names, "close", "closeDb", "close_db", "close_snapshot")
            t_ts       = _find_tool(tool_names, "current_timestamp", "currentTimestamp")
            t_hash     = _find_tool(tool_names, "hash_path", "hashPath")
            t_upsert_c = _find_tool(tool_names, "upsert_confirmed", "upsertConfirmed")
            t_upsert_u = _find_tool(tool_names, "upsert_unconfirmed", "upsertUnconfirmed")
            t_absent   = _find_tool(tool_names, "mark_absent", "markAbsent")
            t_lookup   = _find_tool(tool_names, "lookup_row", "lookupRow")
            t_purge    = _find_tool(tool_names, "purge", "purge_stale", "purgeStale")

            required = {
                "open": t_open, "close": t_close,
                "current_timestamp": t_ts, "hash_path": t_hash,
                "upsert_confirmed": t_upsert_c, "upsert_unconfirmed": t_upsert_u,
                "mark_absent": t_absent, "lookup_row": t_lookup, "purge": t_purge,
            }
            missing = [k for k, v in required.items() if v is None]
            if missing:
                for m in missing:
                    print(f"[setup] missing tool: {m}")
                    failures.append(f"tool not found: {m}")
                print("\nFAILURES:")
                for f in failures:
                    print(f"  - {f}")
                return 1

            with tempfile.TemporaryDirectory(prefix="test04_purge_") as tmpdir:
                base = Path(tmpdir)

                def open_db(name):
                    r = _call(s, t_open, {"path": str(base / f"{name}.db")}, rpc_id=next_id())
                    return _get(r, "handle") if isinstance(r, dict) else r

                def close_db(handle):
                    _call(s, t_close, {"handle": handle}, rpc_id=next_id())

                def get_ts(handle):
                    r = _call(s, t_ts, {"handle": handle}, rpc_id=next_id())
                    return _get(r, "timestamp", "value") if isinstance(r, dict) else r

                def hash_path_id(path):
                    r = _call(s, t_hash, {"path": path}, rpc_id=next_id())
                    return _get(r, "id", "hash", "value") if isinstance(r, dict) else r

                def upsert_confirmed(handle, path, ts):
                    basename = path.rsplit("/", 1)[-1]
                    _call(s, t_upsert_c, {
                        "handle": handle, "path": path, "basename": basename,
                        "mod_time": ts, "byte_size": 10, "last_seen": ts,
                    }, rpc_id=next_id())

                def upsert_unconfirmed(handle, path, mod_time):
                    basename = path.rsplit("/", 1)[-1]
                    _call(s, t_upsert_u, {
                        "handle": handle, "path": path, "basename": basename,
                        "mod_time": mod_time, "byte_size": 10,
                    }, rpc_id=next_id())

                def mark_absent(handle, path):
                    _call(s, t_absent, {"handle": handle, "path": path}, rpc_id=next_id())

                def do_purge(handle, cutoff):
                    _call(s, t_purge, {"handle": handle, "cutoff": cutoff}, rpc_id=next_id())

                def row_exists(handle, path):
                    row_id = hash_path_id(path)
                    resp = _rpc(s, "tools/call", {
                        "name": t_lookup, "arguments": {"handle": handle, "id": row_id},
                    }, rpc_id=next_id())
                    if "error" in resp:
                        return False
                    content = (resp.get("result") or {}).get("content", [])
                    text = next((c["text"] for c in content if c.get("type") == "text"), None)
                    if text is None:
                        return False
                    parsed = json.loads(text)
                    if parsed is None:
                        return False
                    if isinstance(parsed, dict) and not parsed:
                        return False
                    return True

                # --- 04.1: deleted_time IS NOT NULL AND deleted_time < cutoff → deleted ---
                # Setup: upsert with last_seen=T_early, mark_absent → deleted_time=T_early.
                # Cutoff T_late > T_early, so deleted_time < cutoff → row must be gone.
                h1 = open_db("req04_1")
                t_early_1 = get_ts(h1)
                upsert_confirmed(h1, "file1", t_early_1)
                mark_absent(h1, "file1")           # deleted_time = t_early_1
                t_cutoff_1 = get_ts(h1)            # strictly after t_early_1
                do_purge(h1, t_cutoff_1)
                exists_1 = row_exists(h1, "file1")
                close_db(h1)
                print(f"[04.1] tombstone with deleted_time < cutoff deleted: {not exists_1}")
                if exists_1:
                    failures.append("04.1: row with deleted_time < cutoff was NOT deleted by purge")

                # --- 04.2: deleted_time IS NULL AND (last_seen IS NULL OR last_seen < cutoff) → deleted ---
                h2 = open_db("req04_2")
                t_early_2 = get_ts(h2)
                t_cutoff_2 = get_ts(h2)            # strictly after t_early_2

                # 04.2 case a: last_seen IS NULL (upsert_unconfirmed leaves last_seen NULL)
                upsert_unconfirmed(h2, "file2a", t_early_2)
                # 04.2 case b: last_seen IS NOT NULL but < cutoff
                upsert_confirmed(h2, "file2b", t_early_2)

                do_purge(h2, t_cutoff_2)
                exists_2a = row_exists(h2, "file2a")
                exists_2b = row_exists(h2, "file2b")
                close_db(h2)
                print(f"[04.2a] orphan with last_seen NULL deleted: {not exists_2a}")
                print(f"[04.2b] orphan with last_seen < cutoff deleted: {not exists_2b}")
                if exists_2a:
                    failures.append("04.2: row with last_seen NULL was NOT deleted by purge")
                if exists_2b:
                    failures.append("04.2: row with last_seen < cutoff was NOT deleted by purge")

                # --- 04.3: deleted_time IS NULL AND last_seen >= cutoff → preserved ---
                # Setup: upsert_confirmed with last_seen = cutoff; that satisfies last_seen >= cutoff.
                h3 = open_db("req04_3")
                t_cutoff_3 = get_ts(h3)
                upsert_confirmed(h3, "file3", t_cutoff_3)   # last_seen = cutoff
                do_purge(h3, t_cutoff_3)
                exists_3 = row_exists(h3, "file3")
                close_db(h3)
                print(f"[04.3] active row with last_seen >= cutoff preserved: {exists_3}")
                if not exists_3:
                    failures.append("04.3: row with last_seen >= cutoff was deleted by purge")

                # --- 04.4: deleted_time IS NOT NULL AND deleted_time >= cutoff → preserved ---
                # Setup: upsert_confirmed with last_seen = cutoff, mark_absent → deleted_time = cutoff.
                # deleted_time = cutoff >= cutoff → preserved.
                h4 = open_db("req04_4")
                t_cutoff_4 = get_ts(h4)
                upsert_confirmed(h4, "file4", t_cutoff_4)   # last_seen = cutoff
                mark_absent(h4, "file4")                    # deleted_time = cutoff
                do_purge(h4, t_cutoff_4)
                exists_4 = row_exists(h4, "file4")
                close_db(h4)
                print(f"[04.4] tombstone with deleted_time >= cutoff preserved: {exists_4}")
                if not exists_4:
                    failures.append("04.4: row with deleted_time >= cutoff was deleted by purge")

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
