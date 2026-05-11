#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercises remove_dot_segments (03.10) and merge_paths (03.11) path utilities."""

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


def _text(resp):
    """Extract string result from a tools/call response."""
    r = resp.get("result") or {}
    content = r.get("content") or []
    if content:
        return content[0].get("text", "")
    if "result" in r:
        return str(r["result"])
    return str(r)


def _find_tool(tools, *keywords):
    """Find a tool whose name contains all keywords (ignoring - and _)."""
    kws = [k.lower().replace("-", "").replace("_", "") for k in keywords]
    for t in tools:
        name = t["name"].lower().replace("-", "").replace("_", "")
        if all(k in name for k in kws):
            return t
    return None


def _param_names(tool):
    schema = tool.get("inputSchema") or tool.get("input_schema") or {}
    return list((schema.get("properties") or {}).keys())


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []
            rid = 1

            tl = _rpc(s, "tools/list", rpc_id=rid); rid += 1
            tools = (tl.get("result") or {}).get("tools", [])
            print(f"[info] tools: {[t['name'] for t in tools]}")

            rds = _find_tool(tools, "remove", "dot", "segments")
            mp  = _find_tool(tools, "merge", "path")

            # --- 03.10: remove_dot_segments (RFC 3986 §5.2.4) ---
            if rds is None:
                failures.append("03.10: remove-dot-segments tool not found in tools/list")
            else:
                pnames = _param_names(rds)
                p0 = pnames[0] if pnames else "path"
                cases = [
                    ("/a/b/c/./d",        "/a/b/c/d"),   # single dot collapsed
                    ("/a/b/c/../d",        "/a/b/d"),     # double dot pops segment
                    ("mid/content=5/../6", "mid/6"),      # relative path with ..
                    ("/a/b/c",             "/a/b/c"),     # no dots — no change
                    (".",                  ""),            # lone dot removed
                    ("..",                 ""),            # lone double-dot removed
                ]
                all_ok = True
                for path_in, expected in cases:
                    r = _rpc(s, "tools/call",
                             {"name": rds["name"], "arguments": {p0: path_in}},
                             rpc_id=rid); rid += 1
                    actual = _text(r)
                    ok = actual == expected
                    if not ok:
                        failures.append(
                            f"03.10: remove_dot_segments({path_in!r}) "
                            f"expected {expected!r}, got {actual!r}"
                        )
                        all_ok = False
                print(f"[03.10] remove_dot_segments: {'PASS' if all_ok else 'FAIL'}")

            # --- 03.11: merge_paths (RFC 3986 §5.2.3) ---
            if mp is None:
                failures.append("03.11: merge-paths tool not found in tools/list")
            else:
                pnames = _param_names(mp)
                if len(pnames) >= 3:
                    p_base, p_ref, p_auth = pnames[0], pnames[1], pnames[2]
                else:
                    p_base, p_ref, p_auth = "basePath", "refPath", "baseHasAuthority"
                cases = [
                    # (base_path, ref_path, base_has_authority, expected)
                    # authority + empty base path → "/" prepended
                    ("",       "g", True,  "/g"),
                    # authority + non-empty base path → up-to-last-slash + ref
                    ("/b/c/d", "g", True,  "/b/c/g"),
                    # no authority, non-empty base path
                    ("/b/c/d", "g", False, "/b/c/g"),
                    # base path already ends with /
                    ("/b/c/",  "d", False, "/b/c/d"),
                ]
                all_ok = True
                for base, ref, auth, expected in cases:
                    r = _rpc(s, "tools/call",
                             {"name": mp["name"],
                              "arguments": {p_base: base, p_ref: ref, p_auth: auth}},
                             rpc_id=rid); rid += 1
                    actual = _text(r)
                    ok = actual == expected
                    if not ok:
                        failures.append(
                            f"03.11: merge_paths({base!r},{ref!r},{auth}) "
                            f"expected {expected!r}, got {actual!r}"
                        )
                        all_ok = False
                print(f"[03.11] merge_paths: {'PASS' if all_ok else 'FAIL'}")

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
