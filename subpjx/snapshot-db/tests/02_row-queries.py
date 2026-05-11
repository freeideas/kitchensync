#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Tests row lookup by id and child-row listing (02_row-queries)."""

from __future__ import annotations

import json, os, socket, subprocess, sys, tempfile, threading, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

_rpc_counter = [0]


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
    _rpc_counter[0] += 1
    msg = {"jsonrpc": "2.0", "id": _rpc_counter[0], "method": method}
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


def _call(sock, tool, arguments=None):
    resp = _rpc(sock, "tools/call", {"name": tool, "arguments": arguments or {}})
    if "error" in resp:
        raise RuntimeError(f"Tool '{tool}' error: {resp['error']}")
    result = resp.get("result") or {}
    # Handle content-wrapped responses
    content = result.get("content") if isinstance(result, dict) else None
    if isinstance(content, list):
        text = next((c.get("text") for c in content if c.get("type") == "text"), None)
        if text is not None:
            return json.loads(text)
    return result


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

            tl = _rpc(s, "tools/list")
            tools = (tl.get("result") or {}).get("tools", [])
            tool_names = {t["name"] for t in tools}

            t_open   = _find_tool(tool_names, "open-snapshot", "open-db", "open")
            t_close  = _find_tool(tool_names, "close-snapshot", "close-db", "close-handle", "close")
            t_hash   = _find_tool(tool_names, "hash-path", "hash")
            t_ts     = _find_tool(tool_names, "current-timestamp", "timestamp")
            t_lookup = _find_tool(tool_names, "lookup-row", "lookup-row-by-id", "get-row")
            t_list   = _find_tool(tool_names, "list-child-rows", "list-children", "list-rows")
            t_upsert = _find_tool(tool_names,
                                   "upsert-confirmed-present", "upsert-present", "upsert")

            with tempfile.TemporaryDirectory() as tmpdir:
                db_path = str(Path(tmpdir) / "test.db")

                if t_open is None:
                    print("[setup] FATAL: open-snapshot tool not found")
                    failures.append("setup: open-snapshot tool not found")
                    return 1

                open_resp = _call(s, t_open, {"path": db_path})
                handle = _get(open_resp, "handle", "handleId", "handle_id", "id")
                print(f"[setup] opened db, handle={handle!r}")
                if handle is None:
                    print("[setup] FATAL: open-snapshot returned no handle")
                    failures.append("setup: open-snapshot returned no handle")
                    return 1

                # Obtain a timestamp for upserts; fall back to a static string.
                upsert_ts = "2024-01-01_00-00-00_000000Z"
                if t_ts is not None:
                    try:
                        ts_resp = _call(s, t_ts, {"handle": handle})
                        upsert_ts = _get(ts_resp, "timestamp", "value", "ts") or upsert_ts
                    except RuntimeError:
                        pass

                # --- 02.10: lookup by id with no stored row returns no row ---
                if t_lookup is None:
                    failures.append("02.10: lookup-row tool not found")
                    print("[02.10] SKIP: lookup-row tool not found")
                else:
                    try:
                        r = _call(s, t_lookup, {"handle": handle, "id": "XXXXXXXXXXX"})
                        row = _get(r, "row") if isinstance(r, dict) and "row" in r else r
                        absent = row is None or (isinstance(row, dict) and not row)
                        print(f"[02.10] lookup missing id -> row={row!r}, absent={absent}")
                        if not absent:
                            failures.append(f"02.10: expected no row for unknown id, got {row!r}")
                    except RuntimeError as e:
                        failures.append(f"02.10: RPC error: {e}")
                        print(f"[02.10] RPC error: {e}")

                # --- 02.11: lookup after write returns row with matching fields ---
                if t_lookup is None or t_upsert is None or t_hash is None:
                    failures.append("02.11: required tools missing (lookup-row, upsert, hash-path)")
                    print("[02.11] SKIP: required tools missing")
                else:
                    try:
                        path11 = "docs/readme.txt"
                        mod_time11 = "2024-06-15_12-30-00_000000Z"
                        byte_size11 = 1024

                        _call(s, t_upsert, {
                            "handle": handle,
                            "path": path11,
                            "basename": "readme.txt",
                            "modTime": mod_time11,
                            "byteSize": byte_size11,
                            "lastSeen": upsert_ts,
                        })

                        hash_resp = _call(s, t_hash, {"path": path11})
                        row_id = _get(hash_resp, "id", "hash", "value")

                        parent_hash_resp = _call(s, t_hash, {"path": "docs"})
                        expected_parent_id = _get(parent_hash_resp, "id", "hash", "value")

                        r11 = _call(s, t_lookup, {"handle": handle, "id": row_id})
                        row11 = _get(r11, "row") if isinstance(r11, dict) and "row" in r11 else r11

                        print(f"[02.11] lookup after upsert -> {row11!r}")
                        if not isinstance(row11, dict) or not row11:
                            failures.append(f"02.11: expected a non-empty row, got {row11!r}")
                        else:
                            got_parent  = _get(row11, "parentId", "parent_id")
                            got_base    = _get(row11, "basename")
                            got_mod     = _get(row11, "modTime", "mod_time")
                            got_size    = _get(row11, "byteSize", "byte_size")
                            got_seen    = _get(row11, "lastSeen", "last_seen")
                            got_deleted = _get(row11, "deletedTime", "deleted_time")

                            if got_parent != expected_parent_id:
                                failures.append(
                                    f"02.11: parent_id: got {got_parent!r}, "
                                    f"want {expected_parent_id!r}")
                            if got_base != "readme.txt":
                                failures.append(
                                    f"02.11: basename: got {got_base!r}, want 'readme.txt'")
                            if got_mod != mod_time11:
                                failures.append(
                                    f"02.11: mod_time: got {got_mod!r}, want {mod_time11!r}")
                            if got_size != byte_size11:
                                failures.append(
                                    f"02.11: byte_size: got {got_size!r}, want {byte_size11!r}")
                            if got_seen != upsert_ts:
                                failures.append(
                                    f"02.11: last_seen: got {got_seen!r}, want {upsert_ts!r}")
                            if "deletedTime" in row11 or "deleted_time" in row11:
                                if got_deleted is not None:
                                    failures.append(
                                        f"02.11: deleted_time: got {got_deleted!r}, want None")
                    except RuntimeError as e:
                        failures.append(f"02.11: RPC error: {e}")
                        print(f"[02.11] RPC error: {e}")

                # --- 02.12: list children returns every row with matching parent_id ---
                if t_list is None or t_upsert is None or t_hash is None:
                    failures.append("02.12: required tools missing (list-child-rows, upsert, hash-path)")
                    print("[02.12] SKIP: required tools missing")
                else:
                    try:
                        _call(s, t_upsert, {
                            "handle": handle,
                            "path": "images/photo1.jpg",
                            "basename": "photo1.jpg",
                            "modTime": "2024-01-01_00-00-00_000000Z",
                            "byteSize": 2048,
                            "lastSeen": upsert_ts,
                        })
                        _call(s, t_upsert, {
                            "handle": handle,
                            "path": "images/photo2.jpg",
                            "basename": "photo2.jpg",
                            "modTime": "2024-01-02_00-00-00_000000Z",
                            "byteSize": 3072,
                            "lastSeen": upsert_ts,
                        })

                        hr = _call(s, t_hash, {"path": "images"})
                        images_id = _get(hr, "id", "hash", "value")

                        lr = _call(s, t_list, {"handle": handle, "parentId": images_id})
                        rows12 = (
                            _get(lr, "rows") if isinstance(lr, dict) and "rows" in lr
                            else (lr if isinstance(lr, list) else [])
                        )
                        if not isinstance(rows12, list):
                            rows12 = []

                        basenames12 = {_get(r, "basename") for r in rows12}
                        print(f"[02.12] list children of 'images' -> {len(rows12)} row(s): "
                              f"{basenames12}")
                        if "photo1.jpg" not in basenames12:
                            failures.append(
                                f"02.12: photo1.jpg missing from children, got {basenames12!r}")
                        if "photo2.jpg" not in basenames12:
                            failures.append(
                                f"02.12: photo2.jpg missing from children, got {basenames12!r}")
                    except RuntimeError as e:
                        failures.append(f"02.12: RPC error: {e}")
                        print(f"[02.12] RPC error: {e}")

                # --- 02.13: list children of id with no children returns empty ---
                if t_list is None or t_hash is None:
                    failures.append("02.13: required tools missing (list-child-rows, hash-path)")
                    print("[02.13] SKIP: required tools missing")
                else:
                    try:
                        hr = _call(s, t_hash, {"path": "empty/dir"})
                        empty_id = _get(hr, "id", "hash", "value")

                        lr = _call(s, t_list, {"handle": handle, "parentId": empty_id})
                        rows13 = (
                            _get(lr, "rows") if isinstance(lr, dict) and "rows" in lr
                            else (lr if isinstance(lr, list) else None)
                        )

                        print(f"[02.13] list children of empty parent -> {rows13!r}")
                        if rows13 is not None and not (isinstance(rows13, list) and len(rows13) == 0):
                            failures.append(f"02.13: expected empty list, got {rows13!r}")
                    except RuntimeError as e:
                        failures.append(f"02.13: RPC error: {e}")
                        print(f"[02.13] RPC error: {e}")

                if t_close is not None:
                    try:
                        _call(s, t_close, {"handle": handle})
                    except RuntimeError:
                        pass

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
