#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Tests the public compilation API and PatternSet container (01_api-shape)."""

from __future__ import annotations

import json, os, socket, subprocess, sys, threading, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY",
                               "./aitc/languages/java/build.py"))
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
        chunk = sock.recv(65536)
        if not chunk:
            break
        buf += chunk
        if b"\n" in buf:
            break
    line, _, _ = buf.partition(b"\n")
    return json.loads(line.decode("utf-8"))


def _call(sock, tool, arguments=None, rpc_id=1):
    resp = _rpc(sock, "tools/call",
                {"name": tool, "arguments": arguments or {}},
                rpc_id=rpc_id)
    if "error" in resp:
        raise RuntimeError(f"Tool '{tool}' RPC error: {resp['error']}")
    content = (resp.get("result") or {}).get("content", [])
    text = next((c["text"] for c in content if c.get("type") == "text"), None)
    if text is None:
        raise RuntimeError(f"Tool '{tool}' returned no text content; resp={resp}")
    return json.loads(text)


def _find_tool(tool_names, *candidates):
    """Return the first candidate name present in tool_names, or None."""
    for c in candidates:
        if c in tool_names:
            return c
    return None


def _get(obj, *keys):
    """Return obj[key] for the first key present (camelCase / snake_case variants)."""
    for k in keys:
        if isinstance(obj, dict) and k in obj:
            return obj[k]
    return None


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []

            # Discover available tools and resolve name variants once.
            tl = _rpc(s, "tools/list", rpc_id=1)
            tools = (tl.get("result") or {}).get("tools", [])
            tool_names = {t["name"] for t in tools}

            t_compile = _find_tool(tool_names, "compile_patterns", "compilePatterns")
            t_empty   = _find_tool(tool_names, "empty_pattern_set", "emptyPatternSet")
            t_count   = _find_tool(tool_names, "pattern_count", "patternCount")
            t_at      = _find_tool(tool_names, "pattern_at", "patternAt")

            # --- 01.1: compile_patterns(text) returns a (PatternSet, Diagnostics) pair ---
            if t_compile is None:
                failures.append("01.1: compile_patterns tool not found in tools/list")
                print("[01.1] compile_patterns tool missing")
            else:
                r1 = _call(s, t_compile, {"text": "*.txt\n!important.txt"}, rpc_id=2)
                ps1 = _get(r1, "patternSet", "pattern_set")
                diag1 = _get(r1, "diagnostics")
                print(f"[01.1] compile_patterns result keys: "
                      f"{list(r1.keys()) if isinstance(r1, dict) else type(r1).__name__}")
                if ps1 is None:
                    failures.append("01.1: compile_patterns result missing patternSet/pattern_set")
                if diag1 is None:
                    failures.append("01.1: compile_patterns result missing diagnostics")

            # --- 01.2: empty_pattern_set() -> PatternSet with pattern_count == 0 ---
            if t_empty is None or t_count is None:
                failures.append("01.2: empty_pattern_set or pattern_count tool not found")
                print("[01.2] required tools missing")
            else:
                r2 = _call(s, t_empty, {}, rpc_id=3)
                # empty_pattern_set returns the PatternSet directly
                empty_ps = _get(r2, "patternSet", "pattern_set") or r2
                r2c = _call(s, t_count, {"set": empty_ps}, rpc_id=4)
                n2 = r2c if isinstance(r2c, int) else _get(r2c, "count", "value", "n")
                print(f"[01.2] empty_pattern_set -> pattern_count = {n2!r}")
                if n2 != 0:
                    failures.append(f"01.2: expected pattern_count 0 for empty set, got {n2!r}")

            # --- 01.3: pattern_count(set) returns number of compiled patterns ---
            if t_compile is None or t_count is None:
                failures.append("01.3: compile_patterns or pattern_count tool not found")
                print("[01.3] required tools missing")
            else:
                r3 = _call(s, t_compile, {"text": "*.txt\n*.py\n*.js"}, rpc_id=5)
                ps3 = _get(r3, "patternSet", "pattern_set")
                r3c = _call(s, t_count, {"set": ps3}, rpc_id=6)
                n3 = r3c if isinstance(r3c, int) else _get(r3c, "count", "value", "n")
                print(f"[01.3] compile_patterns('*.txt\\n*.py\\n*.js') -> pattern_count = {n3!r}")
                if n3 != 3:
                    failures.append(f"01.3: expected pattern_count 3, got {n3!r}")

            # --- 01.4: pattern_at(set, index) returns CompiledPattern in source order ---
            if t_compile is None or t_at is None:
                failures.append("01.4: compile_patterns or pattern_at tool not found")
                print("[01.4] required tools missing")
            else:
                r4 = _call(s, t_compile, {"text": "alpha\nbeta"}, rpc_id=7)
                ps4 = _get(r4, "patternSet", "pattern_set")
                cp0 = _call(s, t_at, {"set": ps4, "index": 0}, rpc_id=8)
                cp1 = _call(s, t_at, {"set": ps4, "index": 1}, rpc_id=9)
                src0 = _get(cp0, "source") if isinstance(cp0, dict) else None
                src1 = _get(cp1, "source") if isinstance(cp1, dict) else None
                print(f"[01.4] pattern_at(0).source={src0!r}, pattern_at(1).source={src1!r}")
                if src0 != "alpha":
                    failures.append(
                        f"01.4: expected pattern_at(0).source='alpha', got {src0!r}")
                if src1 != "beta":
                    failures.append(
                        f"01.4: expected pattern_at(1).source='beta', got {src1!r}")

            # --- 01.5: CompiledPattern.source is post-whitespace-strip, pre-flag-consumption ---
            # "!*.txt   " -> strip trailing spaces -> "!*.txt" (! flag not yet consumed)
            if t_compile is None or t_at is None:
                failures.append("01.5: compile_patterns or pattern_at tool not found")
                print("[01.5] required tools missing")
            else:
                r5 = _call(s, t_compile, {"text": "!*.txt   "}, rpc_id=10)
                ps5 = _get(r5, "patternSet", "pattern_set")
                cp5 = _call(s, t_at, {"set": ps5, "index": 0}, rpc_id=11)
                src5 = _get(cp5, "source") if isinstance(cp5, dict) else None
                print(f"[01.5] CompiledPattern.source for '!*.txt   ' = {src5!r}")
                if src5 != "!*.txt":
                    failures.append(
                        f"01.5: expected source='!*.txt' (trailing stripped, ! preserved), "
                        f"got {src5!r}")

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
