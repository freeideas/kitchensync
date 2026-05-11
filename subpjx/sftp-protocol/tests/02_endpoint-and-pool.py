#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Endpoint handles and per-(user,host) connection pool sharing (02.1–02.3)."""

from __future__ import annotations

import json, os, socket, subprocess, sys, threading, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

HOST = "localhost"
USER = "ace"


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
    deadline = time.time() + 30
    while time.time() < deadline:
        chunk = sock.recv(8192)
        if not chunk:
            break
        buf += chunk
        if b"\n" in buf:
            break
    line, _, _ = buf.partition(b"\n")
    return json.loads(line.decode("utf-8"))


def _call(sock, name, args=None, rpc_id=1):
    return _rpc(sock, "tools/call", {"name": name, "arguments": args or {}}, rpc_id)


def _start_blackhole():
    """Accepts TCP connections but never sends data — simulates a hung SSH server."""
    srv = socket.socket()
    srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv.bind(("127.0.0.1", 0))
    srv.listen(5)
    bh_port = srv.getsockname()[1]
    held = []

    def _loop():
        srv.settimeout(30)
        try:
            while True:
                try:
                    conn, _ = srv.accept()
                    held.append(conn)
                except OSError:
                    break
        finally:
            for c in held:
                try:
                    c.close()
                except OSError:
                    pass

    threading.Thread(target=_loop, daemon=True).start()
    return srv, bh_port


