#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Type-conflict resolution: file wins by default; canon dir peer forces directory wins."""

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


NOW = 1_700_000_000
TOLERANCE = 5


def _decide(sock, peers, rpc_id):
    return _rpc(sock, "tools/call", {
        "name": "decide",
        "arguments": {
            "entry_name": "notes.txt",
            "per_peer_inputs": peers,
            "timestamp_tolerance_seconds": TOLERANCE,
            "now": NOW,
        },
    }, rpc_id=rpc_id)


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []

            # --- 03.1: file-vs-dir disagreement produces a type_conflict kind ---
            peers_file_vs_dir = [
                {
                    "peer_id": "peer-a",
                    "role": "contributing",
                    "listing_state": {"type": "live_file", "mod_time": NOW - 100, "byte_size": 512},
                    "snapshot_row": {"type": "none"},
                },
                {
                    "peer_id": "peer-b",
                    "role": "contributing",
                    "listing_state": {"type": "live_dir", "mod_time": NOW - 200},
                    "snapshot_row": {"type": "none"},
                },
            ]
            resp = _decide(s, peers_file_vs_dir, rpc_id=1)
            kind = (resp.get("result") or {}).get("kind", "")
            print(f"[03.1] file-vs-dir kind={kind!r}")
            if kind not in ("type_conflict_file_wins", "type_conflict_directory_wins"):
                failures.append(
                    f"03.1: expected type_conflict_file_wins or type_conflict_directory_wins, got {kind!r}"
                )

            # --- 03.2: no canon peer -> type_conflict_file_wins ---
            resp = _decide(s, peers_file_vs_dir, rpc_id=2)
            kind = (resp.get("result") or {}).get("kind", "")
            print(f"[03.2] no-canon file-vs-dir kind={kind!r}")
            if kind != "type_conflict_file_wins":
                failures.append(f"03.2: expected type_conflict_file_wins, got {kind!r}")

            # --- 03.4: canon peer reporting dir -> type_conflict_directory_wins ---
            peers_canon_dir = [
                {
                    "peer_id": "peer-a",
                    "role": "contributing",
                    "listing_state": {"type": "live_file", "mod_time": NOW - 100, "byte_size": 512},
                    "snapshot_row": {"type": "none"},
                },
                {
                    "peer_id": "peer-canon",
                    "role": "canon",
                    "listing_state": {"type": "live_dir", "mod_time": NOW - 200},
                    "snapshot_row": {"type": "none"},
                },
            ]
            resp = _decide(s, peers_canon_dir, rpc_id=3)
            kind = (resp.get("result") or {}).get("kind", "")
            print(f"[03.4] canon-dir kind={kind!r}")
            if kind != "type_conflict_directory_wins":
                failures.append(f"03.4: expected type_conflict_directory_wins, got {kind!r}")

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
