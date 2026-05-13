#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""SFTP URL field population and rejection (03_sftp-parsing)."""

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


def _call(sock, url, rpc_id):
    return _rpc(sock, "tools/call", {"name": "parse", "arguments": {"text": url, "cwd": "/tmp", "default_user": "testuser"}}, rpc_id=rpc_id)


def _parsed(resp):
    """Extract the ParsedUrl dict from a successful tools/call response."""
    try:
        text = resp["result"]["content"][0]["text"]
        data = json.loads(text)
        urls = data.get("urls") or []
        return urls[0] if urls else {}
    except (KeyError, IndexError, ValueError, TypeError):
        return {}


def _is_rejected(resp):
    """Return True if the tool call signals a parse error."""
    if resp.get("error") is not None:
        return True
    result = resp.get("result") or {}
    if result.get("isError"):
        return True
    return False


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []

            # 03.1 — sftp://user@host/path populates ParsedUrl.user
            resp = _call(s, "sftp://alice@example.com/path", rpc_id=1)
            parsed = _parsed(resp)
            print(f"[03.1] sftp://alice@example.com/path -> user={parsed.get('user')!r}")
            if parsed.get("user") != "alice":
                failures.append("03.1: expected user='alice', got " + repr(parsed.get("user")))

            # 03.2 — sftp://user:password@host/path populates user and password
            resp = _call(s, "sftp://alice:secret@example.com/path", rpc_id=2)
            parsed = _parsed(resp)
            print(f"[03.2] sftp://alice:secret@example.com/path -> user={parsed.get('user')!r}, password={parsed.get('password')!r}")
            if parsed.get("user") != "alice" or parsed.get("password") != "secret":
                failures.append(
                    "03.2: expected user='alice' password='secret', got "
                    + repr(parsed.get("user")) + "/" + repr(parsed.get("password"))
                )

            # 03.3 — ParsedUrl.host is populated from the authority
            resp = _call(s, "sftp://alice@myhost.example.com/path", rpc_id=3)
            parsed = _parsed(resp)
            print(f"[03.3] sftp://alice@myhost.example.com/path -> host={parsed.get('host')!r}")
            if parsed.get("host") != "myhost.example.com":
                failures.append("03.3: expected host='myhost.example.com', got " + repr(parsed.get("host")))

            # 03.4 — sftp URL with explicit port populates ParsedUrl.port
            resp = _call(s, "sftp://alice@example.com:2222/path", rpc_id=4)
            parsed = _parsed(resp)
            print(f"[03.4] sftp://alice@example.com:2222/path -> port={parsed.get('port')!r}")
            if parsed.get("port") != 2222:
                failures.append("03.4: expected port=2222, got " + repr(parsed.get("port")))

            # 03.5 — sftp URL without a host is rejected
            resp = _call(s, "sftp:///path", rpc_id=5)
            rejected = _is_rejected(resp)
            print(f"[03.5] sftp:///path -> rejected={rejected}")
            if not rejected:
                failures.append("03.5: expected rejection for sftp URL without host")

            # 03.6 — sftp URL with port outside 1..=65535 is rejected (test port=0)
            resp = _call(s, "sftp://alice@example.com:0/path", rpc_id=6)
            rejected = _is_rejected(resp)
            print(f"[03.6] sftp://alice@example.com:0/path (port=0) -> rejected={rejected}")
            if not rejected:
                failures.append("03.6: expected rejection for sftp URL with port=0 (outside 1..=65535)")

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
