#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Per-key concurrency cap and FIFO blocking (02.1–02.4)."""

from __future__ import annotations

import json, os, socket, subprocess, sys, threading, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

_id_lock = threading.Lock()
_id_seq = 0


def _next_id():
    global _id_seq
    with _id_lock:
        _id_seq += 1
        return _id_seq


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


def _rpc(sock, method, params=None, timeout=10):
    rpc_id = _next_id()
    msg = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
    if params is not None:
        msg["params"] = params
    sock.sendall((json.dumps(msg) + "\n").encode("utf-8"))
    buf = b""
    deadline = time.time() + timeout
    while time.time() < deadline:
        sock.settimeout(max(0.05, deadline - time.time()))
        try:
            chunk = sock.recv(8192)
        except (socket.timeout, OSError):
            break
        if not chunk:
            break
        buf += chunk
        if b"\n" in buf:
            break
    if b"\n" not in buf:
        return None
    line, _, _ = buf.partition(b"\n")
    return json.loads(line.decode("utf-8"))


def _call(sock, tool, args, timeout=10):
    return _rpc(sock, "tools/call", {"name": tool, "arguments": args}, timeout=timeout)


def _hid(result):
    r = (result or {}).get("result", {})
    return r.get("handleId") or r.get("handleId") or r.get("id")


def _pid(result):
    r = (result or {}).get("result", {})
    return r.get("poolId") or r.get("poolId") or r.get("id")


def _wait_len(lst, lock, n, timeout=5):
    deadline = time.time() + timeout
    while time.time() < deadline:
        with lock:
            if len(lst) >= n:
                return True
        time.sleep(0.05)
    return False


