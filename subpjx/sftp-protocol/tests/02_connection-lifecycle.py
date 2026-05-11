#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercises connection-lifecycle requirements 02.10-02.17 against the sftp-protocol MCP wrapper."""

from __future__ import annotations

import json, os, socket, subprocess, sys, threading, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

SFTP_USER = "ace"
SFTP_HOST = "localhost"


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


def _connect(port):
    return socket.create_connection(("127.0.0.1", port), timeout=10)


def _rpc(sock, method, params=None, rpc_id=1, timeout=15):
    msg = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
    if params is not None:
        msg["params"] = params
    sock.sendall((json.dumps(msg) + "\n").encode("utf-8"))
    buf = b""
    deadline = time.time() + timeout
    while time.time() < deadline:
        remaining = deadline - time.time()
        if remaining <= 0:
            break
        sock.settimeout(remaining)
        try:
            chunk = sock.recv(8192)
        except socket.timeout:
            break
        if not chunk:
            break
        buf += chunk
        if b"\n" in buf:
            break
    line, _, _ = buf.partition(b"\n")
    return json.loads(line.decode("utf-8"))


def _call(sock, tool, args, rpc_id=1, timeout=15):
    return _rpc(sock, "tools/call", {"name": tool, "arguments": args},
                rpc_id=rpc_id, timeout=timeout)


