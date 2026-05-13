#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Tests 03_per-key-isolation: per-key cap independence and factory exception behavior."""

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


def _rpc(sock, method, params=None, rpc_id=1, deadline_s=10):
    msg = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
    if params is not None:
        msg["params"] = params
    sock.sendall((json.dumps(msg) + "\n").encode("utf-8"))
    buf = b""
    end = time.time() + deadline_s
    orig = sock.gettimeout()
    sock.settimeout(1.0)
    try:
        while time.time() < end:
            try:
                chunk = sock.recv(8192)
            except socket.timeout:
                continue
            if not chunk:
                break
            buf += chunk
            if b"\n" in buf:
                break
    finally:
        sock.settimeout(orig)
    if not buf:
        return None
    line, _, _ = buf.partition(b"\n")
    try:
        return json.loads(line.decode("utf-8"))
    except json.JSONDecodeError:
        return None


def _call(sock, tool, args, rid, deadline_s=10):
    return _rpc(sock, "tools/call", {"name": tool, "arguments": args},
                rpc_id=rid, deadline_s=deadline_s)


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []
            rid = 1

            tl = _rpc(s, "tools/list", rpc_id=rid); rid += 1
            tools = (tl.get("result") or {}).get("tools", [])
            tool_names = [t["name"] for t in tools]
            print(f"[info] tools: {tool_names}")

            def find(*kws):
                for name in tool_names:
                    if all(k in name for k in kws):
                        return name
                return None

            create_pool = find("create")   # "create-pool"
            acquire     = find("acquire")
            release     = find("release")
            shutdown    = find("shutdown")
            set_delay   = find("delay")    # "set-factory-delay"
            set_error   = find("error")    # "set-factory-error"

            for label, tool in [("create-pool", create_pool), ("acquire", acquire),
                                 ("release", release), ("shutdown", shutdown)]:
                if tool is None:
                    print(f"FATAL: no tool matching '{label}' in {tool_names}")
                    return 1

            # ── 03.1: max_per_key cap is per-key; key A at cap does not block key B ──

            r = _call(s, create_pool, {"maxPerKey": 1, "idleTtlSeconds": 60}, rid); rid += 1
            pid1 = (r.get("result") or {}).get("poolId") if r else None

            ra = _call(s, acquire, {"poolId": pid1, "key": "A"}, rid); rid += 1
            rb = _call(s, acquire, {"poolId": pid1, "key": "B"}, rid); rid += 1

            ok_a = ra is not None and "result" in ra and "error" not in ra
            ok_b = rb is not None and "result" in rb and "error" not in rb
            print(f"[03.1] acquire key-A: {'ok' if ok_a else 'FAIL'}  "
                  f"acquire key-B (A at cap): {'ok' if ok_b else 'FAIL'}")
            if not ok_a:
                failures.append(f"03.1: acquire key-A failed: {ra}")
            if not ok_b:
                failures.append("03.1: acquire key-B blocked/failed when key-A was at cap")

            _call(s, shutdown, {"poolId": pid1}, rid); rid += 1

            # ── 03.2: slow create for one key does not delay acquire for other keys ──

            if set_delay is None:
                print("[03.2] FAIL: no factory-delay tool found")
                failures.append("03.2: factory-delay tool not present")
            else:
                r = _call(s, create_pool, {"maxPerKey": 2, "idleTtlSeconds": 60}, rid); rid += 1
                pid2 = (r.get("result") or {}).get("poolId") if r else None

                _call(s, set_delay, {"poolId": pid2, "key": "slow", "delayMs": 2000}, rid); rid += 1

                times: dict = {}

                def timed_acquire(key: str, label: str) -> None:
                    t0 = time.time()
                    try:
                        with socket.create_connection(("127.0.0.1", port), timeout=10) as ts:
                            _rpc(ts, "tools/call",
                                 {"name": acquire,
                                  "arguments": {"poolId": pid2, "key": key}},
                                 rpc_id=1, deadline_s=8)
                    except Exception:
                        pass
                    times[label] = time.time() - t0

                t_slow = threading.Thread(target=timed_acquire, args=("slow", "slow"))
                t_fast = threading.Thread(target=timed_acquire, args=("fast", "fast"))
                t_slow.start()
                t_fast.start()
                t_fast.join(timeout=5)
                t_slow.join(timeout=10)

                slow_t = times.get("slow", 999.0)
                fast_t = times.get("fast", 999.0)
                print(f"[03.2] slow-key={slow_t:.3f}s  fast-key={fast_t:.3f}s  (factory delay=2000ms)")
                if fast_t > 0.8:
                    failures.append(
                        f"03.2: fast-key acquire took {fast_t:.3f}s — blocked by slow factory (expected < 0.8s)")
                if slow_t < 1.5:
                    failures.append(
                        f"03.2: slow-key acquire completed in {slow_t:.3f}s — factory delay not applied")

                _call(s, shutdown, {"poolId": pid2}, rid); rid += 1

            # ── 03.3: exception from create propagates to the calling acquirer ──

            if set_error is None:
                print("[03.3] FAIL: no factory-error tool found")
                failures.append("03.3: factory-error tool not present")
            else:
                r = _call(s, create_pool, {"maxPerKey": 2, "idleTtlSeconds": 60}, rid); rid += 1
                pid3 = (r.get("result") or {}).get("poolId") if r else None

                _call(s, set_error,
                      {"poolId": pid3, "key": "err", "error": "factory create failed"},
                      rid); rid += 1

                r_err = _call(s, acquire, {"poolId": pid3, "key": "err"}, rid); rid += 1
                is_err = r_err is not None and "error" in r_err
                print(f"[03.3] acquire with raising factory returned error: {is_err}  resp={r_err}")
                if not is_err:
                    failures.append(f"03.3: expected error propagation from factory exception, got: {r_err}")

                _call(s, shutdown, {"poolId": pid3}, rid); rid += 1

            # ── 03.4: failed create consumes no slot; subsequent acquire proceeds ──

            if set_error is None:
                print("[03.4] FAIL: no factory-error tool found")
                failures.append("03.4: factory-error tool not present")
            else:
                r = _call(s, create_pool, {"maxPerKey": 1, "idleTtlSeconds": 60}, rid); rid += 1
                pid4 = (r.get("result") or {}).get("poolId") if r else None

                _call(s, set_error,
                      {"poolId": pid4, "key": "k", "error": "one-shot factory error"},
                      rid); rid += 1

                # First acquire: factory raises — error propagated
                r1 = _call(s, acquire, {"poolId": pid4, "key": "k"}, rid); rid += 1
                first_is_err = r1 is not None and "error" in r1
                print(f"[03.4a] first acquire (factory raises): error={first_is_err}")
                if not first_is_err:
                    failures.append(f"03.4: factory error not raised on first acquire: {r1}")

                # Second acquire with a 3-second deadline on a fresh connection.
                # If the failed factory call consumed a slot (bug), this blocks and returns None.
                # If it did not consume a slot (correct), this returns promptly.
                with socket.create_connection(("127.0.0.1", port), timeout=10) as s2:
                    r2 = _rpc(s2, "tools/call",
                              {"name": acquire,
                               "arguments": {"poolId": pid4, "key": "k"}},
                              rpc_id=1, deadline_s=3)
                not_blocked = r2 is not None
                print(f"[03.4b] second acquire after factory error: "
                      f"{'returned promptly (slot not consumed)' if not_blocked else 'TIMED OUT (slot consumed)'}")
                if not not_blocked:
                    failures.append(
                        "03.4: second acquire timed out — slot was consumed by the failed factory call")

                _call(s, shutdown, {"poolId": pid4}, rid); rid += 1

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
