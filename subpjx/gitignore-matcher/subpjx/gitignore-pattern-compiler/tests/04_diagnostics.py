#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercises req 04: diagnostics for lines that fail to compile."""

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
    for c in candidates:
        if c in tool_names:
            return c
    return None


def _get(obj, *keys):
    for k in keys:
        if isinstance(obj, dict) and k in obj:
            return obj[k]
    return None


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []

            tl = _rpc(s, "tools/list", rpc_id=1)
            tools = (tl.get("result") or {}).get("tools", [])
            tool_names = {t["name"] for t in tools}

            t_compile = _find_tool(tool_names, "compile_patterns", "compilePatterns")
            t_count   = _find_tool(tool_names, "pattern_count", "patternCount")

            if t_compile is None:
                failures.append("compile_patterns tool not found in tools/list")
                print("[04.x] compile_patterns tool missing — cannot exercise any req")
            else:
                # --- 04.1 + 04.4: unclosed character class → result returned, diagnostics entry added ---
                # Input: two valid patterns around one unclosed class; no exception expected.
                r1 = _call(s, t_compile, {"text": "*.txt\n[abc\n*.md\n"}, rpc_id=2)
                ps1   = _get(r1, "patternSet", "pattern_set")
                diag1 = _get(r1, "diagnostics")

                print(f"[04.4] compile_patterns with unclosed class returned result (no error)")
                # 04.4 is satisfied: _call would have raised on an RPC error response.

                n_diag1 = len(diag1) if isinstance(diag1, list) else 0
                print(f"[04.1] diagnostics entries for unclosed char class: {n_diag1}")
                if n_diag1 == 0:
                    failures.append(
                        "04.1: expected ≥1 diagnostics entry for unclosed character class, got 0")

                # --- 04.3: each diagnostics entry exposes line_number, line_text, reason ---
                if isinstance(diag1, list):
                    for i, d in enumerate(diag1):
                        ln  = _get(d, "line_number", "lineNumber")
                        lt  = _get(d, "line_text",   "lineText")
                        rsn = _get(d, "reason")
                        ok  = ln is not None and lt is not None and rsn is not None
                        print(f"[04.3] diagnostics[{i}] fields ok={ok} "
                              f"(line_number={ln!r}, line_text={lt!r}, reason={rsn!r})")
                        if not ok:
                            failures.append(
                                f"04.3: diagnostics[{i}] missing required field(s); "
                                f"got keys: {list(d.keys()) if isinstance(d, dict) else d}")
                else:
                    failures.append(f"04.3: diagnostics is not a list: {diag1!r}")

                # --- 04.5: valid lines before and after the bad line still compile ---
                # Input had *.txt (line 1), [abc (line 2, bad), *.md (line 3) → expect 2 patterns.
                if t_count is not None and ps1 is not None:
                    rc1 = _call(s, t_count, {"set": ps1}, rpc_id=3)
                    n1 = rc1 if isinstance(rc1, int) else _get(rc1, "count", "value", "n")
                    print(f"[04.5] pattern_count from input with 2 valid + 1 bad line: {n1!r}")
                    if n1 != 2:
                        failures.append(
                            f"04.5: expected pattern_count 2 (valid lines before/after bad), "
                            f"got {n1!r}")
                elif ps1 is None:
                    failures.append("04.5: compile_patterns result missing patternSet/pattern_set")
                    print("[04.5] patternSet missing in result")
                else:
                    # pattern_count tool unavailable — count not verifiable this way
                    print("[04.5] pattern_count tool not found; skipping count assertion")

                # --- 04.2: trailing backslash with nothing to escape → diagnostics entry + omitted from PatternSet ---
                r2 = _call(s, t_compile, {"text": "foo\\\n"}, rpc_id=4)
                diag2 = _get(r2, "diagnostics")
                ps2   = _get(r2, "patternSet", "pattern_set")
                n_diag2 = len(diag2) if isinstance(diag2, list) else 0
                print(f"[04.2] diagnostics entries for trailing backslash: {n_diag2}")
                if n_diag2 == 0:
                    failures.append(
                        "04.2: expected ≥1 diagnostics entry for trailing backslash, got 0")

                if t_count is not None and ps2 is not None:
                    rc2 = _call(s, t_count, {"set": ps2}, rpc_id=5)
                    n2 = rc2 if isinstance(rc2, int) else _get(rc2, "count", "value", "n")
                    print(f"[04.2] pattern_count from input with only bad line: {n2!r}")
                    if n2 != 0:
                        failures.append(
                            f"04.2: expected pattern_count 0 (bad line omitted from PatternSet), "
                            f"got {n2!r}")
                elif ps2 is None:
                    failures.append("04.2: compile_patterns result missing patternSet/pattern_set")
                    print("[04.2] patternSet missing in result")
                else:
                    print("[04.2] pattern_count tool not found; skipping omission count assertion")

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
