#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercises the public parse/normalize API surface (reqs/01_api-surface.md)."""

from __future__ import annotations

import json, os, socket, subprocess, sys, threading, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY",
                               "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

_URL = "file:///tmp/test"
_CWD = "/tmp"
_USER = "testuser"


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


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []

            # 01.1 — parse returns TaggedGroup with role and urls list
            r = _rpc(s, "tools/call",
                     {"name": "parse", "arguments": {"text": _URL, "cwd": _CWD, "default_user": _USER}},
                     rpc_id=1)
            result = (r.get("result") or {})
            content = result.get("content", [{}])
            parsed_obj = json.loads(content[0].get("text", "{}")) if content else {}
            has_role = "role" in parsed_obj
            has_urls = isinstance(parsed_obj.get("urls"), list) and len(parsed_obj.get("urls", [])) > 0
            print(f"[01.1] parse returns TaggedGroup with role and urls: role={has_role}, urls={has_urls}")
            if not has_role or not has_urls:
                failures.append("01.1: parse result missing role or non-empty urls list")

            # 01.2 — normalize returns a canonical identity string
            r2 = _rpc(s, "tools/call",
                      {"name": "normalize", "arguments": {"url": _URL, "cwd": _CWD, "default_user": _USER}},
                      rpc_id=2)
            result2 = (r2.get("result") or {})
            content2 = result2.get("content", [{}])
            identity = json.loads(content2[0].get("text", "null")) if content2 else None
            is_string = isinstance(identity, str) and len(identity) > 0
            print(f"[01.2] normalize returns canonical identity string: {repr(identity)}, is_string={is_string}")
            if not is_string:
                failures.append(f"01.2: normalize did not return a non-empty string, got {repr(identity)}")

            # 01.3 — normalize(u) == parse(u).urls[0].identity
            parse_identity = parsed_obj.get("urls", [{}])[0].get("identity") if parsed_obj.get("urls") else None
            match = (identity == parse_identity)
            print(f"[01.3] normalize == parse.urls[0].identity: normalize={repr(identity)}, parse={repr(parse_identity)}, match={match}")
            if not match:
                failures.append(f"01.3: normalize={repr(identity)} != parse.urls[0].identity={repr(parse_identity)}")

            # 01.4 — empty input is rejected
            r3 = _rpc(s, "tools/call",
                      {"name": "parse", "arguments": {"text": "", "cwd": _CWD, "default_user": _USER}},
                      rpc_id=3)
            is_error = ("error" in r3) or (r3.get("result", {}).get("isError") is True)
            print(f"[01.4] empty input is rejected: is_error={is_error}")
            if not is_error:
                failures.append("01.4: empty input was not rejected")

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
