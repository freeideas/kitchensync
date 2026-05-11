#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercises parse_uri: scheme URI, relative reference, UriParseError on malformed input, error structure, and no stdout/stderr side effects."""

from __future__ import annotations

import json, os, queue, socket, subprocess, sys, threading, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")


def _collect(stream, buf: queue.Queue):
    for line in stream:
        buf.put(line)


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
    threading.Thread(target=_collect, args=(proc.stdout, queue.Queue()), daemon=True).start()
    stderr_q: queue.Queue = queue.Queue()
    threading.Thread(target=_collect, args=(proc.stderr, stderr_q), daemon=True).start()
    return proc, port, stderr_q


def _rpc(sock, method, params=None, rpc_id=1):
    msg = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
    if params is not None:
        msg["params"] = params
    sock.sendall((json.dumps(msg) + "\n").encode("utf-8"))
    buf = b""
    deadline = time.time() + 10
    while time.time() < deadline:
        chunk = sock.recv(65536)
        if not chunk:
            break
        buf += chunk
        if b"\n" in buf:
            break
    line, _, _ = buf.partition(b"\n")
    return json.loads(line.decode("utf-8"))


def _call(sock, tool, arguments, rpc_id):
    return _rpc(sock, "tools/call", {"name": tool, "arguments": arguments}, rpc_id=rpc_id)


def _drain_queue(q: queue.Queue) -> list:
    items = []
    while True:
        try:
            items.append(q.get_nowait())
        except queue.Empty:
            break
    return items


