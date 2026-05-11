#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Tests that parse-uri exposes scheme, authority, path, query, and fragment correctly."""

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


def parse_uri(sock, uri, rpc_id):
    return _rpc(sock, "tools/call", {"name": "parse-uri", "arguments": {"uri": uri}}, rpc_id)


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []
            rid = 1

            # 02.10a — scheme present
            resp = parse_uri(s, "http://example.com/path", rid); rid += 1
            result = resp.get("result", {})
            scheme = result.get("scheme")
            print(f"[02.10a] scheme present: scheme={scheme!r}")
            if scheme != "http":
                failures.append("02.10a: expected scheme='http', got " + repr(scheme))

            # 02.10b — scheme absent
            resp = parse_uri(s, "//example.com/path", rid); rid += 1
            result = resp.get("result", {})
            scheme = result.get("scheme")
            print(f"[02.10b] scheme absent: scheme={scheme!r}")
            if scheme is not None:
                failures.append("02.10b: expected scheme absent (None), got " + repr(scheme))

            # 02.19 — scheme case preserved, not folded
            resp = parse_uri(s, "HTTP://example.com/path", rid); rid += 1
            result = resp.get("result", {})
            scheme = result.get("scheme")
            print(f"[02.19] scheme case preserved: scheme={scheme!r}")
            if scheme != "HTTP":
                failures.append("02.19: expected scheme='HTTP' (no folding), got " + repr(scheme))

            # 02.11a — authority present exposes userinfo, host, port keys
            resp = parse_uri(s, "http://user:pass@host:8080/path", rid); rid += 1
            result = resp.get("result", {})
            authority = result.get("authority")
            print(f"[02.11a] authority present: authority={authority!r}")
            if not isinstance(authority, dict):
                failures.append("02.11a: expected authority dict, got " + repr(authority))
            else:
                for key in ("host",):
                    if key not in authority:
                        failures.append(f"02.11a: authority missing key '{key}'")

            # 02.11b — authority absent
            resp = parse_uri(s, "/path/only", rid); rid += 1
            result = resp.get("result", {})
            authority = result.get("authority")
            print(f"[02.11b] authority absent: authority={authority!r}")
            if authority is not None:
                failures.append("02.11b: expected authority absent (None), got " + repr(authority))

            # 02.12 — userinfo splits into user and password at first colon
            resp = parse_uri(s, "http://alice:secret@host/", rid); rid += 1
            result = resp.get("result", {})
            authority = result.get("authority") or {}
            userinfo = authority.get("userinfo") if isinstance(authority, dict) else None
            user = userinfo.get("user") if isinstance(userinfo, dict) else None
            password = userinfo.get("password") if isinstance(userinfo, dict) else None
            print(f"[02.12] userinfo split: user={user!r} password={password!r}")
            if user != "alice":
                failures.append("02.12: expected user='alice', got " + repr(user))
            if password != "secret":
                failures.append("02.12: expected password='secret', got " + repr(password))

            # 02.13a — empty userinfo (@host) is present-but-empty, not absent
            resp = parse_uri(s, "http://@host/", rid); rid += 1
            result = resp.get("result", {})
            authority = result.get("authority") or {}
            userinfo_empty = authority.get("userinfo") if isinstance(authority, dict) else None
            print(f"[02.13a] empty userinfo present: userinfo={userinfo_empty!r}")
            if userinfo_empty is None:
                failures.append("02.13a: expected present-but-empty userinfo, got absent")

            # 02.13b — no @ means userinfo is absent
            resp = parse_uri(s, "http://host/", rid); rid += 1
            result = resp.get("result", {})
            authority = result.get("authority") or {}
            userinfo_absent = authority.get("userinfo") if isinstance(authority, dict) else None
            print(f"[02.13b] no userinfo absent: userinfo={userinfo_absent!r}")
            if userinfo_absent is not None:
                failures.append("02.13b: expected userinfo absent (None), got " + repr(userinfo_absent))

            # 02.14 — port is an integer when present
            resp = parse_uri(s, "http://host:9090/", rid); rid += 1
            result = resp.get("result", {})
            authority = result.get("authority") or {}
            port_val = authority.get("port") if isinstance(authority, dict) else None
            print(f"[02.14] port is integer: port={port_val!r} type={type(port_val).__name__}")
            if not isinstance(port_val, int):
                failures.append("02.14: expected port int, got " + repr(port_val))
            elif port_val != 9090:
                failures.append("02.14: expected port=9090, got " + repr(port_val))

            # 02.15a — path is always present (non-empty case)
            resp = parse_uri(s, "http://host/some/path", rid); rid += 1
            result = resp.get("result", {})
            path = result.get("path")
            print(f"[02.15a] path present: path={path!r}")
            if path != "/some/path":
                failures.append("02.15a: expected path='/some/path', got " + repr(path))

            # 02.15b — path is always present and may be empty
            resp = parse_uri(s, "http://host", rid); rid += 1
            result = resp.get("result", {})
            path = result.get("path")
            print(f"[02.15b] path empty string: path={path!r}")
            if path is None:
                failures.append("02.15b: expected path='' (always present), got None")
            elif path != "":
                failures.append("02.15b: expected path='', got " + repr(path))

            # 02.16a — query present
            resp = parse_uri(s, "http://host/path?foo=bar", rid); rid += 1
            result = resp.get("result", {})
            query = result.get("query")
            print(f"[02.16a] query present: query={query!r}")
            if query != "foo=bar":
                failures.append("02.16a: expected query='foo=bar', got " + repr(query))

            # 02.16b — query absent
            resp = parse_uri(s, "http://host/path", rid); rid += 1
            result = resp.get("result", {})
            query = result.get("query")
            print(f"[02.16b] query absent: query={query!r}")
            if query is not None:
                failures.append("02.16b: expected query absent (None), got " + repr(query))

            # 02.17a — fragment present
            resp = parse_uri(s, "http://host/path#section1", rid); rid += 1
            result = resp.get("result", {})
            fragment = result.get("fragment")
            print(f"[02.17a] fragment present: fragment={fragment!r}")
            if fragment != "section1":
                failures.append("02.17a: expected fragment='section1', got " + repr(fragment))

            # 02.17b — fragment absent
            resp = parse_uri(s, "http://host/path", rid); rid += 1
            result = resp.get("result", {})
            fragment = result.get("fragment")
            print(f"[02.17b] fragment absent: fragment={fragment!r}")
            if fragment is not None:
                failures.append("02.17b: expected fragment absent (None), got " + repr(fragment))

            # 02.18 — query is raw, not split on & or =
            resp = parse_uri(s, "http://host/path?a=1&b=2&c=3", rid); rid += 1
            result = resp.get("result", {})
            query = result.get("query")
            print(f"[02.18] query raw: query={query!r}")
            if query != "a=1&b=2&c=3":
                failures.append("02.18: expected query='a=1&b=2&c=3' (raw), got " + repr(query))

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
