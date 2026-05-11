#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Test per-peer directory action assignments (02.19–02.24) via the decide MCP tool."""

from __future__ import annotations

import json, os, socket, subprocess, sys, threading, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

NOW = 1_700_000_000
TOLS = 5

VALID_DIR_ACTIONS = {
    "create_directory",
    "displace_existing_file_then_create",
    "displace_directory",
    "recurse_only",
    "no_action_no_row",
}


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


def _rpc(sock, method, params, rpc_id):
    msg = {"jsonrpc": "2.0", "id": rpc_id, "method": method, "params": params}
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


def _decide(sock, peers, rpc_id):
    return _rpc(sock, "tools/call", {
        "name": "decide",
        "arguments": {
            "entry_name": "testdir",
            "per_peer_inputs": peers,
            "timestamp_tolerance_seconds": TOLS,
            "now": NOW,
        },
    }, rpc_id)


def _peer(peer_id, role, listing_state, snapshot_row):
    return {"peer_id": peer_id, "role": role,
            "listing_state": listing_state, "snapshot_row": snapshot_row}


def _live_dir(mod_time=NOW):
    return {"kind": "live_dir", "mod_time": mod_time}


def _live_file(mod_time=NOW, byte_size=1024):
    return {"kind": "live_file", "mod_time": mod_time, "byte_size": byte_size}


def _absent():
    return {"kind": "absent"}


def _snap(mod_time=NOW, last_seen=None):
    return {"mod_time": mod_time, "byte_size": 0,
            "last_seen": last_seen if last_seen is not None else mod_time,
            "deleted_time": None}


def _kind(r):
    return (r.get("result") or {}).get("kind")


def _action(r, peer_id):
    for o in (r.get("result") or {}).get("per_peer_outcomes", []):
        if o.get("peer_id") == peer_id:
            return o.get("action")
    return None


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []
            seq = 0

            def nid():
                nonlocal seq
                seq += 1
                return seq

            # 02.19 — every per-peer action in a directory decision is from the valid set
            r = _decide(s, [
                _peer("p1", "contributing", _live_dir(), _snap()),
                _peer("p2", "contributing", _absent(), _snap()),
            ], nid())
            k = _kind(r)
            print(f"[02.19] decision kind={k!r}")
            if k not in ("directory", "type_conflict_directory_wins"):
                failures.append(f"02.19: expected directory/type_conflict_directory_wins, got {k!r}")
            else:
                outcomes = (r.get("result") or {}).get("per_peer_outcomes", [])
                bad = [o.get("action") for o in outcomes
                       if o.get("action") not in VALID_DIR_ACTIONS]
                if bad:
                    failures.append(f"02.19: invalid actions {bad}")
                else:
                    print(f"[02.19] actions {[o.get('action') for o in outcomes]} all valid")

            # 02.20 — peer with live_dir listing receives recurse_only
            r = _decide(s, [
                _peer("dir-peer", "contributing", _live_dir(), _snap()),
                _peer("abs-peer", "contributing", _absent(), _snap()),
            ], nid())
            a = _action(r, "dir-peer")
            print(f"[02.20] dir-peer action={a!r}")
            if a != "recurse_only":
                failures.append(f"02.20: live_dir peer got {a!r}, want recurse_only")

            # 02.21 — peer with absent listing and no conflicting entry receives create_directory
            r = _decide(s, [
                _peer("src", "contributing", _live_dir(), _snap()),
                _peer("absent-has-row", "contributing", _absent(), _snap()),
            ], nid())
            a = _action(r, "absent-has-row")
            print(f"[02.21] absent peer (has snapshot row) action={a!r}")
            if a != "create_directory":
                failures.append(f"02.21: absent peer with snapshot row got {a!r}, want create_directory")

            # 02.22 — type_conflict_directory_wins: live_file peer gets displace_existing_file_then_create
            r = _decide(s, [
                _peer("canon-dir", "canon", _live_dir(), _snap()),
                _peer("file-peer", "contributing", _live_file(), _snap()),
            ], nid())
            k = _kind(r)
            a = _action(r, "file-peer")
            print(f"[02.22] kind={k!r} file-peer action={a!r}")
            if k != "type_conflict_directory_wins":
                failures.append(f"02.22: expected type_conflict_directory_wins, got {k!r}")
            if a != "displace_existing_file_then_create":
                failures.append(f"02.22: file-peer got {a!r}, want displace_existing_file_then_create")

            # 02.23 — peer with absent listing and snapshot_row=none receives no_action_no_row
            r = _decide(s, [
                _peer("src2", "contributing", _live_dir(), _snap()),
                _peer("no-row", "contributing", _absent(), None),
            ], nid())
            a = _action(r, "no-row")
            print(f"[02.23] absent peer (no snapshot row) action={a!r}")
            if a != "no_action_no_row":
                failures.append(f"02.23: absent peer with no snapshot row got {a!r}, want no_action_no_row")

            # 02.24 — directory decision is existence-based, not mod_time-based
            r_a = _decide(s, [
                _peer("p1", "contributing", _live_dir(mod_time=1_000), _snap(mod_time=1_000)),
                _peer("p2", "contributing", _live_dir(mod_time=999_000_000), _snap(mod_time=999_000_000)),
            ], nid())
            r_b = _decide(s, [
                _peer("p1", "contributing", _live_dir(mod_time=999_000_000), _snap(mod_time=999_000_000)),
                _peer("p2", "contributing", _live_dir(mod_time=1_000), _snap(mod_time=1_000)),
            ], nid())
            k_a, k_b = _kind(r_a), _kind(r_b)
            a1_a, a2_a = _action(r_a, "p1"), _action(r_a, "p2")
            a1_b, a2_b = _action(r_b, "p1"), _action(r_b, "p2")
            print(f"[02.24] low-mod_time: kind={k_a!r} p1={a1_a!r} p2={a2_a!r}")
            print(f"[02.24] high-mod_time: kind={k_b!r} p1={a1_b!r} p2={a2_b!r}")
            if k_a != "directory":
                failures.append(f"02.24: variant-a kind={k_a!r}, want directory")
            if k_b != "directory":
                failures.append(f"02.24: variant-b kind={k_b!r}, want directory")
            if a1_a != "recurse_only" or a2_a != "recurse_only":
                failures.append(f"02.24: variant-a actions ({a1_a!r},{a2_a!r}) should both be recurse_only")
            if a1_b != "recurse_only" or a2_b != "recurse_only":
                failures.append(f"02.24: variant-b actions ({a1_b!r},{a2_b!r}) should both be recurse_only")

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
