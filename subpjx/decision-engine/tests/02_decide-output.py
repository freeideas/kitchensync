#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercises 02.8, 02.11, 02.12: decide kind is valid; noop when all peers match or have no row; file decision carries winning metadata."""

from __future__ import annotations

import json, os, socket, subprocess, sys, threading, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

NOW   = "2024-06-01_12-00-00_000000Z"
MOD_T = "2024-06-01_10-00-00_000000Z"

VALID_KINDS = {"file", "directory", "type_conflict_file_wins", "type_conflict_directory_wins", "noop"}

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


def decide(sock, entry_name, peers):
    return call(sock, "decide", {
        "entry_name": entry_name,
        "timestamp_tolerance_seconds": 5,
        "now": NOW,
        "per_peer_inputs": peers,
    })


def main() -> int:
    proc, port = _launch()
    failures = []
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:

            # verify the decide tool is advertised
            tl = _rpc(s, "tools/list")
            tools = {t["name"] for t in (tl.get("result") or {}).get("tools", [])}
            if "decide" not in tools:
                failures.append(f"decide tool not in tools/list; got {sorted(tools)}")
                print(f"[pre] decide tool present: False — {sorted(tools)}")
            else:
                print(f"[pre] decide tool present: True")

            # ── 02.8: kind is one of the five valid values ────────────────────
            # peer_a has a new file; peer_b is absent with no row.
            r8 = decide(s, "notes.txt", [
                {
                    "peer_id": "peer_a",
                    "role": "contributing",
                    "listing_state": {"kind": "live_file", "mod_time": MOD_T, "byte_size": 512},
                    "snapshot_row": None,
                },
                {
                    "peer_id": "peer_b",
                    "role": "contributing",
                    "listing_state": {"kind": "absent"},
                    "snapshot_row": None,
                },
            ])
            kind_8 = (r8.get("result") or {}).get("kind")
            ok_8 = kind_8 in VALID_KINDS
            print(f"[02.8] kind is one of the five valid values (got '{kind_8}'): {ok_8}")
            if not ok_8:
                failures.append(f"02.8: kind '{kind_8}' not in {VALID_KINDS}; response={r8}")

            # ── 02.11: noop when every peer already matches the group's view ──
            # Both peers have the same confirmed file (mod_time and byte_size
            # match their snapshot row) — the group's view is satisfied, no
            # copies needed.
            confirmed_row = {
                "mod_time": MOD_T,
                "byte_size": 256,
                "last_seen": MOD_T,
                "deleted_time": None,
            }
            r11 = decide(s, "sync.txt", [
                {
                    "peer_id": "peer_a",
                    "role": "contributing",
                    "listing_state": {"kind": "live_file", "mod_time": MOD_T, "byte_size": 256},
                    "snapshot_row": confirmed_row,
                },
                {
                    "peer_id": "peer_b",
                    "role": "contributing",
                    "listing_state": {"kind": "live_file", "mod_time": MOD_T, "byte_size": 256},
                    "snapshot_row": confirmed_row,
                },
            ])
            kind_11 = (r11.get("result") or {}).get("kind")
            ok_11 = kind_11 == "noop"
            print(f"[02.11] noop when every peer already matches (got '{kind_11}'): {ok_11}")
            if not ok_11:
                failures.append(f"02.11: expected 'noop', got '{kind_11}'; response={r11}")

            # ── 02.12: file / type_conflict_file_wins carries winning metadata ─
            # peer_a has a new file (no snapshot); peer_b is absent (no snapshot).
            # Decision is 'file'; peer_a is the winning source.
            r12 = decide(s, "report.txt", [
                {
                    "peer_id": "peer_a",
                    "role": "contributing",
                    "listing_state": {"kind": "live_file", "mod_time": MOD_T, "byte_size": 1024},
                    "snapshot_row": None,
                },
                {
                    "peer_id": "peer_b",
                    "role": "contributing",
                    "listing_state": {"kind": "absent"},
                    "snapshot_row": None,
                },
            ])
            result_12 = (r12.get("result") or {})
            kind_12 = result_12.get("kind")
            has_meta = (
                "winning_mod_time" in result_12
                and "winning_byte_size" in result_12
                and "winning_source_peer_id" in result_12
            )
            ok_12 = kind_12 in {"file", "type_conflict_file_wins"} and has_meta
            print(f"[02.12] file decision carries winning metadata (kind='{kind_12}', has_meta={has_meta}): {ok_12}")
            if not ok_12:
                failures.append(f"02.12: expected file kind with winning metadata; response={r12}")

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
