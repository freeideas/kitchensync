#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Tests timestamp format, UTC bounds, lexicographic monotonicity, and microsecond increment (reqs 02.6–02.9)."""

from __future__ import annotations

import json, os, re, socket, subprocess, sys, tempfile, threading, time
from datetime import datetime, timezone
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

TIMESTAMP_PAT = re.compile(r'^\d{4}-\d{2}-\d{2}_\d{2}-\d{2}-\d{2}_\d{6}Z$')


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


def _call(sock, tool, args, rpc_id=1):
    return _rpc(sock, "tools/call", {"name": tool, "arguments": args}, rpc_id=rpc_id)


def _parse_ts(ts: str) -> datetime:
    return datetime.strptime(ts, "%Y-%m-%d_%H-%M-%S_%fZ").replace(tzinfo=timezone.utc)


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []
            rid = 1

            with tempfile.TemporaryDirectory() as tmpdir:
                db_path = str(Path(tmpdir) / "timestamps_test.db")

                open_resp = _call(s, "open-snapshot", {"path": db_path}, rpc_id=rid)
                rid += 1
                if "error" in open_resp:
                    print(f"[setup] open-snapshot failed: {open_resp['error']}")
                    failures.append(f"setup: open-snapshot failed: {open_resp['error']}")
                    print("\nFAILURES:")
                    for f in failures:
                        print(f"  - {f}")
                    return 1

                handle = (open_resp.get("result") or {}).get("handle")
                if handle is None:
                    print("[setup] open-snapshot returned no handle")
                    failures.append("setup: open-snapshot returned no handle")
                    print("\nFAILURES:")
                    for f in failures:
                        print(f"  - {f}")
                    return 1

                # --- 02.6: format matches YYYY-MM-DD_HH-mm-ss_ffffffZ ---
                r = _call(s, "current-timestamp", {"handle": handle}, rpc_id=rid)
                rid += 1
                if "error" in r:
                    print(f"[02.6] current-timestamp error: {r['error']}")
                    failures.append(f"02.6: current-timestamp error: {r['error']}")
                else:
                    ts = (r.get("result") or {}).get("timestamp", "")
                    if TIMESTAMP_PAT.match(ts):
                        print(f"[02.6] timestamp matches YYYY-MM-DD_HH-mm-ss_ffffffZ: {ts}")
                    else:
                        print(f"[02.6] FAIL — timestamp does not match pattern: {ts!r}")
                        failures.append(f"02.6: {ts!r} does not match YYYY-MM-DD_HH-mm-ss_ffffffZ")

                # --- 02.7: parsed UTC value falls between system clock before and after the call ---
                before_utc = datetime.now(timezone.utc)
                r = _call(s, "current-timestamp", {"handle": handle}, rpc_id=rid)
                rid += 1
                after_utc = datetime.now(timezone.utc)
                if "error" in r:
                    print(f"[02.7] current-timestamp error: {r['error']}")
                    failures.append(f"02.7: current-timestamp error: {r['error']}")
                else:
                    ts = (r.get("result") or {}).get("timestamp", "")
                    try:
                        ts_dt = _parse_ts(ts)
                        if before_utc <= ts_dt <= after_utc:
                            print(f"[02.7] timestamp {ts} within UTC bounds [{before_utc.isoformat()}, {after_utc.isoformat()}]")
                        else:
                            print(f"[02.7] FAIL — timestamp {ts} outside UTC bounds")
                            failures.append(
                                f"02.7: {ts} not in [{before_utc.isoformat()}, {after_utc.isoformat()}]"
                            )
                    except ValueError as e:
                        print(f"[02.7] FAIL — could not parse timestamp {ts!r}: {e}")
                        failures.append(f"02.7: parse error on {ts!r}: {e}")

                # --- 02.8: two timestamps A then B satisfy A < B under lexicographic comparison ---
                r_a = _call(s, "current-timestamp", {"handle": handle}, rpc_id=rid)
                rid += 1
                r_b = _call(s, "current-timestamp", {"handle": handle}, rpc_id=rid)
                rid += 1
                if "error" in r_a or "error" in r_b:
                    print(f"[02.8] FAIL — current-timestamp error fetching A or B")
                    failures.append("02.8: current-timestamp error during A/B fetch")
                else:
                    ts_a = (r_a.get("result") or {}).get("timestamp", "")
                    ts_b = (r_b.get("result") or {}).get("timestamp", "")
                    if ts_a < ts_b:
                        print(f"[02.8] A={ts_a} < B={ts_b} (lexicographic)")
                    else:
                        print(f"[02.8] FAIL — A={ts_a!r} is not < B={ts_b!r}")
                        failures.append(f"02.8: A={ts_a!r} not < B={ts_b!r}")

                # --- 02.9: consecutive timestamps always differ by at least 1μs ---
                # The spec guarantees that if the wall clock hasn't advanced between two
                # requests, the second is exactly +1μs after the first.  That makes 1μs
                # the minimum observable difference for any consecutive pair.
                seq = []
                seq_error = False
                for _ in range(10):
                    r = _call(s, "current-timestamp", {"handle": handle}, rpc_id=rid)
                    rid += 1
                    if "error" in r:
                        print(f"[02.9] FAIL — current-timestamp error in sequence: {r['error']}")
                        failures.append(f"02.9: current-timestamp error in sequence: {r['error']}")
                        seq_error = True
                        break
                    seq.append((r.get("result") or {}).get("timestamp", ""))

                if not seq_error and len(seq) >= 2:
                    pair_failures = []
                    for i in range(len(seq) - 1):
                        ta, tb = seq[i], seq[i + 1]
                        try:
                            diff_us = int(
                                (_parse_ts(tb) - _parse_ts(ta)).total_seconds() * 1_000_000
                            )
                            if diff_us < 1:
                                pair_failures.append(
                                    f"pair [{i},{i+1}] diff={diff_us}μs (< 1μs): {ta!r} → {tb!r}"
                                )
                        except ValueError as e:
                            pair_failures.append(f"pair [{i},{i+1}] parse error: {e}")
                    if pair_failures:
                        for pf in pair_failures:
                            print(f"[02.9] FAIL — {pf}")
                            failures.append(f"02.9: {pf}")
                    else:
                        print(f"[02.9] {len(seq)} consecutive timestamps each differ by >= 1μs")

                _call(s, "close-snapshot", {"handle": handle}, rpc_id=rid)
                rid += 1

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