def main() -> int:
    proc, port = _launch()
    failures = []
    extra_socks = []

    try:
        s = socket.create_connection(("127.0.0.1", port), timeout=10)
        extra_socks.append(s)

        # Create pool with maxPerKey=2 for 02.1
        r = _call(s, "create-pool", {"maxPerKey": 2, "idleTtlSeconds": 60})
        if not r or "error" in r:
            print("FATAL: create-pool failed; cannot run assertions")
            return 1
        pool2 = _pid(r)

        # Create pool with maxPerKey=1 for 02.2–02.4
        r = _call(s, "create-pool", {"maxPerKey": 1, "idleTtlSeconds": 60})
        if not r or "error" in r:
            print("FATAL: create-pool (cap=1) failed; cannot run assertions")
            return 1
        pool1 = _pid(r)

        # ── 02.1: acquire below cap returns without blocking ─────────────────
        r1 = _call(s, "acquire", {"poolId": pool2, "key": "k1"}, timeout=5)
        r2 = _call(s, "acquire", {"poolId": pool2, "key": "k1"}, timeout=5)
        if not r1 or "error" in r1:
            failures.append("02.1: first acquire (1 of 2 slots) did not return within 5 s")
        elif not r2 or "error" in r2:
            failures.append("02.1: second acquire (2 of 2 slots) did not return within 5 s")
        else:
            print("[02.1] acquire below cap returns without blocking: PASS")
        if r1 and "result" in r1:
            _call(s, "release", {"handleId": _hid(r1)}, timeout=5)
        if r2 and "result" in r2:
            _call(s, "release", {"handleId": _hid(r2)}, timeout=5)

        # ── 02.2: acquire blocks when cap is full and all slots are held ──────
        ra = _call(s, "acquire", {"poolId": pool1, "key": "k2"}, timeout=5)
        if not ra or "error" in ra:
            failures.append("02.2: initial acquire (to fill cap) failed")
        else:
            s2 = socket.create_connection(("127.0.0.1", port), timeout=10)
            extra_socks.append(s2)
            done_22 = threading.Event()
            result_22 = [None]

            def blocker_22():
                result_22[0] = _call(s2, "acquire", {"poolId": pool1, "key": "k2"}, timeout=15)
                done_22.set()

            threading.Thread(target=blocker_22, daemon=True).start()
            time.sleep(0.5)
            if done_22.is_set():
                failures.append("02.2: acquire returned immediately when cap was full and all slots held")
            else:
                print("[02.2] acquire blocks when cap reached and all resources held: PASS")

            # ── 02.3 (release path): blocked acquirer proceeds after release ──
            _call(s, "release", {"handleId": _hid(ra)}, timeout=5)
            done_22.wait(timeout=5)
            if not done_22.is_set():
                failures.append("02.3: blocked acquirer did not proceed within 5 s of release")
            elif not result_22[0] or "error" in result_22[0]:
                failures.append("02.3: blocked acquirer received error instead of resource after release")
            else:
                print("[02.3] blocked acquirer proceeds and receives a resource after release: PASS")
            if result_22[0] and "result" in result_22[0]:
                _call(s, "release", {"handleId": _hid(result_22[0])}, timeout=5)

        # ── 02.3 (discard path): blocked acquirer also proceeds after discard ─
        rd = _call(s, "acquire", {"poolId": pool1, "key": "k3"}, timeout=5)
        if not rd or "error" in rd:
            failures.append("02.3(discard): initial acquire failed")
        else:
            s3 = socket.create_connection(("127.0.0.1", port), timeout=10)
            extra_socks.append(s3)
            done_23d = threading.Event()
            result_23d = [None]

            def blocker_23d():
                result_23d[0] = _call(s3, "acquire", {"poolId": pool1, "key": "k3"}, timeout=15)
                done_23d.set()

            threading.Thread(target=blocker_23d, daemon=True).start()
            time.sleep(0.3)
            _call(s, "discard", {"handleId": _hid(rd)}, timeout=5)
            done_23d.wait(timeout=5)
            if not done_23d.is_set():
                failures.append("02.3(discard): blocked acquirer did not proceed within 5 s of discard")
            elif not result_23d[0] or "error" in result_23d[0]:
                failures.append("02.3(discard): blocked acquirer received error instead of resource after discard")
            else:
                print("[02.3] blocked acquirer proceeds and receives a resource after discard: PASS")
            if result_23d[0] and "result" in result_23d[0]:
                _call(s, "release", {"handleId": _hid(result_23d[0])}, timeout=5)

        # ── 02.4: multiple blocked acquirers served in FIFO arrival order ─────
        rf = _call(s, "acquire", {"poolId": pool1, "key": "k4"}, timeout=5)
        if not rf or "error" in rf:
            failures.append("02.4: initial acquire (to fill cap) failed")
        else:
            order = []
            order_lock = threading.Lock()
            fifo_results = {}

            fifo_socks = []
            for _ in range(3):
                si = socket.create_connection(("127.0.0.1", port), timeout=10)
                fifo_socks.append(si)
                extra_socks.append(si)

            def make_acquirer(idx, sock):
                def fn():
                    r = _call(sock, "acquire", {"poolId": pool1, "key": "k4"}, timeout=20)
                    with order_lock:
                        order.append(idx)
                        fifo_results[idx] = r
                return fn

            for i, si in enumerate(fifo_socks):
                threading.Thread(target=make_acquirer(i, si), daemon=True).start()
                time.sleep(0.25)  # stagger to ensure FIFO arrival order at server

            # Release one slot at a time and collect the order acquirers complete.
            _call(s, "release", {"handleId": _hid(rf)}, timeout=5)
            if not _wait_len(order, order_lock, 1):
                failures.append("02.4: no blocked acquirer woke up after first release")
            else:
                r0 = fifo_results.get(order[0])
                if r0 and "result" in r0:
                    _call(s, "release", {"handleId": _hid(r0)}, timeout=5)
                if not _wait_len(order, order_lock, 2):
                    failures.append("02.4: second blocked acquirer did not wake up after second release")
                else:
                    r1 = fifo_results.get(order[1])
                    if r1 and "result" in r1:
                        _call(s, "release", {"handleId": _hid(r1)}, timeout=5)
                    if not _wait_len(order, order_lock, 3):
                        failures.append("02.4: third blocked acquirer did not wake up after third release")
                    else:
                        r2 = fifo_results.get(order[2])
                        if r2 and "result" in r2:
                            _call(s, "release", {"handleId": _hid(r2)}, timeout=5)

            with order_lock:
                final_order = list(order)
            if len(final_order) == 3 and final_order != [0, 1, 2]:
                failures.append(f"02.4: FIFO order expected [0, 1, 2], got {final_order}")
            elif len(final_order) == 3:
                print("[02.4] Multiple blocked acquirers served in FIFO arrival order: PASS")

        if failures:
            print("\nFAILURES:")
            for f in failures:
                print(f"  - {f}")
            return 1
        print("\nAll assertions passed.")
        return 0

    finally:
        for sx in extra_socks:
            try:
                sx.close()
            except Exception:
                pass
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()


if __name__ == "__main__":
    sys.exit(main())
