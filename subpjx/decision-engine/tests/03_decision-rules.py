#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Decision rules: mod_time winner, byte_size tiebreaker, existence-over-deletion, absent_unconfirmed Rule 4b."""

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


def _decide(sock, entry_name, peers, tolerance=5, now=10000, rpc_id=1):
    return _rpc(sock, "tools/call", {
        "name": "decide",
        "arguments": {
            "entry_name": entry_name,
            "per_peer_inputs": peers,
            "timestamp_tolerance_seconds": tolerance,
            "now": now,
        },
    }, rpc_id=rpc_id)


def _peer_live(peer_id, role, mod_time, byte_size, snapshot=None):
    return {
        "peer_id": peer_id,
        "role": role,
        "listing_state": {"type": "live_file", "mod_time": mod_time, "byte_size": byte_size},
        "snapshot_row": snapshot,
    }


def _peer_absent(peer_id, role, snapshot=None):
    return {
        "peer_id": peer_id,
        "role": role,
        "listing_state": {"type": "absent"},
        "snapshot_row": snapshot,
    }


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []

            # 03.12: peer with latest mod_time (outside tolerance) is the winner;
            # winning_source_peer_id, winning_mod_time, winning_byte_size reflect that peer.
            resp = _decide(s, "report.txt", [
                _peer_live("peer-a", "contributing", mod_time=1000, byte_size=100),
                _peer_live("peer-b", "contributing", mod_time=2000, byte_size=50),
            ], rpc_id=1)
            r = resp.get("result", {})
            ok = (
                r.get("kind") in ("file", "type_conflict_file_wins")
                and r.get("winning_source_peer_id") == "peer-b"
                and r.get("winning_mod_time") == 2000
                and r.get("winning_byte_size") == 50
            )
            print(f"[03.12] latest mod_time wins: {'PASS' if ok else 'FAIL'} — "
                  f"winner={r.get('winning_source_peer_id')!r} "
                  f"mod_time={r.get('winning_mod_time')} byte_size={r.get('winning_byte_size')}")
            if not ok:
                failures.append(f"03.12: want winner=peer-b mod_time=2000 byte_size=50, got {r!r}")

            # 03.13: when mod_times fall within timestamp_tolerance_seconds, byte_size is the tiebreaker;
            # peer-b has mod_time 3 s later (within 5 s tolerance) and larger byte_size → wins.
            resp = _decide(s, "report.txt", [
                _peer_live("peer-a", "contributing", mod_time=1000, byte_size=100),
                _peer_live("peer-b", "contributing", mod_time=1003, byte_size=200),
            ], rpc_id=2)
            r = resp.get("result", {})
            ok = (
                r.get("kind") in ("file", "type_conflict_file_wins")
                and r.get("winning_source_peer_id") == "peer-b"
            )
            print(f"[03.13] byte_size tiebreaker within tolerance: {'PASS' if ok else 'FAIL'} — "
                  f"winner={r.get('winning_source_peer_id')!r}")
            if not ok:
                failures.append(f"03.13: want winner=peer-b (larger byte_size tiebreaker), got {r!r}")

            # 03.14: a live peer overrides a peer whose snapshot_row has a non-null deleted_time
            # when mod_times would otherwise tie (existence-over-deletion).
            # peer-a: live file, snapshot has no deleted_time.
            # peer-b: absent, snapshot carries deleted_time (entry was deleted on peer-b).
            # Same mod_time on both snapshots → tie broken by existence rule → peer-a wins.
            resp = _decide(s, "report.txt", [
                _peer_live("peer-a", "contributing", mod_time=1000, byte_size=100,
                           snapshot={"mod_time": 1000, "byte_size": 100,
                                     "last_seen": 900, "deleted_time": None}),
                _peer_absent("peer-b", "contributing",
                             snapshot={"mod_time": 1000, "byte_size": 100,
                                       "last_seen": 900, "deleted_time": 950}),
            ], rpc_id=3)
            r = resp.get("result", {})
            ok = (
                r.get("kind") in ("file", "type_conflict_file_wins")
                and r.get("winning_source_peer_id") == "peer-a"
            )
            print(f"[03.14] existence-over-deletion tiebreaker: {'PASS' if ok else 'FAIL'} — "
                  f"winner={r.get('winning_source_peer_id')!r}")
            if not ok:
                failures.append(f"03.14: want live peer-a to win over tombstoned peer-b, got {r!r}")

            # 03.15: an absent_unconfirmed peer (absent listing, snapshot with last_seen but no deleted_time)
            # is reconciled against the maximum mod_time across the other peers (Rule 4b).
            # peer-a: absent_unconfirmed — last known mod_time=1500; peer-b: live at mod_time=2000.
            # peer-b's mod_time (2000) > peer-a's last known mod_time (1500) → peer-b wins,
            # peer-a should receive the file (copy_from_winner).
            resp = _decide(s, "report.txt", [
                _peer_absent("peer-a", "contributing",
                             snapshot={"mod_time": 1500, "byte_size": 80,
                                       "last_seen": 1400, "deleted_time": None}),
                _peer_live("peer-b", "contributing", mod_time=2000, byte_size=120,
                           snapshot={"mod_time": 2000, "byte_size": 120,
                                     "last_seen": 1900, "deleted_time": None}),
            ], rpc_id=4)
            r = resp.get("result", {})
            peer_decisions = r.get("peer_decisions", [])
            peer_a_entry = next((p for p in peer_decisions if p.get("peer_id") == "peer-a"), None)
            peer_a_action = peer_a_entry.get("action") if peer_a_entry else None
            ok = (
                r.get("kind") in ("file", "type_conflict_file_wins")
                and r.get("winning_source_peer_id") == "peer-b"
                and peer_a_action == "copy_from_winner"
            )
            print(f"[03.15] absent_unconfirmed reconciled against max mod_time (Rule 4b): {'PASS' if ok else 'FAIL'} — "
                  f"winner={r.get('winning_source_peer_id')!r} peer-a action={peer_a_action!r}")
            if not ok:
                failures.append(
                    f"03.15: want winner=peer-b and peer-a action=copy_from_winner, got {r!r}"
                )

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
