#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""mc caps concurrency; ct bounds each open; failed opens do not consume slots."""

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


class _Conn:
    def __init__(self, port: int, timeout: float = 30.0):
        self._sock = socket.create_connection(("127.0.0.1", port), timeout=timeout)
        self._sock.settimeout(timeout)
        self._buf = b""

    def rpc(self, method, params=None, rpc_id: int = 1):
        msg = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
        if params is not None:
            msg["params"] = params
        self._sock.sendall((json.dumps(msg) + "\n").encode())
        while b"\n" not in self._buf:
            chunk = self._sock.recv(65536)
            if not chunk:
                raise EOFError("server closed connection")
            self._buf += chunk
        line, _, self._buf = self._buf.partition(b"\n")
        return json.loads(line)

    def call(self, name, arguments, rpc_id: int):
        return self.rpc("tools/call", {"name": name, "arguments": arguments}, rpc_id)

    def close(self):
        try:
            self._sock.close()
        except Exception:
            pass


def main() -> int:
    proc, port = _launch()
    open_conns: list[_Conn] = []
    try:
        failures = []
        rid = 1

        c = _Conn(port)
        open_conns.append(c)

        # ── 03.1 / 03.2: mc=1 blocks a second acquire; release unblocks it ───────

        reg = c.call("register-pool", {"key": "test-03-mc1", "mc": 1, "ct": 10, "ka": 60}, rid)
        rid += 1
        pool_mc1 = (reg.get("result") or {}).get("pool")
        print(f"[setup] register-pool mc=1 → pool={pool_mc1!r}")
        if not pool_mc1:
            failures.append("setup: register-pool (mc=1) returned no pool handle")
            print("\nFAILURES:")
            for f in failures:
                print(f"  - {f}")
            return 1

        # First acquire must succeed (pool empty, below mc=1).
        acq1 = c.call("acquire", {"pool": pool_mc1}, rid); rid += 1
        conn1 = (acq1.get("result") or {}).get("connection")
        print(f"[setup] first acquire → connection={conn1!r}")
        if not conn1:
            failures.append("setup: first acquire on empty pool failed")
            print("\nFAILURES:")
            for f in failures:
                print(f"  - {f}")
            return 1

        # Second acquire on a separate TCP connection must block (mc=1 is full).
        c2 = _Conn(port, timeout=15)
        open_conns.append(c2)
        acq2: dict = {"resp": None}
        acq2_ready = threading.Event()

        def _do_acquire():
            try:
                acq2["resp"] = c2.call("acquire", {"pool": pool_mc1}, 99)
            except Exception:
                pass  # acq2["resp"] stays None; acq2_ready still set below
            finally:
                acq2_ready.set()

        t2 = threading.Thread(target=_do_acquire, daemon=True)
        t2.start()
        time.sleep(0.5)  # let the second acquire reach the server

        # 03.1: second acquire has not returned (pool full).
        if acq2_ready.is_set():
            failures.append("03.1: second acquire returned before any release (mc=1 should block)")
            print("[03.1] FAIL: second acquire returned immediately — did not block")
        else:
            print("[03.1] PASS: second acquire is blocked while mc=1 pool is fully occupied")

        # Release the first connection; this must unblock the second acquire.
        c.call("release", {"pool": pool_mc1, "connection": conn1}, rid); rid += 1
        returned = acq2_ready.wait(timeout=5)

        # 03.2: blocked acquire proceeds and returns a connection after release.
        if not returned:
            failures.append("03.2: blocked acquire did not return within 5 s after release")
            print("[03.2] FAIL: blocked acquire did not unblock after release")
        elif acq2["resp"] is None:
            failures.append("03.2: blocked acquire thread raised an exception instead of returning")
            print("[03.2] FAIL: acquire thread threw; no response received")
        else:
            resp2 = acq2["resp"]
            err2 = resp2.get("error")
            conn2 = (resp2.get("result") or {}).get("connection")
            if err2:
                failures.append(f"03.2: blocked acquire returned error after release: {err2}")
                print(f"[03.2] FAIL: acquire error: {err2}")
            elif conn2:
                print(f"[03.2] PASS: blocked acquire proceeded and returned connection={conn2!r}")
                c.call("release", {"pool": pool_mc1, "connection": conn2}, rid); rid += 1
            else:
                failures.append("03.2: blocked acquire returned no connection after release")
                print("[03.2] FAIL: acquire response missing connection field")

        # ── 03.3 / 03.4: open exceeding ct treated as failed; error surfaced ─────

        # open_delay_ms=3000 > ct=1 s forces the open to time out.
        reg_ct = c.call("register-pool", {
            "key": "test-03-ct", "mc": 2, "ct": 1, "ka": 60,
            "open_delay_ms": 3000,
        }, rid); rid += 1
        pool_ct = (reg_ct.get("result") or {}).get("pool")
        print(f"[setup] register-pool ct=1 open_delay_ms=3000 → pool={pool_ct!r}")

        # acquire blocks until open times out (~1 s), then returns an error.
        acq_ct = c.call("acquire", {"pool": pool_ct}, rid); rid += 1
        ct_err = acq_ct.get("error")
        ct_conn = (acq_ct.get("result") or {}).get("connection")

        # 03.3: open exceeding ct is treated as a failed open.
        if ct_err or not ct_conn:
            print(f"[03.3] PASS: open exceeding ct treated as failed open (error={ct_err!r})")
        else:
            failures.append("03.3: acquire succeeded despite open exceeding ct seconds")
            print("[03.3] FAIL: acquire returned a connection — open should have timed out")

        # 03.4: acquire surfaces the failure to its caller.
        if ct_err:
            print(f"[03.4] PASS: acquire surfaced open failure to caller (message={ct_err.get('message')!r})")
        else:
            failures.append("03.4: acquire did not surface open failure; no error in response")
            print("[03.4] FAIL: no error in acquire response when open should have failed")

        # ── 03.5: after a failed open, a subsequent acquire can still proceed ─────

        # open_fail_count=1: first open fails; the next open succeeds.
        # The failed open must not consume a slot, so the subsequent acquire can proceed.
        reg_f = c.call("register-pool", {
            "key": "test-03-fail-then-ok", "mc": 1, "ct": 10, "ka": 60,
            "open_fail_count": 1,
        }, rid); rid += 1
        pool_f = (reg_f.get("result") or {}).get("pool")
        print(f"[setup] register-pool open_fail_count=1 → pool={pool_f!r}")

        # First acquire: open fails; slot must NOT be consumed.
        acq_f1 = c.call("acquire", {"pool": pool_f}, rid); rid += 1
        f1_err = acq_f1.get("error")
        if not f1_err:
            failures.append("03.5: setup: first acquire did not fail despite open_fail_count=1")
            print("[03.5] FAIL: first acquire unexpectedly succeeded (open_fail_count=1 had no effect)")
        else:
            print(f"[03.5] first acquire failed as expected (error={f1_err.get('message')!r})")

            # Second acquire: open succeeds; slot is still available (not consumed by failure).
            acq_f2 = c.call("acquire", {"pool": pool_f}, rid); rid += 1
            f2_err = acq_f2.get("error")
            conn_f2 = (acq_f2.get("result") or {}).get("connection")
            if f2_err or not conn_f2:
                failures.append(
                    f"03.5: subsequent acquire after failed open did not succeed "
                    f"(error={f2_err!r}, connection={conn_f2!r})"
                )
                print(f"[03.5] FAIL: subsequent acquire failed — failed open may have wrongly consumed a slot")
            else:
                print(f"[03.5] PASS: subsequent acquire succeeded after failed open (connection={conn_f2!r})")
                c.call("release", {"pool": pool_f, "connection": conn_f2}, rid); rid += 1

        if failures:
            print("\nFAILURES:")
            for f in failures:
                print(f"  - {f}")
            return 1
        print("\nAll assertions passed.")
        return 0
    finally:
        for conn in open_conns:
            conn.close()
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()


if __name__ == "__main__":
    sys.exit(main())
