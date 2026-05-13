#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Idle keep-alive timer: destroy after TTL, slot freed, reuse prevents destroy."""

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


def _call(sock, name, arguments, rid):
    return _rpc(sock, "tools/call", {"name": name, "arguments": arguments}, rpc_id=rid)


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []
            rid = 1

            # --- 02.1: released resource not reused within idle_ttl_seconds → destroy called ---
            TTL = 1.0
            r = _call(s, "create-pool", {"maxPerKey": 1, "idleTtlSeconds": TTL}, rid); rid += 1
            pool1 = (r.get("result") or {}).get("poolId")
            if not pool1:
                failures.append("02.1: create-pool failed")
            else:
                r = _call(s, "acquire", {"poolId": pool1, "key": "k"}, rid); rid += 1
                h1 = (r.get("result") or {}).get("handleId")
                if not h1:
                    failures.append("02.1: acquire failed")
                else:
                    _call(s, "release", {"handleId": h1}, rid); rid += 1
                    time.sleep(TTL * 2)
                    r = _call(s, "get-destroy-count", {"poolId": pool1}, rid); rid += 1
                    dc = (r.get("result") or {}).get("count", -1)
                    print(f"[02.1] destroy-count after idle TTL expiry: {dc}")
                    if dc != 1:
                        failures.append(f"02.1: expected destroy-count=1 after TTL, got {dc}")

            # --- 02.2: after TTL expiry destroys resource, slot freed; new acquire succeeds ---
            if pool1:
                r = _call(s, "acquire", {"poolId": pool1, "key": "k"}, rid); rid += 1
                h2 = (r.get("result") or {}).get("handleId")
                print(f"[02.2] acquire after TTL expiry returned handle: {h2!r}")
                if not h2:
                    failures.append("02.2: acquire after TTL expiry failed (slot not freed)")
                else:
                    _call(s, "release", {"handleId": h2}, rid); rid += 1

            # --- 02.3: reuse via acquire before idle timer fires prevents destroy ---
            TTL2 = 2.0
            r = _call(s, "create-pool", {"maxPerKey": 1, "idleTtlSeconds": TTL2}, rid); rid += 1
            pool2 = (r.get("result") or {}).get("poolId")
            if not pool2:
                failures.append("02.3: create-pool failed")
            else:
                r = _call(s, "acquire", {"poolId": pool2, "key": "rk"}, rid); rid += 1
                h3 = (r.get("result") or {}).get("handleId")
                if not h3:
                    failures.append("02.3: initial acquire failed")
                else:
                    _call(s, "release", {"handleId": h3}, rid); rid += 1
                    time.sleep(0.4)  # well within TTL2, re-acquire before timer fires
                    r = _call(s, "acquire", {"poolId": pool2, "key": "rk"}, rid); rid += 1
                    h4 = (r.get("result") or {}).get("handleId")
                    if not h4:
                        failures.append("02.3: re-acquire before TTL failed")
                    else:
                        time.sleep(TTL2 * 1.5)  # past original timer window; resource is held
                        r = _call(s, "get-destroy-count", {"poolId": pool2}, rid); rid += 1
                        dc2 = (r.get("result") or {}).get("count", -1)
                        print(f"[02.3] destroy-count after re-acquire (past original timer window): {dc2}")
                        if dc2 != 0:
                            failures.append(
                                f"02.3: expected destroy-count=0 after reuse before TTL, got {dc2}"
                            )
                        _call(s, "release", {"handleId": h4}, rid); rid += 1

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
