#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""percent_encode encodes outside a named safe class (reqs 02.30–02.33)."""

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


def _encode(sock, input_str, safe_class, rpc_id):
    return _rpc(sock, "tools/call",
                {"name": "percent-encode", "arguments": {"s": input_str, "safe": safe_class}},
                rpc_id)


def _str_result(resp):
    """Extract the encoded string from a tools/call response, or return (None, err)."""
    if "error" in resp:
        return None, resp["error"].get("message", str(resp["error"]))
    r = resp.get("result")
    if isinstance(r, dict):
        for key in ("result", "encoded", "value"):
            if key in r and isinstance(r[key], str):
                return r[key], None
        for v in r.values():
            if isinstance(v, str):
                return v, None
    if isinstance(r, str):
        return r, None
    return None, f"no string in result: {r!r}"


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []
            rid = 1

            # 02.30 — percent_encode encodes per RFC 3986 §2.1
            # Space (0x20) is not in any RFC 3986 character class; must become %20
            resp = _encode(s, " ", "unreserved", rid); rid += 1
            encoded, err = _str_result(resp)
            print(f"[02.30] encode(' ', unreserved) -> {encoded!r} (err={err})")
            if err or encoded != "%20":
                failures.append(f"02.30: expected '%20', got {encoded!r} (err={err})")

            # 02.31 — characters in the safe class are left unencoded
            # "abc" are ALPHA, which are unreserved per RFC 3986 §2.3; must pass through unchanged
            resp = _encode(s, "abc", "unreserved", rid); rid += 1
            encoded, err = _str_result(resp)
            print(f"[02.31] encode('abc', unreserved) -> {encoded!r} (err={err})")
            if err or encoded != "abc":
                failures.append(f"02.31: expected 'abc', got {encoded!r} (err={err})")

            # 02.32 — accepts unreserved, gen_delims, sub_delims, reserved as safe class
            for cls in ("unreserved", "gen_delims", "sub_delims", "reserved"):
                resp = _encode(s, "a", cls, rid); rid += 1
                _, err = _str_result(resp)
                print(f"[02.32] encode('a', {cls!r}) err={err}")
                if err:
                    failures.append(f"02.32: safe class {cls!r} rejected: {err}")

            # 02.33 — accepts per-component allowed classes
            for cls in ("scheme", "userinfo", "host", "path", "query", "fragment"):
                resp = _encode(s, "a", cls, rid); rid += 1
                _, err = _str_result(resp)
                print(f"[02.33] encode('a', {cls!r}) err={err}")
                if err:
                    failures.append(f"02.33: safe class {cls!r} rejected: {err}")

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
