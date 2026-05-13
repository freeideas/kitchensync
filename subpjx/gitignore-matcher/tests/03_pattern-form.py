#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Tests 03_pattern-form: anchoring and directory-only pattern restrictions (03.1–03.4)."""

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


def _call_tool(sock, name, args, rpc_id):
    resp = _rpc(sock, "tools/call", {"name": name, "arguments": args}, rpc_id)
    content = (resp.get("result") or {}).get("content", [])
    if content and content[0].get("type") == "text":
        try:
            return json.loads(content[0]["text"])
        except (json.JSONDecodeError, ValueError):
            return content[0]["text"]
    return resp.get("result")


def _is_ignored(result) -> bool:
    if isinstance(result, str):
        return result.strip().lower() == "ignored"
    if isinstance(result, dict):
        v = result.get("result") or result.get("value") or result.get("status") or ""
        return str(v).lower() == "ignored"
    return False


def _is_not_ignored(result) -> bool:
    if isinstance(result, str):
        return result.strip().lower() == "notignored"
    if isinstance(result, dict):
        v = result.get("result") or result.get("value") or result.get("status") or ""
        return str(v).lower() == "notignored"
    return False


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []
            rid = 1

            tl = _rpc(s, "tools/list", rpc_id=rid); rid += 1
            tool_names = [t["name"] for t in (tl.get("result") or {}).get("tools", [])]
            print(f"[info] discovered tools: {tool_names}")

            compile_tool = next((n for n in tool_names if "compile" in n.lower()), None)
            match_tool = next((n for n in tool_names if "match" in n.lower()), None)
            if not compile_tool or not match_tool:
                print(f"FATAL: compile or match tool not found in {tool_names}")
                return 1

            def compile_text(text):
                nonlocal rid
                r = _call_tool(s, compile_tool, {"text": text}, rid); rid += 1
                return r

            def do_match(stack, path, is_dir):
                nonlocal rid
                normalized = [{"scope": e[0], **e[1]} for e in stack]
                r = _call_tool(s, match_tool, {"stack": normalized, "relative_path": path, "is_directory": is_dir}, rid); rid += 1
                return r

            # --- 03.1: no-slash pattern matches at any depth ---
            p_no_slash = compile_text("*.txt")

            r = do_match([["", p_no_slash]], "file.txt", False)
            print(f"[03.1a] '*.txt' at scope '' matches 'file.txt' (shallow): {r}")
            if not _is_ignored(r):
                failures.append("03.1a: no-slash pattern '*.txt' must match at shallow depth")

            r = do_match([["", p_no_slash]], "a/b/c/file.txt", False)
            print(f"[03.1b] '*.txt' at scope '' matches 'a/b/c/file.txt' (deep): {r}")
            if not _is_ignored(r):
                failures.append("03.1b: no-slash pattern '*.txt' must match at any depth")

            # 03.1: trailing-slash-only pattern also matches at any depth (for directories)
            p_trailing_only = compile_text("logs/")

            r = do_match([["", p_trailing_only]], "a/b/logs", True)
            print(f"[03.1c] 'logs/' at scope '' matches dir 'a/b/logs' (deep): {r}")
            if not _is_ignored(r):
                failures.append("03.1c: trailing-slash-only 'logs/' must match dir at any depth")

            # --- 03.2: internal-slash pattern anchored at declaring scope ---
            p_internal = compile_text("src/foo.txt")

            r = do_match([["", p_internal]], "src/foo.txt", False)
            print(f"[03.2a] 'src/foo.txt' at scope '' matches 'src/foo.txt': {r}")
            if not _is_ignored(r):
                failures.append("03.2a: internal-slash 'src/foo.txt' must match 'src/foo.txt' at scope root")

            r = do_match([["", p_internal]], "dir/src/foo.txt", False)
            print(f"[03.2b] 'src/foo.txt' at scope '' does NOT match 'dir/src/foo.txt' (anchored): {r}")
            if not _is_not_ignored(r):
                failures.append("03.2b: anchored 'src/foo.txt' must NOT match 'dir/src/foo.txt'")

            # --- 03.3: leading-slash pattern anchored at scope's root ---
            # Contrast: 'foo.txt' (no slash) floats to any depth; '/foo.txt' does not.
            p_leading = compile_text("/foo.txt")

            r = do_match([["", p_leading]], "foo.txt", False)
            print(f"[03.3a] '/foo.txt' at scope '' matches 'foo.txt' (scope root): {r}")
            if not _is_ignored(r):
                failures.append("03.3a: leading-slash '/foo.txt' must match 'foo.txt' at scope root")

            r = do_match([["", p_leading]], "dir/foo.txt", False)
            print(f"[03.3b] '/foo.txt' at scope '' does NOT match 'dir/foo.txt': {r}")
            if not _is_not_ignored(r):
                failures.append("03.3b: leading-slash '/foo.txt' must NOT match 'dir/foo.txt'")

            # --- 03.4: trailing-slash restricts pattern to directories only ---
            p_dir_only = compile_text("build/")

            r = do_match([["", p_dir_only]], "build", True)
            print(f"[03.4a] 'build/' matches 'build' with is_directory=true: {r}")
            if not _is_ignored(r):
                failures.append("03.4a: trailing-slash 'build/' must match when is_directory=true")

            r = do_match([["", p_dir_only]], "build", False)
            print(f"[03.4b] 'build/' does NOT match 'build' with is_directory=false: {r}")
            if not _is_not_ignored(r):
                failures.append("03.4b: trailing-slash 'build/' must NOT match when is_directory=false")

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
