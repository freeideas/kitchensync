#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercise snapshot update directives (02.25–02.32) via the MCP wrapper."""

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


def _decide(sock, per_peer_inputs, now=5000, tolerance=5, entry_name="file.txt", rpc_id=1):
    return _rpc(sock, "tools/call", {
        "name": "decide",
        "arguments": {
            "entry_name": entry_name,
            "now": now,
            "timestamp_tolerance_seconds": tolerance,
            "per_peer_inputs": per_peer_inputs,
        },
    }, rpc_id=rpc_id)


def _parse_decision(resp):
    text = (resp.get("result") or {}).get("content", [{}])[0].get("text", "{}")
    return json.loads(text)


def _directive(decision, peer_id):
    for entry in decision.get("per_peer", []):
        if entry.get("peer_id") == peer_id:
            return entry.get("snapshot_directive")
    return None


def _live_file(mod_time, byte_size):
    return {"type": "live_file", "mod_time": mod_time, "byte_size": byte_size}


def _row(mod_time, byte_size, last_seen=None, deleted_time=None):
    return {"mod_time": mod_time, "byte_size": byte_size,
            "last_seen": last_seen, "deleted_time": deleted_time}


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []

            # 02.25 — every per-peer entry in the decision carries one snapshot directive
            # Scenario: peer_a has a new live file; peer_b is absent with no snapshot row.
            r25 = _decide(s, [
                {"peer_id": "pa", "role": "contributing",
                 "listing_state": _live_file(1000, 100), "snapshot_row": None},
                {"peer_id": "pb", "role": "contributing",
                 "listing_state": {"type": "absent"}, "snapshot_row": None},
            ], now=5000, rpc_id=1)
            d25 = _parse_decision(r25)
            pp25 = d25.get("per_peer", [])
            all_have = all("snapshot_directive" in e for e in pp25)
            print(f"[02.25] {len(pp25)} per-peer entries, all have snapshot_directive={all_have}")
            if len(pp25) != 2 or not all_have:
                failures.append("02.25: not every per-peer entry carries a snapshot_directive")

            # 02.26 — each directive type is one of the five allowed values
            VALID = {"upsert_present", "upsert_decided_target", "mark_tombstone",
                     "clear_tombstone", "no_change"}
            types25 = [e.get("snapshot_directive", {}).get("type") for e in pp25]
            bad = [t for t in types25 if t not in VALID]
            print(f"[02.26] directive types: {types25}")
            if bad:
                failures.append(f"02.26: invalid directive type(s): {bad}")

            # 02.27 — upsert_present carries mod_time, byte_size, set_last_seen
            # 02.28 — upsert_decided_target carries mod_time, byte_size (last_seen unchanged)
            # 02.30 — peer whose listing confirms snapshot live gets upsert_present set_last_seen=true
            # Scenario: peer_a has a live file matching its snapshot (unchanged); peer_b is absent
            # with no row and will receive the copy.
            r_multi = _decide(s, [
                {"peer_id": "pa", "role": "contributing",
                 "listing_state": _live_file(2000, 200),
                 "snapshot_row": _row(2000, 200, last_seen=1800)},
                {"peer_id": "pb", "role": "contributing",
                 "listing_state": {"type": "absent"}, "snapshot_row": None},
            ], now=6000, rpc_id=2)
            d_multi = _parse_decision(r_multi)
            dir_pa = _directive(d_multi, "pa")
            dir_pb = _directive(d_multi, "pb")

            print(f"[02.27] upsert_present directive for live-matched peer: {dir_pa}")
            if dir_pa is None or dir_pa.get("type") != "upsert_present":
                failures.append(f"02.27: expected upsert_present, got {dir_pa}")
            elif not all(k in dir_pa for k in ("mod_time", "byte_size", "set_last_seen")):
                failures.append(f"02.27: upsert_present missing required field(s): {dir_pa}")

            print(f"[02.28] upsert_decided_target directive for copy-receiver: {dir_pb}")
            if dir_pb is None or dir_pb.get("type") != "upsert_decided_target":
                failures.append(f"02.28: expected upsert_decided_target, got {dir_pb}")
            elif not all(k in dir_pb for k in ("mod_time", "byte_size")):
                failures.append(f"02.28: upsert_decided_target missing required field(s): {dir_pb}")

            print(f"[02.30] set_last_seen for live-confirmed peer: {dir_pa}")
            if dir_pa is not None and dir_pa.get("type") == "upsert_present":
                if dir_pa.get("set_last_seen") is not True:
                    failures.append(f"02.30: set_last_seen not true for live-confirmed peer: {dir_pa}")
            else:
                failures.append(f"02.30: pre-condition failed (peer_a was not upsert_present): {dir_pa}")

            # 02.29 — mark_tombstone carries deleted_time equal to configured now
            # 02.31 — peer whose listing is absent and snapshot row is live receives mark_tombstone
            # Scenario: both peers report absent; both have live snapshot rows → deletion wins.
            NOW_DEL = 9999
            r_del = _decide(s, [
                {"peer_id": "pa", "role": "contributing",
                 "listing_state": {"type": "absent"},
                 "snapshot_row": _row(1000, 100, last_seen=800)},
                {"peer_id": "pb", "role": "contributing",
                 "listing_state": {"type": "absent"},
                 "snapshot_row": _row(1000, 100, last_seen=800)},
            ], now=NOW_DEL, rpc_id=3)
            d_del = _parse_decision(r_del)
            dir_pa_del = _directive(d_del, "pa")

            print(f"[02.29] mark_tombstone directive: {dir_pa_del}")
            if dir_pa_del is None or dir_pa_del.get("type") != "mark_tombstone":
                failures.append(f"02.29: expected mark_tombstone, got {dir_pa_del}")
            elif dir_pa_del.get("deleted_time") != NOW_DEL:
                failures.append(
                    f"02.29: deleted_time {dir_pa_del.get('deleted_time')} != now {NOW_DEL}")

            print(f"[02.31] absent+live-snapshot triggers mark_tombstone: {dir_pa_del}")
            if dir_pa_del is None or dir_pa_del.get("type") != "mark_tombstone":
                failures.append(f"02.31: absent listing + live snapshot row did not yield mark_tombstone,"
                                 f" got {dir_pa_del}")

            # 02.32 — peer whose listing is live but snapshot row is tombstoned receives clear_tombstone
            # Scenario: both peers have live files but tombstoned snapshot rows (resurrection).
            r_res = _decide(s, [
                {"peer_id": "pa", "role": "contributing",
                 "listing_state": _live_file(1000, 100),
                 "snapshot_row": _row(1000, 100, deleted_time=700)},
                {"peer_id": "pb", "role": "contributing",
                 "listing_state": _live_file(1000, 100),
                 "snapshot_row": _row(1000, 100, deleted_time=700)},
            ], now=7000, rpc_id=4)
            d_res = _parse_decision(r_res)
            dir_pa_res = _directive(d_res, "pa")

            print(f"[02.32] clear_tombstone directive for resurrected peer: {dir_pa_res}")
            if dir_pa_res is None or dir_pa_res.get("type") != "clear_tombstone":
                failures.append(f"02.32: expected clear_tombstone for live listing + tombstoned snapshot,"
                                 f" got {dir_pa_res}")

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
