#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Lazy per-key pool creation with idempotent registration (01.1–01.4)."""

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
    """Thread-safe JSON-RPC 2.0 client over a single TCP socket."""

    def __init__(self, sock):
        self._sock = sock
        self._buf = b""
        self._id = 0
        self._lock = threading.Lock()

    def rpc(self, method, params=None):
        with self._lock:
            self._id += 1
            msg = {"jsonrpc": "2.0", "id": self._id, "method": method}
            if params is not None:
                msg["params"] = params
            self._sock.sendall((json.dumps(msg) + "\n").encode())
            deadline = time.time() + 15
            while b"\n" not in self._buf and time.time() < deadline:
                chunk = self._sock.recv(65536)
                if not chunk:
                    break
                self._buf += chunk
            line, _, self._buf = self._buf.partition(b"\n")
            return json.loads(line.decode())

    def call(self, tool, args):
        return self.rpc("tools/call", {"name": tool, "arguments": args})


def main() -> int:
    proc, port = _launch()
    try:
        failures = []

        with socket.create_connection(("127.0.0.1", port), timeout=15) as sock:
            s = _Conn(sock)

            tl = s.rpc("tools/list")
            tools = {t["name"] for t in (tl.get("result") or {}).get("tools", [])}
            print(f"[info] tools available: {sorted(tools)}")

            STD = {"mc": 2, "ct": 5, "ka": 30}

            # ── 01.1 ── same key → same pool: acquire from either handle draws from
            #            the same idle set ──────────────────────────────────────────
            print("[01.1] same key → same pool: shared idle set")
            r = s.call("register-pool", {"key": "k01.1", "settings": STD})
            pool1 = r.get("result", {}).get("pool") if "error" not in r else None
            r = s.call("register-pool", {"key": "k01.1", "settings": STD})
            pool2 = r.get("result", {}).get("pool") if "error" not in r else None

            if pool1 is None or pool2 is None:
                failures.append("01.1: register-pool failed")
            else:
                r = s.call("acquire", {"pool": pool1})
                if "error" in r:
                    failures.append(f"01.1: acquire from pool1 failed: {r['error']}")
                else:
                    conn_a = r["result"]["connection"]
                    print(f"[01.1] acquired {conn_a!r} from pool1, releasing")
                    s.call("release", {"pool": pool1, "connection": conn_a})
                    r = s.call("acquire", {"pool": pool2})
                    if "error" in r:
                        failures.append(f"01.1: acquire from pool2 failed: {r['error']}")
                    else:
                        conn_b = r["result"]["connection"]
                        if conn_b == conn_a:
                            print(f"[01.1] PASS: pool2 reused idle conn {conn_a!r} — same pool confirmed")
                        else:
                            failures.append(
                                f"01.1: pool2 returned {conn_b!r} instead of idle {conn_a!r}; "
                                "idle sets not shared"
                            )
                        s.call("release", {"pool": pool2, "connection": conn_b})

            # ── 01.2 ── re-registration does not replace original settings ─────────
            # Register with mc=1, then register same key with mc=99.
            # A second acquire should block (mc=1 retained), not return immediately.
            print("[01.2] re-registration does not replace original settings (mc)")
            r = s.call("register-pool", {"key": "k01.2", "settings": {"mc": 1, "ct": 5, "ka": 30}})
            pool_orig = r.get("result", {}).get("pool") if "error" not in r else None
            r = s.call("register-pool", {"key": "k01.2", "settings": {"mc": 99, "ct": 5, "ka": 30}})
            pool_later = r.get("result", {}).get("pool") if "error" not in r else None

            if pool_orig is None or pool_later is None:
                failures.append("01.2: register-pool failed")
            else:
                r = s.call("acquire", {"pool": pool_orig})
                if "error" in r:
                    failures.append(f"01.2: acquire from pool_orig failed: {r['error']}")
                else:
                    conn_orig = r["result"]["connection"]
                    # A second acquire via pool_later must block because mc=1 is the original
                    # setting that was retained; if mc had been replaced to 99 it would return
                    # immediately.  Use a separate TCP connection so the release on s can be
                    # processed while the acquire on s2 is blocked inside the server.
                    with socket.create_connection(("127.0.0.1", port), timeout=15) as sock2:
                        s2 = _Conn(sock2)
                        second_done = threading.Event()

                        def _try_second():
                            s2.call("acquire", {"pool": pool_later})
                            second_done.set()

                        t = threading.Thread(target=_try_second, daemon=True)
                        t.start()
                        # 1.5 s — if mc=99 had been applied the acquire would have
                        # returned well before this.
                        time.sleep(1.5)
                        if second_done.is_set():
                            failures.append(
                                "01.2: second acquire returned before first was released; "
                                "mc=99 from re-registration was incorrectly applied instead of original mc=1"
                            )
                        else:
                            print("[01.2] PASS: second acquire still blocking after 1.5 s — original mc=1 retained")
                        # Release the first connection to unblock the second acquire
                        s.call("release", {"pool": pool_orig, "connection": conn_orig})
                        second_done.wait(timeout=5)

            # ── 01.3 ── distinct keys → distinct pools ────────────────────────────
            # With mc=1 per pool, acquiring from both should succeed concurrently.
            # If the pools shared capacity (same pool), the second acquire would block.
            print("[01.3] distinct keys → distinct pools: independent capacity and idle sets")
            r1 = s.call("register-pool", {"key": "k01.3a", "settings": {"mc": 1, "ct": 5, "ka": 30}})
            r2 = s.call("register-pool", {"key": "k01.3b", "settings": {"mc": 1, "ct": 5, "ka": 30}})
            pool_3a = r1.get("result", {}).get("pool") if "error" not in r1 else None
            pool_3b = r2.get("result", {}).get("pool") if "error" not in r2 else None

            if pool_3a is None or pool_3b is None:
                failures.append("01.3: register-pool failed")
            else:
                ra = s.call("acquire", {"pool": pool_3a})
                rb = s.call("acquire", {"pool": pool_3b})
                if "error" in ra:
                    failures.append(f"01.3: acquire from pool_3a failed: {ra['error']}")
                elif "error" in rb:
                    failures.append(
                        f"01.3: acquire from pool_3b failed (would block if capacity were shared): {rb['error']}"
                    )
                else:
                    conn_3a = ra["result"]["connection"]
                    conn_3b = rb["result"]["connection"]
                    print(
                        f"[01.3] PASS: both mc=1 acquires succeeded ({conn_3a!r}, {conn_3b!r}) "
                        "— independent capacity confirmed"
                    )
                    # Also verify idle sets are separate: release 3a's conn back to pool_3a,
                    # then re-acquire from pool_3b — must return pool_3b's own idle conn,
                    # not pool_3a's.
                    s.call("release", {"pool": pool_3a, "connection": conn_3a})
                    s.call("release", {"pool": pool_3b, "connection": conn_3b})
                    rc = s.call("acquire", {"pool": pool_3b})
                    if "error" not in rc:
                        conn_3b_idle = rc["result"]["connection"]
                        if conn_3b_idle != conn_3a:
                            print(f"[01.3] PASS: pool_3b idle set is independent from pool_3a")
                        else:
                            failures.append(
                                f"01.3: pool_3b returned pool_3a's conn {conn_3a!r} from idle — idle sets not independent"
                            )
                        s.call("release", {"pool": pool_3b, "connection": conn_3b_idle})

            # ── 01.4 ── value-equal keys → same pool ──────────────────────────────
            # Use a structured JSON-object key so value equality (not identity) is exercised.
            print("[01.4] value-equal keys (JSON object) → same pool")
            key_val = {"host": "h01.4", "port": 22}
            r1 = s.call("register-pool", {"key": key_val, "settings": {"mc": 2, "ct": 5, "ka": 30}})
            r2 = s.call("register-pool", {"key": key_val, "settings": {"mc": 2, "ct": 5, "ka": 30}})
            pool_va = r1.get("result", {}).get("pool") if "error" not in r1 else None
            pool_vb = r2.get("result", {}).get("pool") if "error" not in r2 else None

            if pool_va is None or pool_vb is None:
                failures.append("01.4: register-pool with object key failed")
            else:
                r = s.call("acquire", {"pool": pool_va})
                if "error" in r:
                    failures.append(f"01.4: acquire from pool_va failed: {r['error']}")
                else:
                    conn_va = r["result"]["connection"]
                    s.call("release", {"pool": pool_va, "connection": conn_va})
                    r = s.call("acquire", {"pool": pool_vb})
                    if "error" in r:
                        failures.append(f"01.4: acquire from pool_vb failed: {r['error']}")
                    else:
                        conn_vb = r["result"]["connection"]
                        if conn_vb == conn_va:
                            print(
                                f"[01.4] PASS: value-equal object key resolves to same pool; "
                                f"idle conn {conn_va!r} reused via pool_vb"
                            )
                        else:
                            failures.append(
                                f"01.4: value-equal keys did not share idle set "
                                f"({conn_va!r} vs {conn_vb!r})"
                            )
                        s.call("release", {"pool": pool_vb, "connection": conn_vb})

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