def main() -> int:
    proc, port, stderr_q = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []
            rpc_id = 1

            # Drain startup stderr before tool calls
            time.sleep(0.2)
            _drain_queue(stderr_q)

            # Discover tools
            tl = _rpc(s, "tools/list", rpc_id=rpc_id)
            rpc_id += 1
            tools = (tl.get("result") or {}).get("tools", [])
            tool_names = [t["name"] for t in tools]
            print(f"[tools/list] tools: {tool_names}")

            # Find parse-uri tool and its string argument name
            parse_tool = None
            parse_arg = "uri"
            for t in tools:
                if "parse" in t["name"] and "uri" in t["name"]:
                    parse_tool = t["name"]
                    props = t.get("inputSchema", {}).get("properties", {})
                    str_args = [k for k, v in props.items() if v.get("type") == "string"]
                    if str_args:
                        parse_arg = str_args[0]
                    break

            if parse_tool is None:
                failures.append("prereq: no parse-uri tool found in tools/list")
                print("[prereq] FAIL: no parse-uri tool found")
            else:
                print(f"[prereq] tool='{parse_tool}' arg='{parse_arg}'")

            if parse_tool:
                # 02.1 — parse_uri accepts a URI with a scheme and returns components
                r1 = _call(s, parse_tool, {parse_arg: "https://user:pass@example.com:8080/path/to/res?q=1#frag"}, rpc_id)
                rpc_id += 1
                result1 = r1.get("result")
                error1 = r1.get("error")
                print(f"[02.1] result={json.dumps(result1)} error={json.dumps(error1)}")
                if error1:
                    failures.append(f"02.1: JSON-RPC error for valid scheme URI: {error1}")
                elif result1 is None:
                    failures.append("02.1: null result for valid scheme URI")
                else:
                    # result may have an "error" discriminant when parsing fails
                    if isinstance(result1, dict) and result1.get("error") is not None:
                        failures.append(f"02.1: parse_uri returned error for valid URI: {result1['error']}")
                    else:
                        # scheme must be present; field name may vary in case
                        scheme1 = None
                        for k, v in (result1.items() if isinstance(result1, dict) else []):
                            if k.lower() == "scheme" and v:
                                scheme1 = v
                                break
                        if scheme1 is None:
                            failures.append(f"02.1: scheme absent in result: {result1}")
                        elif scheme1.lower() != "https":
                            failures.append(f"02.1: expected scheme 'https', got '{scheme1}'")
                        else:
                            print(f"[02.1] PASS scheme='{scheme1}'")
                        # path must be present (may be empty string, but key must exist)
                        has_path = any(k.lower() == "path" for k in (result1.keys() if isinstance(result1, dict) else []))
                        if not has_path:
                            failures.append(f"02.1: path field absent in result: {result1}")

                # 02.2 — parse_uri accepts a relative reference (no scheme)
                r2 = _call(s, parse_tool, {parse_arg: "/relative/path?q=1"}, rpc_id)
                rpc_id += 1
                result2 = r2.get("result")
                error2 = r2.get("error")
                print(f"[02.2] result={json.dumps(result2)} error={json.dumps(error2)}")
                if error2:
                    failures.append(f"02.2: JSON-RPC error for relative reference: {error2}")
                elif result2 is None:
                    failures.append("02.2: null result for relative reference")
                else:
                    if isinstance(result2, dict) and result2.get("error") is not None:
                        failures.append(f"02.2: parse_uri returned error for relative reference: {result2['error']}")
                    else:
                        # scheme must be absent (null/missing/empty) for a relative reference
                        scheme2 = None
                        if isinstance(result2, dict):
                            scheme2 = result2.get("scheme") or result2.get("Scheme")
                        if scheme2:
                            failures.append(f"02.2: relative reference should have no scheme, got '{scheme2}'")
                        else:
                            print(f"[02.2] PASS scheme absent for relative reference")
                        has_path2 = any(k.lower() == "path" for k in (result2.keys() if isinstance(result2, dict) else []))
                        if not has_path2:
                            failures.append(f"02.2: path field absent in relative reference result: {result2}")

                # 02.3 — parse_uri returns a UriParseError for malformed input
                r3 = _call(s, parse_tool, {parse_arg: "http://[::invalid"}, rpc_id)
                rpc_id += 1
                result3 = r3.get("result")
                error3 = r3.get("error")
                print(f"[02.3] result={json.dumps(result3)} error={json.dumps(error3)}")
                # error may surface as JSON-RPC error OR as a discriminated result field
                is_parse_error = (
                    error3 is not None
                    or (isinstance(result3, dict) and result3.get("error") is not None)
                )
                if not is_parse_error:
                    failures.append(f"02.3: expected UriParseError for malformed URI, got result={result3}")
                else:
                    print(f"[02.3] PASS: UriParseError returned for malformed URI")

                # 02.4 — UriParseError carries a message and offset
                if is_parse_error:
                    if error3 is not None:
                        # JSON-RPC error: message in error.message; offset may be in error.data
                        msg4 = error3.get("message", "")
                        data4 = error3.get("data") or {}
                        offset4 = data4.get("offset")
                    else:
                        # Discriminated result: error object with message + offset
                        err_obj = result3.get("error") or {}
                        msg4 = err_obj.get("message", "")
                        offset4 = err_obj.get("offset")
                    print(f"[02.4] message='{msg4}' offset={offset4}")
                    if not msg4:
                        failures.append(f"02.4: UriParseError has no message: {r3}")
                    if offset4 is None:
                        failures.append(f"02.4: UriParseError has no offset: {r3}")
                    if msg4 and offset4 is not None:
                        print(f"[02.4] PASS: message and offset present")
                else:
                    failures.append("02.4: cannot check UriParseError fields — 02.3 did not produce an error")

            # 02.5 — parse_uri does not write to stdout or stderr
            # Any stderr appearing after startup drain (above) was written during tool calls.
            time.sleep(0.1)
            stderr_during_calls = _drain_queue(stderr_q)
            print(f"[02.5] stderr lines during tool calls: {len(stderr_during_calls)}")
            if stderr_during_calls:
                failures.append(f"02.5: stderr written during parse_uri calls: {stderr_during_calls[:3]}")
            else:
                print(f"[02.5] PASS: no stderr output during parse_uri calls")

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
