#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercises file_uri_to_path behavior per reqs/02_uri-to-path.md."""

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


def _call(sock, args, rpc_id):
    resp = _rpc(sock, "tools/call",
                {"name": "file_uri_to_path", "arguments": args},
                rpc_id=rpc_id)
    result = resp.get("result", {})
    content = result.get("content", [])
    if content and content[0].get("type") == "text":
        return content[0]["text"]
    return None


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []

            # 02.18: empty authority → local filesystem, POSIX path begins with /
            r = _call(s, {"uri": "file:///home/user/file.txt", "style": "posix"}, rpc_id=1)
            print(f"[02.18] empty authority → {r!r}")
            if r is None or not r.startswith("/"):
                failures.append(f"02.18: expected path starting with '/', got {r!r}")

            # 02.19: localhost authority → local filesystem, POSIX path begins with /
            r = _call(s, {"uri": "file://localhost/home/user/file.txt", "style": "posix"}, rpc_id=2)
            print(f"[02.19] localhost authority → {r!r}")
            if r is None or not r.startswith("/"):
                failures.append(f"02.19: expected path starting with '/', got {r!r}")

            # 02.20: non-empty non-localhost authority → UNC server name, path begins with \\server\
            r = _call(s, {"uri": "file://myserver/share/data", "style": "windows"}, rpc_id=3)
            print(f"[02.20] UNC authority → {r!r}")
            if r is None or not r.startswith("\\\\myserver\\"):
                failures.append(f"02.20: expected path starting with '\\\\myserver\\', got {r!r}")

            # 02.21: /<letter>:/<rest> → leading / dropped, DOS-style path
            r = _call(s, {"uri": "file:///C:/Users/foo", "style": "windows"}, rpc_id=4)
            print(f"[02.21] drive+rest path → {r!r}")
            if r is None or r.startswith("/") or not r.upper().startswith("C:"):
                failures.append(f"02.21: expected DOS-style path starting with 'C:', got {r!r}")

            # 02.22: /<letter>: (drive only) → leading / dropped
            r = _call(s, {"uri": "file:///D:", "style": "windows"}, rpc_id=5)
            print(f"[02.22] drive-only path → {r!r}")
            if r is None or r.startswith("/") or not r.upper().startswith("D:"):
                failures.append(f"02.22: expected DOS-style path starting with 'D:', got {r!r}")

            # 02.23: percent-encoded octets are decoded in the returned path
            r = _call(s, {"uri": "file:///path/with%20space/file.txt", "style": "posix"}, rpc_id=6)
            print(f"[02.23] percent-encoded → {r!r}")
            if r is None or "with space" not in r:
                failures.append(f"02.23: expected decoded 'with space' in result, got {r!r}")

            # 02.24: style=posix → forward-slash separators, no backslashes
            r = _call(s, {"uri": "file:///home/user/docs/file.txt", "style": "posix"}, rpc_id=7)
            print(f"[02.24] posix style → {r!r}")
            if r is None or "\\" in r:
                failures.append(f"02.24: expected POSIX path (no backslashes), got {r!r}")

            # 02.25: style=windows → backslash separators and DOS drive-letter formatting
            r = _call(s, {"uri": "file:///C:/Users/foo", "style": "windows"}, rpc_id=8)
            print(f"[02.25] windows style → {r!r}")
            if r is None or "\\" not in r:
                failures.append(f"02.25: expected Windows path with backslashes, got {r!r}")

            # 02.26: style absent → host platform native (Linux = POSIX = starts with /)
            r = _call(s, {"uri": "file:///home/user/file.txt"}, rpc_id=9)
            print(f"[02.26] native style → {r!r}")
            if r is None or not r.startswith("/"):
                failures.append(f"02.26: expected native POSIX path starting with '/', got {r!r}")

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
