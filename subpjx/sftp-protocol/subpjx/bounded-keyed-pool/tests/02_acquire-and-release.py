#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercises 02_acquire-and-release: acquire/release/discard semantics and idle reuse."""

from __future__ import annotations

import json, os, socket, subprocess, sys, threading, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

_rpc_id = 0


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


def _rpc(sock, method, params=None):
    global _rpc_id
    _rpc_id += 1
    msg = {"jsonrpc": "2.0", "id": _rpc_id, "method": method}
    if params is not None:
        msg["params"] = params
    sock.sendall((json.dumps(msg) + "\n").encode("utf-8"))
    buf = b""
    deadline = time.time() + 10
    while time.time() < deadline:
        try:
            chunk = sock.recv(8192)
        except socket.timeout:
            break
        if not chunk:
            break
        buf += chunk
        if b"\n" in buf:
            break
    if not buf:
        return None
    line, _, _ = buf.partition(b"\n")
    return json.loads(line.decode("utf-8"))


def _call(sock, tool, args=None):
    resp = _rpc(sock, "tools/call", {"name": tool, "arguments": args or {}})
    if resp is None:
        return None
    if "error" in resp:
        return {"__error__": resp["error"]}
    result = resp.get("result") or {}
    if result.get("isError"):
        content = result.get("content", [])
        text = content[0].get("text", "") if content else ""
        return {"__error__": text}
    content = result.get("content", [])
    if content and content[0].get("type") == "text":
        text = content[0]["text"]
        try:
            return json.loads(text)
        except (json.JSONDecodeError, ValueError):
            return {"text": text}
    return result


def _extract_id(obj, *keys):
    if not isinstance(obj, dict):
        return None
    for k in keys:
        if k in obj:
            return obj[k]
    return None


def _extract_count(obj):
    if obj is None:
        return None
    if isinstance(obj, (int, float)):
        return int(obj)
    if isinstance(obj, dict):
        for k in ("count", "value", "n", "result"):
            if k in obj:
                v = obj[k]
                return int(v) if isinstance(v, (int, float)) else None
    return None


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []

            # --- 02.1: acquire with no idle resource invokes create ---
            pool1 = _call(s, "create-pool", {"maxPerKey": 2, "idleTtlSeconds": 60})
            pool1_id = _extract_id(pool1, "poolId", "id")
            h1 = _call(s, "acquire", {"poolId": pool1_id, "key": "k1"})
            handle1_id = _extract_id(h1, "handleId", "id")
            rv1 = h1.get("resourceValue") if isinstance(h1, dict) else None
            print(f"[02.1] resourceValue on first acquire (no idle): {rv1}")
            if rv1 != 1:
                failures.append(f"02.1: expected resourceValue=1 on fresh acquire (factory called once), got {rv1}")

            # --- 02.2: acquire with idle resource reuses it, create not called again ---
            _call(s, "release", {"handleId": handle1_id})
            h2 = _call(s, "acquire", {"poolId": pool1_id, "key": "k1"})
            handle2_id = _extract_id(h2, "handleId", "id")
            rv2 = h2.get("resourceValue") if isinstance(h2, dict) else None
            print(f"[02.2] resourceValue after acquire from idle cache: {rv2}")
            if rv2 != 1:
                failures.append(f"02.2: expected resourceValue=1 after idle reuse (factory not called again), got {rv2}")
            _call(s, "release", {"handleId": handle2_id})

            # --- 02.3: discard invokes destroy immediately ---
            pool2 = _call(s, "create-pool", {"maxPerKey": 2, "idleTtlSeconds": 60})
            pool2_id = _extract_id(pool2, "poolId", "id")
            h3 = _call(s, "acquire", {"poolId": pool2_id, "key": "k1"})
            handle3_id = _extract_id(h3, "handleId", "id")
            _call(s, "discard", {"handleId": handle3_id})
            dc = _extract_count(_call(s, "get-destroy-count", {"poolId": pool2_id}))
            print(f"[02.3] destroy_count immediately after discard: {dc}")
            if dc != 1:
                failures.append(f"02.3: expected destroy_count=1 immediately after discard, got {dc}")

            # --- 02.4: after discard, subsequent acquire for same key is not blocked ---
            pool3 = _call(s, "create-pool", {"maxPerKey": 1, "idleTtlSeconds": 60})
            pool3_id = _extract_id(pool3, "poolId", "id")
            h4 = _call(s, "acquire", {"poolId": pool3_id, "key": "k1"})
            handle4_id = _extract_id(h4, "handleId", "id")
            _call(s, "discard", {"handleId": handle4_id})

            result5 = [None]
            exc5 = [None]

            def _do_acquire():
                try:
                    result5[0] = _call(s, "acquire", {"poolId": pool3_id, "key": "k1"})
                except Exception as e:
                    exc5[0] = e

            t = threading.Thread(target=_do_acquire, daemon=True)
            t.start()
            t.join(timeout=3.0)

            if t.is_alive():
                print("[02.4] acquire after discard still blocked after 3s")
                failures.append("02.4: acquire after discard is blocking; discard did not free the slot")
            elif exc5[0] is not None:
                print(f"[02.4] acquire after discard raised: {exc5[0]}")
                failures.append(f"02.4: acquire after discard raised: {exc5[0]}")
            else:
                handle5_id = _extract_id(result5[0], "handleId", "id") if result5[0] else None
                print(f"[02.4] acquire after discard returned handle: {handle5_id is not None}")
                if handle5_id is None:
                    failures.append(f"02.4: acquire after discard returned no handle: {result5[0]}")
                else:
                    _call(s, "release", {"handleId": handle5_id})

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