def main() -> int:
    proc, port = _launch()
    extra_socks = []
    try:
        s = _connect(port)
        failures = []
        rid = [0]

        def nid():
            rid[0] += 1
            return rid[0]

        tl = _rpc(s, "tools/list", rpc_id=nid())
        tools = {t["name"] for t in (tl.get("result") or {}).get("tools", [])}
        print(f"[info] tools: {sorted(tools)}")

        # One pool for ace@localhost; mc=1 so 02.11 blocking is demonstrable.
        # ka=5 so 02.17 expiry is observable with a 7-second wait.
        r_ep = _call(s, "open-endpoint", {
            "user": SFTP_USER, "host": SFTP_HOST, "mc": 1, "ct": 10, "ka": 5,
        }, rpc_id=nid())
        if "error" in r_ep:
            print(f"[FAIL] setup: open-endpoint: {r_ep['error']}")
            return 1
        ep = r_ep["result"]["endpoint_id"]

        # --- 02.10: acquire returns an open Connection ---
        r = _call(s, "acquire", {"endpoint_id": ep}, rpc_id=nid())
        if "error" in r:
            failures.append(f"02.10: acquire failed: {r['error']}")
            print(f"[FAIL] 02.10: acquire failed: {r['error']}")
        else:
            conn = r["result"]["connection_id"]
            r2 = _call(s, "stat", {"connection_id": conn, "path": "/home/ace"}, rpc_id=nid())
            if "error" in r2:
                failures.append(f"02.10: stat on acquired connection failed: {r2['error']}")
                print(f"[FAIL] 02.10: connection not open — stat failed: {r2['error']}")
            else:
                print("[PASS] 02.10: acquire returned open Connection")
            _call(s, "release", {"connection_id": conn}, rpc_id=nid())

        # --- 02.11: acquire blocks when mc saturated; resumes after release ---
        r = _call(s, "acquire", {"endpoint_id": ep}, rpc_id=nid())
        if "error" in r:
            failures.append(f"02.11: initial acquire failed: {r['error']}")
            print(f"[FAIL] 02.11: initial acquire failed")
        else:
            busy = r["result"]["connection_id"]
            s2 = _connect(port)
            extra_socks.append(s2)
            result2 = [None]

            def do_second_acquire():
                # Uses id=1 on its own socket — no conflict with s.
                result2[0] = _call(s2, "acquire", {"endpoint_id": ep}, rpc_id=1, timeout=20)

            t = threading.Thread(target=do_second_acquire)
            t.start()
            time.sleep(0.5)
            still_blocking = result2[0] is None
            _call(s, "release", {"connection_id": busy}, rpc_id=nid())
            t.join(timeout=10)
            if not still_blocking:
                failures.append("02.11: second acquire returned before release (did not block)")
                print("[FAIL] 02.11: second acquire returned before release")
            elif result2[0] is None or "error" in result2[0]:
                failures.append(f"02.11: second acquire failed after release: {result2[0]}")
                print(f"[FAIL] 02.11: second acquire failed after release: {result2[0]}")
            else:
                print("[PASS] 02.11: acquire blocked on mc=1; unblocked after release")
                _call(s2, "release",
                      {"connection_id": result2[0]["result"]["connection_id"]}, rpc_id=2)

        # --- 02.12: released connection within ka window is reused ---
        r1 = _call(s, "acquire", {"endpoint_id": ep}, rpc_id=nid())
        if "error" in r1:
            failures.append(f"02.12: first acquire failed: {r1['error']}")
            print(f"[FAIL] 02.12: first acquire failed")
        else:
            _call(s, "release", {"connection_id": r1["result"]["connection_id"]}, rpc_id=nid())
            r2 = _call(s, "acquire", {"endpoint_id": ep}, rpc_id=nid())
            if "error" in r2:
                failures.append(f"02.12: acquire within ka window failed: {r2['error']}")
                print(f"[FAIL] 02.12: acquire within ka window failed")
            else:
                print("[PASS] 02.12: connection reused within ka window")
                _call(s, "release", {"connection_id": r2["result"]["connection_id"]}, rpc_id=nid())

        # --- 02.13: ct timeout on fresh connection surfaces as I/O error ---
        # A local TCP socket that accepts but never speaks SSH triggers the handshake timeout.
        listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        listener.bind(("127.0.0.1", 0))
        listener.listen(5)
        hang_port = listener.getsockname()[1]

        def accept_and_hang():
            try:
                listener.settimeout(10)
                hung, _ = listener.accept()
                time.sleep(30)
                hung.close()
            except Exception:
                pass

        threading.Thread(target=accept_and_hang, daemon=True).start()
        try:
            r_ep13 = _call(s, "open-endpoint", {
                "user": SFTP_USER, "host": "127.0.0.1", "port": hang_port,
                "mc": 1, "ct": 2, "ka": 5,
            }, rpc_id=nid())
            if "error" in r_ep13:
                failures.append(f"02.13: open-endpoint failed: {r_ep13['error']}")
                print(f"[FAIL] 02.13: open-endpoint failed")
            else:
                ep13 = r_ep13["result"]["endpoint_id"]
                r13 = _call(s, "acquire", {"endpoint_id": ep13}, rpc_id=nid(), timeout=10)
                if "error" in r13 and r13["error"].get("code") == -32000:
                    print("[PASS] 02.13: ct timeout surfaced as I/O error")
                else:
                    failures.append(f"02.13: expected -32000 I/O error, got: {r13}")
                    print(f"[FAIL] 02.13: expected I/O error, got: {r13}")
        finally:
            listener.close()

        # --- 02.14: authentication failure surfaces as I/O error ---
        r_ep14 = _call(s, "open-endpoint", {
            "user": "no_such_user_xyzzy_12345", "host": SFTP_HOST,
            "mc": 1, "ct": 10, "ka": 5,
        }, rpc_id=nid())
        if "error" in r_ep14:
            failures.append(f"02.14: open-endpoint failed: {r_ep14['error']}")
            print(f"[FAIL] 02.14: open-endpoint failed unexpectedly")
        else:
            ep14 = r_ep14["result"]["endpoint_id"]
            r14 = _call(s, "acquire", {"endpoint_id": ep14}, rpc_id=nid())
            if "error" in r14 and r14["error"].get("code") == -32000:
                print("[PASS] 02.14: auth failure surfaced as I/O error")
            else:
                failures.append(f"02.14: expected -32000 I/O error, got: {r14}")
                print(f"[FAIL] 02.14: expected I/O error, got: {r14}")

        # --- 02.15: release returns connection to pool, freeing a slot ---
        r15a = _call(s, "acquire", {"endpoint_id": ep}, rpc_id=nid())
        if "error" in r15a:
            failures.append(f"02.15: acquire failed: {r15a['error']}")
            print(f"[FAIL] 02.15: acquire failed")
        else:
            _call(s, "release", {"connection_id": r15a["result"]["connection_id"]}, rpc_id=nid())
            r15b = _call(s, "acquire", {"endpoint_id": ep}, rpc_id=nid())
            if "error" in r15b:
                failures.append(f"02.15: acquire after release failed: {r15b['error']}")
                print(f"[FAIL] 02.15: slot not freed by release — acquire blocked/failed")
            else:
                print("[PASS] 02.15: release freed slot; subsequent acquire succeeded")
                _call(s, "release", {"connection_id": r15b["result"]["connection_id"]}, rpc_id=nid())

        # --- 02.16: reuse within ka window resets idle timer ---
        # Strategy: release at T=0 (original expiry T=5), wait 3s, reuse-acquire+release at
        # T=3 (resets timer to expire at T=8), wait 3s more (T=6 > original expiry T=5 but
        # < reset expiry T=8). A passing 3rd acquire at T=6 proves the reset happened.
        r16a = _call(s, "acquire", {"endpoint_id": ep}, rpc_id=nid())
        if "error" in r16a:
            failures.append(f"02.16: first acquire failed: {r16a['error']}")
            print(f"[FAIL] 02.16: first acquire failed")
        else:
            _call(s, "release", {"connection_id": r16a["result"]["connection_id"]}, rpc_id=nid())
            # Wait within ka=5 window so the reuse acquire straddles the original expiry.
            time.sleep(3)
            r16b = _call(s, "acquire", {"endpoint_id": ep}, rpc_id=nid())
            if "error" in r16b:
                failures.append(f"02.16: reuse acquire failed: {r16b['error']}")
                print(f"[FAIL] 02.16: reuse acquire failed")
            else:
                _call(s, "release", {"connection_id": r16b["result"]["connection_id"]}, rpc_id=nid())
                # T=6 from 1st release: past original expiry (T=5), within reset expiry (T=8).
                time.sleep(3)
                r16c = _call(s, "acquire", {"endpoint_id": ep}, rpc_id=nid())
                if "error" in r16c:
                    failures.append(f"02.16: acquire after ka-reset failed: {r16c['error']}")
                    print(f"[FAIL] 02.16: acquire after ka-reset failed")
                else:
                    print("[PASS] 02.16: ka timer reset on reuse; connection survived past original expiry")
                    _call(s, "release", {"connection_id": r16c["result"]["connection_id"]}, rpc_id=nid())

        # --- 02.17: idle > ka seconds causes underlying SSH session to close ---
        # After ka=5 expires a new acquire must succeed (old session closed, fresh one created).
        r17a = _call(s, "acquire", {"endpoint_id": ep}, rpc_id=nid())
        if "error" in r17a:
            failures.append(f"02.17: initial acquire failed: {r17a['error']}")
            print(f"[FAIL] 02.17: initial acquire failed")
        else:
            _call(s, "release", {"connection_id": r17a["result"]["connection_id"]}, rpc_id=nid())
            print("[info] 02.17: waiting 7s for ka=5 to expire...")
            time.sleep(7)
            r17b = _call(s, "acquire", {"endpoint_id": ep}, rpc_id=nid())
            if "error" in r17b:
                failures.append(f"02.17: acquire after ka expiry failed: {r17b['error']}")
                print(f"[FAIL] 02.17: acquire after ka expiry failed")
            else:
                print("[PASS] 02.17: new connection established after ka expiry")
                _call(s, "release", {"connection_id": r17b["result"]["connection_id"]}, rpc_id=nid())

        if failures:
            print("\nFAILURES:")
            for f in failures:
                print(f"  - {f}")
            return 1
        print("\nAll assertions passed.")
        return 0

    finally:
        for sock in extra_socks:
            try:
                sock.close()
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
