#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercises percent_decode and percent_decode_unreserved per RFC 3986 §2.1 and §6.2.2.2 (req 02.40–02.44)."""

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


def _pd(sock, input_str, rpc_id):
    return _rpc(sock, "tools/call",
                {"name": "percent-decode", "arguments": {"s": input_str}}, rpc_id)


def _pdu(sock, input_str, rpc_id):
    return _rpc(sock, "tools/call",
                {"name": "percent-decode-unreserved", "arguments": {"s": input_str}}, rpc_id)


def _decoded(resp):
    """Extract the decoded string from a successful tools/call response."""
    r = resp.get("result")
    if isinstance(r, dict):
        for key in ("value", "result", "decoded"):
            if key in r:
                return r[key]
        # fallback: return first string value found
        for v in r.values():
            if isinstance(v, str):
                return v
    if isinstance(r, str):
        return r
    return None


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []
            rid = 1

            # --- 02.40: percent_decode decodes every %HH triplet ---
            resp = _pd(s, "hello%20world", rid); rid += 1
            got = _decoded(resp)
            print(f"[02.40a] percent-decode 'hello%20world': {got!r}")
            if "error" in resp:
                failures.append(f"02.40a: unexpected error: {resp['error']}")
            elif got != "hello world":
                failures.append(f"02.40a: expected 'hello world', got {got!r}")

            resp = _pd(s, "%41%42%43", rid); rid += 1
            got = _decoded(resp)
            print(f"[02.40b] percent-decode '%41%42%43': {got!r}")
            if "error" in resp:
                failures.append(f"02.40b: unexpected error: {resp['error']}")
            elif got != "ABC":
                failures.append(f"02.40b: expected 'ABC', got {got!r}")

            resp = _pd(s, "%2F", rid); rid += 1
            got = _decoded(resp)
            print(f"[02.40c] percent-decode '%2F': {got!r}")
            if "error" in resp:
                failures.append(f"02.40c: unexpected error for valid %2F: {resp['error']}")
            elif got != "/":
                failures.append(f"02.40c: expected '/', got {got!r}")

            # --- 02.41: percent_decode returns PercentDecodeError for invalid triplets ---
            resp = _pd(s, "%ZZ", rid); rid += 1
            print(f"[02.41a] percent-decode '%ZZ' (invalid hex): {'error' in resp}")
            if "error" not in resp:
                failures.append(f"02.41a: expected error for '%ZZ', got result={resp.get('result')!r}")

            resp = _pd(s, "test%", rid); rid += 1
            print(f"[02.41b] percent-decode 'test%' (truncated): {'error' in resp}")
            if "error" not in resp:
                failures.append(f"02.41b: expected error for 'test%', got result={resp.get('result')!r}")

            resp = _pd(s, "%2", rid); rid += 1
            print(f"[02.41c] percent-decode '%2' (one hex digit): {'error' in resp}")
            if "error" not in resp:
                failures.append(f"02.41c: expected error for '%2', got result={resp.get('result')!r}")

            # --- 02.42: percent_decode_unreserved decodes only unreserved characters ---
            # unreserved = ALPHA / DIGIT / "-" / "." / "_" / "~"
            # %41='A' (unreserved) -> decoded; %2F='/' (reserved) -> stays
            resp = _pdu(s, "%41%2F", rid); rid += 1
            got = _decoded(resp)
            print(f"[02.42a] percent-decode-unreserved '%41%2F': {got!r}")
            if "error" in resp:
                failures.append(f"02.42a: unexpected error: {resp['error']}")
            elif got is None or not (got.startswith("A") and "%" in got):
                failures.append(f"02.42a: expected 'A%2F' (A decoded, / stays), got {got!r}")

            # %7E='~' (unreserved) -> decoded
            resp = _pdu(s, "%7E", rid); rid += 1
            got = _decoded(resp)
            print(f"[02.42b] percent-decode-unreserved '%7E' (tilde, unreserved): {got!r}")
            if "error" in resp:
                failures.append(f"02.42b: unexpected error: {resp['error']}")
            elif got != "~":
                failures.append(f"02.42b: expected '~', got {got!r}")

            # %3A=':' (gen-delim, reserved) -> stays
            resp = _pdu(s, "%3A", rid); rid += 1
            got = _decoded(resp)
            print(f"[02.42c] percent-decode-unreserved '%3A' (colon, reserved): {got!r}")
            if "error" in resp:
                failures.append(f"02.42c: unexpected error: {resp['error']}")
            elif got is None or "%" not in got:
                failures.append(f"02.42c: expected reserved '%3A' to stay encoded, got {got!r}")

            # --- 02.43: percent_decode_unreserved uppercases hex digits of remaining triplets ---
            resp = _pdu(s, "%2f", rid); rid += 1
            got = _decoded(resp)
            print(f"[02.43a] percent-decode-unreserved '%2f' -> '%2F': {got!r}")
            if "error" in resp:
                failures.append(f"02.43a: unexpected error: {resp['error']}")
            elif got != "%2F":
                failures.append(f"02.43a: expected '%2F' (uppercased), got {got!r}")

            resp = _pdu(s, "%41%2f", rid); rid += 1
            got = _decoded(resp)
            print(f"[02.43b] percent-decode-unreserved '%41%2f' -> 'A%2F': {got!r}")
            if "error" in resp:
                failures.append(f"02.43b: unexpected error: {resp['error']}")
            elif got != "A%2F":
                failures.append(f"02.43b: expected 'A%2F', got {got!r}")

            # --- 02.44: percent_decode_unreserved returns PercentDecodeError for invalid triplets ---
            resp = _pdu(s, "%ZZ", rid); rid += 1
            print(f"[02.44a] percent-decode-unreserved '%ZZ' (invalid hex): {'error' in resp}")
            if "error" not in resp:
                failures.append(f"02.44a: expected error for '%ZZ', got result={resp.get('result')!r}")

            resp = _pdu(s, "test%", rid); rid += 1
            print(f"[02.44b] percent-decode-unreserved 'test%' (truncated): {'error' in resp}")
            if "error" not in resp:
                failures.append(f"02.44b: expected error for 'test%', got result={resp.get('result')!r}")

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
