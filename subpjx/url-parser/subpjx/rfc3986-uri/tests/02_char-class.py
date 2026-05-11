#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercises RFC 3986 §2.2/§2.3 character-class predicates (req 02.50–02.53)."""

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


def _call(sock, tool, c, rpc_id):
    return _rpc(sock, "tools/call", {"name": tool, "arguments": {"c": c}}, rpc_id)


def _result(resp):
    """Return the boolean result value from a successful tools/call response."""
    r = resp.get("result")
    if isinstance(r, dict):
        # outputSchema wraps the bool: {"result": true}
        if "result" in r:
            return r["result"]
        # some wrappers use "value"
        if "value" in r:
            return r["value"]
    return r


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []
            rid = 1

            # --- 02.50: is-unreserved ---
            # unreserved = ALPHA / DIGIT / "-" / "." / "_" / "~"
            unreserved_true = list("ABCZabcz09") + ["-", ".", "_", "~"]
            unreserved_false = [":", "/", "?", "#", "[", "]", "@",
                                "!", "$", "&", "'", "(", ")", "*", "+", ",", ";", "="]
            ok_02_50 = True
            for c in unreserved_true:
                resp = _call(s, "is-unreserved", c, rid); rid += 1
                val = _result(resp)
                if val is not True:
                    failures.append(f"02.50: is-unreserved({c!r}) expected true, got {resp}")
                    ok_02_50 = False
            for c in unreserved_false:
                resp = _call(s, "is-unreserved", c, rid); rid += 1
                val = _result(resp)
                if val is not False:
                    failures.append(f"02.50: is-unreserved({c!r}) expected false, got {resp}")
                    ok_02_50 = False
            print(f"[02.50] is-unreserved: {'PASS' if ok_02_50 else 'FAIL'}")

            # --- 02.51: is-reserved ---
            # reserved = gen-delims / sub-delims
            reserved_true = [":", "/", "?", "#", "[", "]", "@",
                              "!", "$", "&", "'", "(", ")", "*", "+", ",", ";", "="]
            reserved_false = list("AZaz09") + ["-", ".", "_", "~"]
            ok_02_51 = True
            for c in reserved_true:
                resp = _call(s, "is-reserved", c, rid); rid += 1
                val = _result(resp)
                if val is not True:
                    failures.append(f"02.51: is-reserved({c!r}) expected true, got {resp}")
                    ok_02_51 = False
            for c in reserved_false:
                resp = _call(s, "is-reserved", c, rid); rid += 1
                val = _result(resp)
                if val is not False:
                    failures.append(f"02.51: is-reserved({c!r}) expected false, got {resp}")
                    ok_02_51 = False
            print(f"[02.51] is-reserved: {'PASS' if ok_02_51 else 'FAIL'}")

            # --- 02.52: is-gen-delim ---
            # gen-delims = ":" / "/" / "?" / "#" / "[" / "]" / "@"
            gen_delims_true = [":", "/", "?", "#", "[", "]", "@"]
            gen_delims_false = ["!", "$", "&", "'", "(", ")", "*", "+", ",", ";", "=",
                                "A", "z", "0", "-", "~"]
            ok_02_52 = True
            for c in gen_delims_true:
                resp = _call(s, "is-gen-delim", c, rid); rid += 1
                val = _result(resp)
                if val is not True:
                    failures.append(f"02.52: is-gen-delim({c!r}) expected true, got {resp}")
                    ok_02_52 = False
            for c in gen_delims_false:
                resp = _call(s, "is-gen-delim", c, rid); rid += 1
                val = _result(resp)
                if val is not False:
                    failures.append(f"02.52: is-gen-delim({c!r}) expected false, got {resp}")
                    ok_02_52 = False
            print(f"[02.52] is-gen-delim: {'PASS' if ok_02_52 else 'FAIL'}")

            # --- 02.53: is-sub-delim ---
            # sub-delims = "!" / "$" / "&" / "'" / "(" / ")" / "*" / "+" / "," / ";" / "="
            sub_delims_true = ["!", "$", "&", "'", "(", ")", "*", "+", ",", ";", "="]
            sub_delims_false = [":", "/", "?", "#", "[", "]", "@",
                                "A", "z", "0", "-", "~"]
            ok_02_53 = True
            for c in sub_delims_true:
                resp = _call(s, "is-sub-delim", c, rid); rid += 1
                val = _result(resp)
                if val is not True:
                    failures.append(f"02.53: is-sub-delim({c!r}) expected true, got {resp}")
                    ok_02_53 = False
            for c in sub_delims_false:
                resp = _call(s, "is-sub-delim", c, rid); rid += 1
                val = _result(resp)
                if val is not False:
                    failures.append(f"02.53: is-sub-delim({c!r}) expected false, got {resp}")
                    ok_02_53 = False
            print(f"[02.53] is-sub-delim: {'PASS' if ok_02_53 else 'FAIL'}")

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
