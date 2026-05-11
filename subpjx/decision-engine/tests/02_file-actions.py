#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Per-peer file-action assertions: 02.14, 02.15, 02.16, 02.18."""

from __future__ import annotations

import json, os, socket, subprocess, sys, threading, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

VALID_FILE_ACTIONS = {
    "copy_from_winner",
    "already_matches",
    "displace_existing_file",
    "displace_existing_directory",
    "displace_then_copy",
    "no_action_no_row",
}

NOW = 1_700_000_000


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
        chunk = sock.recv(8192)
        if not chunk:
            break
        buf += chunk
        if b"\n" in buf:
            break
    line, _, _ = buf.partition(b"\n")
    return json.loads(line.decode("utf-8"))


def _call(sock, tool, args):
    return _rpc(sock, "tools/call", {"name": tool, "arguments": args})


def _peer_actions(result):
    """Return {peer_id: action} from a decide result, tolerating different field names."""
    if not isinstance(result, dict):
        return {}
    for key in ("per_peer", "peers", "peer_results", "peerResults", "perPeer"):
        entries = result.get(key)
        if not isinstance(entries, list):
            continue
        out = {}
        for e in entries:
            if not isinstance(e, dict):
                continue
            pid = e.get("peer_id") or e.get("peerId")
            act = e.get("action")
            if pid and act:
                out[pid] = act
        if out:
            return out
    return {}


def _live_file(mod_time, byte_size):
    return {"kind": "live_file", "mod_time": mod_time, "byte_size": byte_size}


def _snapshot(mod_time, byte_size, last_seen=None, deleted_time=None):
    return {"mod_time": mod_time, "byte_size": byte_size,
            "last_seen": last_seen, "deleted_time": deleted_time}


def _peer(peer_id, role, listing_state, snapshot_row):
    return {"peer_id": peer_id, "role": role,
            "listing_state": listing_state, "snapshot_row": snapshot_row}


def _decide(sock, entry_name, peers):
    return _call(sock, "decide", {
        "entry_name": entry_name,
        "per_peer_inputs": peers,
        "timestamp_tolerance_seconds": 5,
        "now": NOW,
    })


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []

            # ── 02.14: every per-peer action in a file decision is from the closed set ──
            # p_winner: new file (newer mod_time, no snapshot) → wins
            # p_old: existing older file → needs winner's content, has conflicting file
            # p_pristine: absent, no snapshot → no_action_no_row
            r14 = _decide(s, "report.txt", [
                _peer("p_winner", "contributing",
                      _live_file(2000, 500), None),
                _peer("p_old", "contributing",
                      _live_file(1000, 300),
                      _snapshot(1000, 300, NOW - 100)),
                _peer("p_pristine", "contributing",
                      {"kind": "absent"}, None),
            ])
            result14 = r14.get("result")
            actions14 = _peer_actions(result14 or {})
            kind14 = result14.get("kind") if isinstance(result14, dict) else None
            print(f"[02.14] kind={kind14!r}, actions={actions14}")
            if r14.get("error"):
                failures.append(f"02.14: decide error: {r14['error'].get('message')}")
            elif not actions14:
                failures.append(f"02.14: no per-peer actions in result: {result14}")
            else:
                bad = {a for a in actions14.values() if a not in VALID_FILE_ACTIONS}
                if bad:
                    failures.append(f"02.14: action(s) outside closed set: {bad}")
                else:
                    print("[02.14] PASS")

            # ── 02.15: peer whose listing matches winning mod_time+byte_size → already_matches ──
            # src and matcher both have identical live_file; outdated has an older file.
            # The file decision fires (outdated needs an update), and matcher already
            # has the winning content → already_matches.
            r15 = _decide(s, "shared.txt", [
                _peer("src", "contributing",
                      _live_file(3000, 200),
                      _snapshot(3000, 200, NOW - 10)),
                _peer("matcher", "contributing",
                      _live_file(3000, 200),
                      _snapshot(3000, 200, NOW - 10)),
                _peer("outdated", "contributing",
                      _live_file(1000, 50),
                      _snapshot(1000, 50, NOW - 200)),
            ])
            result15 = r15.get("result")
            actions15 = _peer_actions(result15 or {})
            print(f"[02.15] kind={result15.get('kind') if isinstance(result15, dict) else '?'}, actions={actions15}")
            if r15.get("error"):
                failures.append(f"02.15: decide error: {r15['error'].get('message')}")
            elif "matcher" not in actions15:
                failures.append(f"02.15: no action for 'matcher'; result={result15}")
            elif actions15["matcher"] != "already_matches":
                failures.append(f"02.15: expected already_matches for 'matcher', got {actions15['matcher']!r}")
            else:
                print("[02.15] PASS")

            # ── 02.16: absent peer with snapshot history, no conflicting entry → copy_from_winner ──
            # source has the winner file; missing is absent but has a snapshot row
            # (was previously observed, now gone) → needs content, nothing to displace.
            r16 = _decide(s, "needed.txt", [
                _peer("source", "contributing",
                      _live_file(4000, 100),
                      _snapshot(4000, 100, NOW - 5)),
                _peer("missing", "contributing",
                      {"kind": "absent"},
                      _snapshot(2000, 80, NOW - 200)),
            ])
            result16 = r16.get("result")
            actions16 = _peer_actions(result16 or {})
            print(f"[02.16] kind={result16.get('kind') if isinstance(result16, dict) else '?'}, actions={actions16}")
            if r16.get("error"):
                failures.append(f"02.16: decide error: {r16['error'].get('message')}")
            elif "missing" not in actions16:
                failures.append(f"02.16: no action for 'missing'; result={result16}")
            elif actions16["missing"] != "copy_from_winner":
                failures.append(f"02.16: expected copy_from_winner for absent-with-history peer, got {actions16['missing']!r}")
            else:
                print("[02.16] PASS")

            # ── 02.18: absent peer with no snapshot row → no_action_no_row ──
            r18 = _decide(s, "new_file.txt", [
                _peer("has_file", "contributing",
                      _live_file(5000, 400),
                      _snapshot(5000, 400, NOW - 1)),
                _peer("pristine", "contributing",
                      {"kind": "absent"}, None),
            ])
            result18 = r18.get("result")
            actions18 = _peer_actions(result18 or {})
            print(f"[02.18] kind={result18.get('kind') if isinstance(result18, dict) else '?'}, actions={actions18}")
            if r18.get("error"):
                failures.append(f"02.18: decide error: {r18['error'].get('message')}")
            elif "pristine" not in actions18:
                failures.append(f"02.18: no action for 'pristine'; result={result18}")
            elif actions18["pristine"] != "no_action_no_row":
                failures.append(f"02.18: expected no_action_no_row for absent+no-row peer, got {actions18['pristine']!r}")
            else:
                print("[02.18] PASS")

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
