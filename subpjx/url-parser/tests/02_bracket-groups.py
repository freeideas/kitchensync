#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Bracket-group parsing: multi-URL groups, role tags on groups, and rejection of malformed forms."""

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


def _parse(sock, text, rpc_id):
    return _rpc(sock, "tools/call", {
        "name": "parse",
        "arguments": {"text": text, "cwd": "/tmp", "default_user": "ace"},
    }, rpc_id)


def _is_error(resp):
    if "error" in resp:
        return True
    result = resp.get("result", {})
    if isinstance(result, dict) and result.get("isError"):
        return True
    return False


def _get_group(resp):
    """Extract the TaggedGroup dict from a successful tools/call response."""
    result = resp.get("result", {})
    if not isinstance(result, dict):
        return None
    for item in result.get("content", []):
        if isinstance(item, dict) and item.get("type") == "text":
            try:
                return json.loads(item["text"])
            except (json.JSONDecodeError, KeyError):
                return item.get("text")
    return None


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []
            rpc_id = 1

            # REQ 02.5: bracket group produces one ParsedUrl per inner URL in input order
            resp = _parse(s, "[file:///alpha,file:///beta]", rpc_id); rpc_id += 1
            group = _get_group(resp)
            urls = group.get("urls", []) if isinstance(group, dict) else []
            ok_05 = (
                not _is_error(resp)
                and len(urls) == 2
                and urls[0].get("path") == "/alpha"
                and urls[1].get("path") == "/beta"
            )
            print(f"[02.5] bracket group yields two ParsedUrls in input order: {'PASS' if ok_05 else 'FAIL'}")
            if not ok_05:
                failures.append(f"02.5: expected 2 urls with paths /alpha, /beta; got {group}")

            # REQ 02.6: leading role tag sets group role; inner URLs are unaffected
            resp = _parse(s, "+[file:///alpha,file:///beta]", rpc_id); rpc_id += 1
            group = _get_group(resp)
            urls6 = group.get("urls", []) if isinstance(group, dict) else []
            ok_06 = (
                not _is_error(resp)
                and isinstance(group, dict)
                and group.get("role") == "Canon"
                and len(urls6) == 2
                and urls6[0].get("path") == "/alpha"
                and urls6[1].get("path") == "/beta"
            )
            print(f"[02.6] leading role tag sets group role to Canon, inner URLs unchanged: {'PASS' if ok_06 else 'FAIL'}")
            if not ok_06:
                failures.append(f"02.6: expected role=Canon with paths /alpha, /beta; got {group}")

            # REQ 02.7: unclosed bracket group is rejected
            resp = _parse(s, "[file:///alpha,file:///beta", rpc_id); rpc_id += 1
            ok_07 = _is_error(resp)
            print(f"[02.7] unclosed bracket group is rejected: {'PASS' if ok_07 else 'FAIL'}")
            if not ok_07:
                failures.append(f"02.7: expected error for unclosed bracket group; got {resp}")

            # REQ 02.8: bracket group containing an empty URL is rejected
            resp = _parse(s, "[file:///alpha,,file:///beta]", rpc_id); rpc_id += 1
            ok_08 = _is_error(resp)
            print(f"[02.8] bracket group with empty inner URL is rejected: {'PASS' if ok_08 else 'FAIL'}")
            if not ok_08:
                failures.append(f"02.8: expected error for empty inner URL; got {resp}")

            # REQ 02.9: bracket group with a role-tagged inner URL is rejected
            resp = _parse(s, "[+file:///alpha,file:///beta]", rpc_id); rpc_id += 1
            ok_09 = _is_error(resp)
            print(f"[02.9] bracket group with role-tagged inner URL is rejected: {'PASS' if ok_09 else 'FAIL'}")
            if not ok_09:
                failures.append(f"02.9: expected error for role-tagged inner URL; got {resp}")

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
