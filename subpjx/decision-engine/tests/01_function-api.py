#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercises REQ 01 (function-api): decide and classify-file are pure, deterministic, and stable on peer_id."""

from __future__ import annotations

import json, os, socket, subprocess, sys, threading, time
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
        chunk = sock.recv(8192)
        if not chunk:
            break
        buf += chunk
        if b"\n" in buf:
            break
    line, _, _ = buf.partition(b"\n")
    return json.loads(line.decode("utf-8"))


def _collect_values(obj, field_name):
    found = []
    if isinstance(obj, dict):
        if field_name in obj and obj[field_name] is not None:
            found.append(obj[field_name])
        for v in obj.values():
            found.extend(_collect_values(v, field_name))
    elif isinstance(obj, list):
        for item in obj:
            found.extend(_collect_values(item, field_name))
    return found


def _find_value(obj, field_name, target):
    if isinstance(obj, dict):
        if obj.get(field_name) == target:
            return True
        return any(_find_value(v, field_name, target) for v in obj.values())
    if isinstance(obj, list):
        return any(_find_value(item, field_name, target) for item in obj)
    return False


NOW = 1_700_000_000
TOLERANCE = 5

# File-present scenario: peer-a has live file matching snapshot → upsert_present with last_seen=NOW
# peer-b is absent with no snapshot → copy_from_winner directive, no last_seen emitted
DECIDE_ARGS_PRESENT = {
    "entry_name": "test.txt",
    "per_peer_inputs": [
        {
            "peer_id": "peer-a",
            "role": "contributing",
            "listing_state": {"type": "live_file", "mod_time": NOW - 50, "byte_size": 100},
            "snapshot_row": {
                "type": "row",
                "mod_time": NOW - 50,
                "byte_size": 100,
                "last_seen": NOW - 300,
                "deleted_time": None,
            },
        },
        {
            "peer_id": "peer-b",
            "role": "contributing",
            "listing_state": {"type": "absent"},
            "snapshot_row": {"type": "none"},
        },
    ],
    "timestamp_tolerance_seconds": TOLERANCE,
    "now": NOW,
}

# Deletion scenario: both peers absent with snapshot showing file existed → mark_tombstone with deleted_time=NOW
DECIDE_ARGS_DELETED = {
    "entry_name": "deleted.txt",
    "per_peer_inputs": [
        {
            "peer_id": "peer-a",
            "role": "contributing",
            "listing_state": {"type": "absent"},
            "snapshot_row": {
                "type": "row",
                "mod_time": NOW - 100,
                "byte_size": 42,
                "last_seen": NOW - 200,
                "deleted_time": None,
            },
        },
        {
            "peer_id": "peer-b",
            "role": "contributing",
            "listing_state": {"type": "absent"},
            "snapshot_row": {
                "type": "row",
                "mod_time": NOW - 100,
                "byte_size": 42,
                "last_seen": NOW - 200,
                "deleted_time": None,
            },
        },
    ],
    "timestamp_tolerance_seconds": TOLERANCE,
    "now": NOW,
}

