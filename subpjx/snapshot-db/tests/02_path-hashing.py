#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = ["xxhash"]
# ///
"""02_path-hashing: xxHash64-seed-0 encoded as 11-char base62 path identifier."""

from __future__ import annotations

import json, os, socket, subprocess, sys, threading, time
import xxhash
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

_B62 = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"
_B62_SET = set(_B62)


def _expected_id(path: str) -> str:
    """Independent reference: xxHash64(path bytes, seed=0) encoded as base62, zero-padded to 11."""
    h = xxhash.xxh64(path.encode("utf-8"), seed=0).intdigest()
    digits = []
    n = h
    while n > 0:
        digits.append(_B62[n % 62])
        n //= 62
    while len(digits) < 11:
        digits.append("0")
    return "".join(reversed(digits))


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


def _find_hash_tool(tools):
    """Return (tool_name, param_name) for the path-hashing tool, or (None, None)."""
    param_candidates = ("path", "relative_path", "relPath")
    for t in tools:
        props = t.get("inputSchema", {}).get("properties", {})
        for param in param_candidates:
            if param in props and props[param].get("type") == "string":
                return t["name"], param
    return None, None


def _call_hash(sock, tool_name, param_name, path, rpc_id):
    resp = _rpc(sock, "tools/call", {"name": tool_name, "arguments": {param_name: path}}, rpc_id)
    content = (resp.get("result") or {}).get("content", [])
    for item in content:
        if item.get("type") == "text":
            return item["text"].strip()
    return None


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []
            rpc_id = 1

            tl = _rpc(s, "tools/list", rpc_id=rpc_id); rpc_id += 1
            tools = (tl.get("result") or {}).get("tools", [])
            tool_names = [t["name"] for t in tools]
            print(f"[info] tools/list returned {len(tools)} tool(s): {tool_names}")

            hash_tool, param_name = _find_hash_tool(tools)
            if hash_tool is None:
                print("[02.1-02.5] FAIL: no path-hashing tool found in tools/list")
                failures.append(f"02.1-02.5: no path-hashing tool found; available: {tool_names}")
                print("\nFAILURES:")
                for f in failures:
                    print(f"  - {f}")
                return 1

            print(f"[info] hash tool: {hash_tool!r}, param: {param_name!r}")

            test_path = "docs/readme.txt"

            # 02.1 — identifier is exactly 11 characters
            id1 = _call_hash(s, hash_tool, param_name, test_path, rpc_id); rpc_id += 1
            print(f"[02.1] hash_path({test_path!r}) = {id1!r}, len={len(id1) if id1 else 'None'}")
            if id1 is None or len(id1) != 11:
                failures.append(f"02.1: expected 11-char id, got {id1!r}")

            # 02.2 — only base62 characters (0-9, A-Z, a-z)
            print(f"[02.2] checking base62 chars in {id1!r}")
            if id1 is not None:
                bad = [c for c in id1 if c not in _B62_SET]
                if bad:
                    failures.append(f"02.2: non-base62 chars {bad!r} in {id1!r}")
            else:
                failures.append("02.2: cannot check chars, id1 is None")

            # 02.3 — same path yields same identifier on a second call
            id2 = _call_hash(s, hash_tool, param_name, test_path, rpc_id); rpc_id += 1
            print(f"[02.3] second call hash_path({test_path!r}) = {id2!r}")
            if id1 != id2:
                failures.append(f"02.3: idempotency failed; first={id1!r} second={id2!r}")

            # 02.4 — matches xxHash64-seed-0 base62 encoding (known-vector check)
            expected = _expected_id(test_path)
            print(f"[02.4] expected={expected!r} got={id1!r}")
            if id1 != expected:
                failures.append(f"02.4: hash mismatch; expected={expected!r} got={id1!r}")

            # 02.5 — root parent sentinel equals hash_path("/")
            root_id = _call_hash(s, hash_tool, param_name, "/", rpc_id); rpc_id += 1
            expected_root = _expected_id("/")
            print(f"[02.5] hash_path('/') = {root_id!r}, expected={expected_root!r}")
            if root_id != expected_root:
                failures.append(f"02.5: root sentinel mismatch; expected={expected_root!r} got={root_id!r}")

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
