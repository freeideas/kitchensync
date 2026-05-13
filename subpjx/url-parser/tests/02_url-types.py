#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercises accepted URL shapes and bare-path-to-file:// conversion (reqs 02.10–02.16)."""

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


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []
            rpc_id = 1

            tl = _rpc(s, "tools/list", rpc_id=rpc_id)
            rpc_id += 1
            tools = (tl.get("result") or {}).get("tools", [])
            tool_names = [t["name"] for t in tools]
            print(f"[tools/list] {len(tools)} tool(s): {tool_names}")

            parse_tool = next((t for t in tools if t["name"] == "parse"), None)
            if parse_tool is None:
                failures.append("parse tool not found in tools/list")
                print("\nFAILURES:")
                for f in failures:
                    print(f"  - {f}")
                return 1

            # Discover actual argument name for default_user (camelCase or snake_case)
            props = (parse_tool.get("inputSchema") or {}).get("properties", {})
            if "defaultUser" in props:
                du_key = "defaultUser"
            elif "default_user" in props:
                du_key = "default_user"
            else:
                required = (parse_tool.get("inputSchema") or {}).get("required", [])
                candidates = [k for k in required if k not in ("text", "cwd")]
                du_key = candidates[0] if candidates else "defaultUser"

            def call_parse(text, cwd="/tmp", default_user="testuser"):
                nonlocal rpc_id
                resp = _rpc(s, "tools/call", {
                    "name": "parse",
                    "arguments": {"text": text, "cwd": cwd, du_key: default_user},
                }, rpc_id=rpc_id)
                rpc_id += 1
                return resp

            def get_urls(resp):
                content = (resp.get("result") or {}).get("content", [])
                if not content:
                    return []
                text = content[0].get("text", "") if content else ""
                return json.loads(text).get("urls", []) if text else []

            # 02.10 — file:// URI accepted; scheme="file"
            resp = call_parse("file:///abs/path")
            urls = get_urls(resp)
            scheme = urls[0].get("scheme") if urls else None
            print(f"[02.10] file:///abs/path → scheme={scheme!r}")
            if scheme != "file":
                failures.append(f"02.10: expected scheme='file', got {scheme!r}")

            # 02.11 — sftp:// URI accepted; scheme="sftp"
            resp = call_parse("sftp://host.example/photos")
            urls = get_urls(resp)
            scheme = urls[0].get("scheme") if urls else None
            print(f"[02.11] sftp://host.example/photos → scheme={scheme!r}")
            if scheme != "sftp":
                failures.append(f"02.11: expected scheme='sftp', got {scheme!r}")

            # 02.12 — bare absolute path accepted; scheme="file"
            resp = call_parse("/abs/path")
            urls = get_urls(resp)
            scheme = urls[0].get("scheme") if urls else None
            print(f"[02.12] /abs/path → scheme={scheme!r}")
            if scheme != "file":
                failures.append(f"02.12: expected scheme='file' for bare path, got {scheme!r}")

            # 02.13 — Windows drive letter: path includes drive letter as /c:/foo
            resp = call_parse("c:/foo")
            urls = get_urls(resp)
            path = urls[0].get("path") if urls else None
            print(f"[02.13] c:/foo → path={path!r}")
            if path != "/c:/foo":
                failures.append(f"02.13: expected path='/c:/foo', got {path!r}")

            # 02.14 — backslashes normalised to forward slashes
            resp = call_parse("c:\\foo\\bar")
            urls = get_urls(resp)
            path = urls[0].get("path") if urls else None
            print(f"[02.14] c:\\foo\\bar → path={path!r}")
            if path != "/c:/foo/bar":
                failures.append(f"02.14: expected path='/c:/foo/bar', got {path!r}")

            # 02.15 — relative bare path resolved against caller-supplied cwd
            resp = call_parse("./data", cwd="/home/u")
            urls = get_urls(resp)
            path = urls[0].get("path") if urls else None
            print(f"[02.15] ./data with cwd=/home/u → path={path!r}")
            if path != "/home/u/data":
                failures.append(f"02.15: expected path='/home/u/data', got {path!r}")

            # 02.16 — scheme other than file/sftp is rejected
            resp = call_parse("http://example.com/path")
            has_error = "error" in resp
            print(f"[02.16] http://example.com/path → rejected={has_error}")
            if not has_error:
                failures.append(
                    f"02.16: expected rejection for http:// scheme, got result={resp.get('result')!r}"
                )

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