def main() -> int:
    proc, port = _launch()
    blackhole_srv = None
    try:
        failures = []
        blackhole_srv, bh_port = _start_blackhole()

        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            rid = iter(range(1, 1000))

            tl = _rpc(s, "tools/list", rpc_id=next(rid))
            tools = {t["name"] for t in (tl.get("result") or {}).get("tools", [])}
            print(f"[info] tools: {sorted(tools)}")

            STD = {"mc": 2, "ct": 10, "ka": 60}

            # ── 02.1 ── open-endpoint returns an endpoint handle ─────────────────
            print("[02.1] open-endpoint returns an endpoint handle")
            ep1_resp = _call(s, "open-endpoint", {
                "user": USER, "host": HOST, "port": 22, "settings": STD,
            }, next(rid))
            ep1 = (ep1_resp.get("result") or {}).get("endpoint")
            print(f"[02.1] result={ep1_resp.get('result')}  error={ep1_resp.get('error')}")
            if ep1:
                print(f"[02.1] PASS: endpoint handle = {ep1!r}")
            else:
                failures.append(
                    f"02.1: open-endpoint did not return an endpoint handle; resp={ep1_resp}"
                )
                print("[02.1] FAIL")

            # ── 02.2 ── port=22 vs default port → same pool (idle-reuse proof) ───
            # Acquire a real SSH connection via the explicit-port=22 handle, release it
            # into the pool's idle set, then open a new handle for the same (user, host)
            # without supplying a port (default). Acquiring from that handle must return
            # the idle connection — proving both handles are backed by the same pool.
            print("[02.2] port=22 vs default port → same pool")
            if ep1 is None:
                failures.append("02.2: skipped — ep1 unavailable from 02.1")
                print("[02.2] SKIP")
            else:
                acq1 = _call(s, "acquire", {"endpoint": ep1}, next(rid))
                conn_a = (acq1.get("result") or {}).get("connection")
                print(f"[02.2] acquire via port=22 handle → conn_a={conn_a!r}")
                if conn_a is None:
                    failures.append(f"02.2: acquire via port=22 handle failed: {acq1}")
                    print("[02.2] FAIL: acquire returned no connection")
                else:
                    _call(s, "release", {"connection": conn_a}, next(rid))
                    # Open same (user, host) with the default port (no port field)
                    ep2_resp = _call(s, "open-endpoint", {
                        "user": USER, "host": HOST, "settings": STD,
                    }, next(rid))
                    ep2 = (ep2_resp.get("result") or {}).get("endpoint")
                    if ep2 is None:
                        failures.append(
                            f"02.2: open-endpoint with default port returned no handle: {ep2_resp}"
                        )
                        print("[02.2] FAIL: default-port open-endpoint failed")
                    else:
                        acq2 = _call(s, "acquire", {"endpoint": ep2}, next(rid))
                        conn_b = (acq2.get("result") or {}).get("connection")
                        print(f"[02.2] acquire via default-port handle → conn_b={conn_b!r}")
                        if conn_b is None:
                            failures.append(
                                f"02.2: acquire via default-port handle failed: {acq2}"
                            )
                            print("[02.2] FAIL: acquire returned no connection")
                        elif conn_b == conn_a:
                            print("[02.2] PASS: idle conn reused → same pool confirmed")
                        else:
                            failures.append(
                                f"02.2: expected idle conn {conn_a!r} reused via default-port "
                                f"handle, got {conn_b!r}; pools appear separate"
                            )
                            print("[02.2] FAIL: different connection returned — pools not shared")
                        if conn_b is not None:
                            _call(s, "release", {"connection": conn_b}, next(rid))

            # ── 02.3 ── different host → distinct pools (capacity proof) ─────────
            # Occupy mc=1 on (ace@localhost). Concurrently attempt acquire on
            # (ace@127.0.0.1) — a distinct host string — pointed at a TCP blackhole
            # with ct=2 s. If the pools are independent, the EB acquire fails/times-out
            # on its own within ~2 s. If the pools share capacity (wrong), EB blocks
            # behind EA's hold and only unblocks after it is released.
            print("[02.3] different host → distinct pools")
            ea_resp = _call(s, "open-endpoint", {
                "user": USER, "host": HOST, "port": 22,
                "settings": {"mc": 1, "ct": 5, "ka": 60},
            }, next(rid))
            ep_a = (ea_resp.get("result") or {}).get("endpoint")

            # 127.0.0.1 differs from "localhost" as a pool key; the blackhole port
            # ensures the SSH handshake hangs until the ct=2 timeout fires.
            eb_resp = _call(s, "open-endpoint", {
                "user": USER, "host": "127.0.0.1", "port": bh_port,
                "settings": {"mc": 1, "ct": 2, "ka": 60},
            }, next(rid))
            ep_b = (eb_resp.get("result") or {}).get("endpoint")

            if ep_a is None or ep_b is None:
                failures.append(
                    f"02.3: could not open both endpoints; ep_a={ep_a!r} ep_b={ep_b!r}"
                )
                print("[02.3] FAIL: open-endpoint failed")
            else:
                hold_resp = _call(s, "acquire", {"endpoint": ep_a}, next(rid))
                conn_hold = (hold_resp.get("result") or {}).get("connection")
                print(f"[02.3] hold acquire from ea → {conn_hold!r} (mc=1 occupied)")
                if conn_hold is None:
                    failures.append(f"02.3: acquire from ep_a failed: {hold_resp}")
                    print("[02.3] FAIL: could not occupy EA's mc=1")
                else:
                    eb_done = threading.Event()
                    eb_result = [None]
                    mcp_port = port

                    def _acquire_eb():
                        try:
                            with socket.create_connection(
                                ("127.0.0.1", mcp_port), timeout=15
                            ) as s2:
                                r = _call(s2, "acquire", {"endpoint": ep_b}, rpc_id=1)
                                eb_result[0] = r
                        except Exception as exc:
                            eb_result[0] = {"_exception": str(exc)}
                        finally:
                            eb_done.set()

                    t = threading.Thread(target=_acquire_eb, daemon=True)
                    t.start()

                    # ct=2: EB should complete (fail/timeout against blackhole) within ~3 s
                    # if its pool is distinct. Give 5 s of generous margin.
                    completed = eb_done.wait(timeout=5)

                    _call(s, "release", {"connection": conn_hold}, next(rid))

                    if completed:
                        print(
                            f"[02.3] PASS: EB acquire completed before hold released "
                            f"(result={eb_result[0]}) — distinct pool confirmed"
                        )
                    else:
                        eb_done.wait(timeout=5)
                        if eb_done.is_set():
                            failures.append(
                                "02.3: EB acquire only unblocked after EA's hold was released — "
                                "pools share mc=1 capacity; not distinct"
                            )
                            print("[02.3] FAIL: EB was blocked by EA's mc=1 — shared pool")
                        else:
                            failures.append("02.3: EB acquire never completed within timeout")
                            print("[02.3] FAIL: EB acquire timed out entirely")

        if failures:
            print("\nFAILURES:")
            for f in failures:
                print(f"  - {f}")
            return 1
        print("\nAll assertions passed.")
        return 0
    finally:
        if blackhole_srv is not None:
            try:
                blackhole_srv.close()
            except OSError:
                pass
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()


if __name__ == "__main__":
    sys.exit(main())