# classify-file: live file matching snapshot → unchanged classification
CLASSIFY_ARGS = {
    "listing_state": {"type": "live_file", "mod_time": NOW - 50, "byte_size": 100},
    "snapshot_row": {
        "type": "row",
        "mod_time": NOW - 50,
        "byte_size": 100,
        "last_seen": NOW - 300,
        "deleted_time": None,
    },
    "timestamp_tolerance_seconds": TOLERANCE,
    "now": NOW,
}


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []
            call_id = 0

            def rpc(method, params=None):
                nonlocal call_id
                call_id += 1
                return _rpc(s, method, params, rpc_id=call_id)

            tl = rpc("tools/list")
            tools = {t["name"]: t for t in (tl.get("result") or {}).get("tools", [])}
            print(f"[info] tools/list: {sorted(tools.keys())}")

            # --- 01.1: every last_seen / deleted_time emitted matches configured now ---
            r_present = rpc("tools/call", {"name": "decide", "arguments": DECIDE_ARGS_PRESENT})
            r_deleted = rpc("tools/call", {"name": "decide", "arguments": DECIDE_ARGS_DELETED})
            bad_ts = []
            for label, r in [("present", r_present), ("deleted", r_deleted)]:
                if "error" in r:
                    failures.append(f"01.1: decide({label}) error: {r['error']}")
                    print(f"[FAIL] 01.1: decide({label}) error: {r['error']}")
                    continue
                result = r.get("result", {})
                for field in ("last_seen", "deleted_time"):
                    for val in _collect_values(result, field):
                        if val != NOW:
                            bad_ts.append(f"{label}.{field}={val!r} (expected {NOW})")
            if bad_ts:
                failures.append(f"01.1: timestamp mismatch: {bad_ts}")
                print(f"[FAIL] 01.1: {bad_ts}")
            elif not any("error" in r for r in (r_present, r_deleted)):
                print("[PASS] 01.1: every last_seen/deleted_time in decision matches configured now")

            # --- 01.2: decide is deterministic ---
            r2a = rpc("tools/call", {"name": "decide", "arguments": DECIDE_ARGS_PRESENT})
            r2b = rpc("tools/call", {"name": "decide", "arguments": DECIDE_ARGS_PRESENT})
            if "error" in r2a or "error" in r2b:
                err = r2a.get("error") or r2b.get("error")
                failures.append(f"01.2: decide error: {err}")
                print(f"[FAIL] 01.2: decide error: {err}")
            elif r2a.get("result") != r2b.get("result"):
                failures.append("01.2: decide not deterministic")
                print(f"[FAIL] 01.2: first={r2a['result']} second={r2b['result']}")
            else:
                print("[PASS] 01.2: decide is deterministic (same inputs → equivalent output)")

            # --- 01.3: classify-file is deterministic ---
            r3a = rpc("tools/call", {"name": "classify-file", "arguments": CLASSIFY_ARGS})
            r3b = rpc("tools/call", {"name": "classify-file", "arguments": CLASSIFY_ARGS})
            if "error" in r3a or "error" in r3b:
                err = r3a.get("error") or r3b.get("error")
                failures.append(f"01.3: classify-file error: {err}")
                print(f"[FAIL] 01.3: classify-file error: {err}")
            elif r3a.get("result") != r3b.get("result"):
                failures.append("01.3: classify-file not deterministic")
                print(f"[FAIL] 01.3: first={r3a['result']} second={r3b['result']}")
            else:
                print(f"[PASS] 01.3: classify-file is deterministic (result: {r3a.get('result')})")

            # --- 01.4: no filesystem writes in cwd during decide/classify-file ---
            entries_before = set(os.listdir("."))
            rpc("tools/call", {"name": "decide", "arguments": DECIDE_ARGS_PRESENT})
            rpc("tools/call", {"name": "classify-file", "arguments": CLASSIFY_ARGS})
            new_entries = set(os.listdir(".")) - entries_before
            if new_entries:
                failures.append(f"01.4: unexpected cwd entries after calls: {new_entries}")
                print(f"[FAIL] 01.4: new entries in cwd: {new_entries}")
            else:
                print("[PASS] 01.4: no new filesystem entries in cwd during decide/classify-file")

            # --- 01.5: peer_id passes through unchanged in per-peer output ---
            PEER_ID = "opaque-handle-xyzzy-deadbeef"
            r5 = rpc("tools/call", {"name": "decide", "arguments": {
                "entry_name": "check.txt",
                "per_peer_inputs": [
                    {
                        "peer_id": PEER_ID,
                        "role": "contributing",
                        "listing_state": {"type": "live_file", "mod_time": NOW - 10, "byte_size": 77},
                        "snapshot_row": {"type": "none"},
                    },
                ],
                "timestamp_tolerance_seconds": TOLERANCE,
                "now": NOW,
            }})
            if "error" in r5:
                failures.append(f"01.5: decide error: {r5['error']}")
                print(f"[FAIL] 01.5: decide error: {r5['error']}")
            elif not _find_value(r5.get("result", {}), "peer_id", PEER_ID):
                failures.append(f"01.5: peer_id '{PEER_ID}' not found in decision output")
                print(f"[FAIL] 01.5: result={r5.get('result')}")
            else:
                print(f"[PASS] 01.5: peer_id '{PEER_ID}' appears unchanged in decision output")

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
