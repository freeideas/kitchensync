#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""serialize_uri renders Uri to RFC 3986 §5.3 string; parse_uri + serialize_uri round-trips."""

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


def _result(resp):
    """Extract the payload from a tools/call response."""
    r = resp.get("result")
    if isinstance(r, dict):
        if "result" in r:
            return r["result"]
        if "value" in r:
            return r["value"]
    return r


def _parse(sock, uri_str, rpc_id):
    resp = _rpc(sock, "tools/call", {"name": "parse-uri", "arguments": {"uri": uri_str}}, rpc_id)
    return _result(resp)


def _serialize(sock, uri_obj, rpc_id):
    resp = _rpc(sock, "tools/call", {"name": "serialize-uri", "arguments": {"uri": uri_obj}}, rpc_id)
    return _result(resp)


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []
            rid = 1

            # --- 02.20: serialize-uri renders a Uri back to RFC 3986 §5.3 string form ---
            # URI with all five components to verify §5.3 delimiter placement.
            full_uri = "http://user@example.com:8080/a/b?q=1#sec1"
            parsed = _parse(s, full_uri, rid); rid += 1
            if not isinstance(parsed, dict):
                failures.append(f"02.20: parse-uri did not return a dict; got {parsed!r}")
                print("[02.20] FAIL: parse-uri did not return a dict")
            else:
                serialized = _serialize(s, parsed, rid); rid += 1
                ok = True
                if not isinstance(serialized, str) or not serialized:
                    failures.append(f"02.20: serialize-uri did not return a non-empty string; got {serialized!r}")
                    ok = False
                else:
                    # RFC 3986 §5.3: scheme ":" "//" authority path "?" query "#" fragment
                    if "http:" not in serialized:
                        failures.append(f"02.20: missing 'http:' in {serialized!r}")
                        ok = False
                    if "//" not in serialized:
                        failures.append(f"02.20: missing '//' authority prefix in {serialized!r}")
                        ok = False
                    if "?" not in serialized:
                        failures.append(f"02.20: missing '?' query delimiter in {serialized!r}")
                        ok = False
                    if "#" not in serialized:
                        failures.append(f"02.20: missing '#' fragment delimiter in {serialized!r}")
                        ok = False
                print(f"[02.20] serialize-uri §5.3 delimiters: {'PASS' if ok else 'FAIL'}")

            # --- 02.21: parse-uri + serialize-uri round-trips well-formed inputs ---
            round_trip_cases = [
                "http://example.com/a/b?q=1#frag",
                "https://user:pass@host:8080/path",
                "ftp://ftp.example.org/resource",
                "urn:example:a123,z456",
                "/relative/path?q=1",
            ]
            ok_21 = True
            for uri_str in round_trip_cases:
                parsed = _parse(s, uri_str, rid); rid += 1
                if not isinstance(parsed, dict):
                    failures.append(f"02.21: parse-uri({uri_str!r}) did not return dict; got {parsed!r}")
                    ok_21 = False
                    continue
                serialized = _serialize(s, parsed, rid); rid += 1
                if serialized != uri_str:
                    failures.append(f"02.21: round-trip mismatch for {uri_str!r}: got {serialized!r}")
                    ok_21 = False
                else:
                    print(f"[02.21] round-trip ok: {uri_str!r}")
            if ok_21:
                print("[02.21] parse-uri + serialize-uri round-trips: PASS")
            else:
                print("[02.21] parse-uri + serialize-uri round-trips: FAIL")

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
