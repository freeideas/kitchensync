#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercises normalize_uri: scheme/host lowercasing, unreserved decode, hex uppercase, dot-segment removal (03.20–03.24)."""

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
        chunk = sock.recv(65536)
        if not chunk:
            break
        buf += chunk
        if b"\n" in buf:
            break
    line, _, _ = buf.partition(b"\n")
    return json.loads(line.decode("utf-8"))


def _unwrap(resp):
    """Extract payload from a tools/call JSON-RPC response."""
    r = resp.get("result")
    if isinstance(r, dict):
        content = r.get("content")
        if isinstance(content, list) and content:
            text = content[0].get("text", "")
            try:
                return json.loads(text)
            except (json.JSONDecodeError, ValueError):
                return text
        if "result" in r:
            return r["result"]
        if "value" in r:
            return r["value"]
    return r


def _get(obj, *keys):
    """Case-insensitive nested field access."""
    for key in keys:
        if not isinstance(obj, dict):
            return None
        low = {k.lower(): v for k, v in obj.items()}
        obj = low.get(key.lower())
    return obj


def _find_tool(tools, *keywords):
    kws = [k.lower().replace("-", "").replace("_", "") for k in keywords]
    for t in tools:
        name = t["name"].lower().replace("-", "").replace("_", "")
        if all(k in name for k in kws):
            return t
    return None


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []
            rid = 1

            tl = _rpc(s, "tools/list", rpc_id=rid); rid += 1
            tools = (tl.get("result") or {}).get("tools", [])
            print(f"[info] tools: {[t['name'] for t in tools]}")

            # Discover parse-uri tool and its string argument name
            parse_tool = _find_tool(tools, "parse", "uri")
            parse_arg = "uri"
            if parse_tool:
                props = (parse_tool.get("inputSchema") or {}).get("properties") or {}
                str_args = [k for k, v in props.items() if v.get("type") == "string"]
                if str_args:
                    parse_arg = str_args[0]

            # Discover normalize-uri tool
            norm_tool = _find_tool(tools, "normalize")
            if norm_tool is None:
                failures.append("prereq: no normalize-uri tool found in tools/list")
                print("[prereq] FAIL: no normalize-uri tool found")
                print("\nFAILURES:")
                for f in failures:
                    print(f"  - {f}")
                return 1

            norm_name = norm_tool["name"]
            norm_props = (norm_tool.get("inputSchema") or {}).get("properties") or {}
            norm_arg = list(norm_props.keys())[0] if norm_props else "uri"
            norm_type = (norm_props.get(norm_arg) or {}).get("type", "object")
            print(f"[info] normalize: tool='{norm_name}' arg='{norm_arg}' type='{norm_type}'")

            def call_parse(uri_str):
                nonlocal rid
                if parse_tool is None:
                    return None
                resp = _rpc(s, "tools/call",
                            {"name": parse_tool["name"], "arguments": {parse_arg: uri_str}},
                            rpc_id=rid)
                rid += 1
                return _unwrap(resp)

            def call_normalize(uri_str):
                nonlocal rid
                if norm_type == "string":
                    arg_val = uri_str
                else:
                    arg_val = call_parse(uri_str)
                    if not isinstance(arg_val, dict):
                        return None
                resp = _rpc(s, "tools/call",
                            {"name": norm_name, "arguments": {norm_arg: arg_val}},
                            rpc_id=rid)
                rid += 1
                return _unwrap(resp)

            def get_host(r):
                """Try authority.host first, then flat host."""
                h = _get(r, "authority", "host")
                if h is None:
                    h = _get(r, "host")
                return h

            def get_userinfo(r):
                """Try authority.userinfo first, then flat userinfo."""
                u = _get(r, "authority", "userinfo")
                if u is None:
                    u = _get(r, "userinfo")
                return u

            # --- 03.20: normalize_uri lowercases the scheme ---
            r20 = call_normalize("HTTP://example.com/path")
            scheme20 = _get(r20, "scheme") if isinstance(r20, dict) else None
            ok20 = scheme20 == "http"
            if not ok20:
                failures.append(f"03.20: expected scheme='http', got {scheme20!r}")
            print(f"[03.20] lowercase scheme: {'PASS' if ok20 else 'FAIL'} scheme={scheme20!r}")

            # --- 03.21: normalize_uri lowercases the host ---
            r21 = call_normalize("http://EXAMPLE.COM/path")
            host21 = get_host(r21) if isinstance(r21, dict) else None
            ok21 = host21 == "example.com"
            if not ok21:
                failures.append(f"03.21: expected host='example.com', got {host21!r}")
            print(f"[03.21] lowercase host: {'PASS' if ok21 else 'FAIL'} host={host21!r}")

            # --- 03.22: normalize_uri decodes unreserved percent-encoded characters ---
            ok22 = True

            # path: %41='A' and %7E='~' are unreserved, must be decoded
            r22p = call_normalize("http://example.com/%41%7E")
            path22 = _get(r22p, "path") if isinstance(r22p, dict) else None
            if path22 is None:
                failures.append("03.22 path: no path in result")
                ok22 = False
            elif "%41" in path22 or "%7E" in path22 or "%7e" in path22:
                failures.append(f"03.22 path: unreserved %41/%7E not decoded; path={path22!r}")
                ok22 = False
            elif "A~" not in path22:
                failures.append(f"03.22 path: expected 'A~' in decoded path, got {path22!r}")
                ok22 = False

            # query: %61='a' and %62='b' are unreserved, must be decoded
            r22q = call_normalize("http://example.com/?%61=%62")
            query22 = _get(r22q, "query") if isinstance(r22q, dict) else None
            if query22 is None:
                failures.append("03.22 query: no query in result")
                ok22 = False
            elif "%61" in query22 or "%62" in query22:
                failures.append(f"03.22 query: unreserved %61/%62 not decoded; query={query22!r}")
                ok22 = False

            # fragment: %7E='~' is unreserved, must be decoded
            r22f = call_normalize("http://example.com/#%7E")
            frag22 = _get(r22f, "fragment") if isinstance(r22f, dict) else None
            if frag22 is None:
                failures.append("03.22 fragment: no fragment in result")
                ok22 = False
            elif "%7E" in frag22 or "%7e" in frag22:
                failures.append(f"03.22 fragment: unreserved %7E not decoded; fragment={frag22!r}")
                ok22 = False
            elif frag22 != "~":
                failures.append(f"03.22 fragment: expected '~', got {frag22!r}")
                ok22 = False

            # host (reg-name): %6d='m' is unreserved, must be decoded
            r22h = call_normalize("http://exa%6dple.com/")
            host22h = get_host(r22h) if isinstance(r22h, dict) else None
            if host22h is None:
                failures.append("03.22 host: no host in result")
                ok22 = False
            elif "%6d" in host22h or "%6D" in host22h:
                failures.append(f"03.22 host: unreserved %6d not decoded; host={host22h!r}")
                ok22 = False
            elif host22h != "example.com":
                failures.append(f"03.22 host: expected 'example.com', got {host22h!r}")
                ok22 = False

            # userinfo: %75='u' is unreserved, must be decoded
            r22u = call_normalize("http://%75ser@example.com/")
            ui22 = get_userinfo(r22u) if isinstance(r22u, dict) else None
            if ui22 is None:
                failures.append("03.22 userinfo: no userinfo in result")
                ok22 = False
            else:
                ui_str = ui22 if isinstance(ui22, str) else json.dumps(ui22)
                if "%75" in ui_str:
                    failures.append(f"03.22 userinfo: unreserved %75 not decoded; userinfo={ui22!r}")
                    ok22 = False
                elif "user" not in ui_str:
                    failures.append(f"03.22 userinfo: expected 'user' in decoded userinfo, got {ui22!r}")
                    ok22 = False

            print(f"[03.22] decode unreserved percent-encoded: {'PASS' if ok22 else 'FAIL'}")

            # --- 03.23: normalize_uri uppercases hex digits of remaining percent-encoded triplets ---
            ok23 = True

            # path: %2f (slash, reserved) must become %2F
            r23p = call_normalize("http://example.com/path%2fmore")
            path23 = _get(r23p, "path") if isinstance(r23p, dict) else None
            if path23 is None:
                failures.append("03.23 path: no path in result")
                ok23 = False
            elif "%2f" in path23:
                failures.append(f"03.23 path: lowercase hex not uppercased; path={path23!r}")
                ok23 = False
            elif "%2F" not in path23:
                failures.append(f"03.23 path: expected %2F in path, got {path23!r}")
                ok23 = False

            # query: %3a (colon, reserved) must become %3A
            r23q = call_normalize("http://example.com/?key%3aval")
            query23 = _get(r23q, "query") if isinstance(r23q, dict) else None
            if query23 is None:
                failures.append("03.23 query: no query in result")
                ok23 = False
            elif "%3a" in query23:
                failures.append(f"03.23 query: lowercase hex not uppercased; query={query23!r}")
                ok23 = False
            elif "%3A" not in query23:
                failures.append(f"03.23 query: expected %3A in query, got {query23!r}")
                ok23 = False

            print(f"[03.23] uppercase remaining percent-encoded hex: {'PASS' if ok23 else 'FAIL'}")

            # --- 03.24: normalize_uri applies remove_dot_segments to the path ---
            ok24 = True
            dot_cases = [
                ("http://example.com/a/b/../c",      "/a/c"),
                ("http://example.com/a/./b",         "/a/b"),
                ("http://example.com/a/b/c/../../d", "/a/d"),
            ]
            for uri_in, expected in dot_cases:
                r24 = call_normalize(uri_in)
                path24 = _get(r24, "path") if isinstance(r24, dict) else None
                if path24 != expected:
                    failures.append(
                        f"03.24: normalize({uri_in!r}) expected path={expected!r}, got {path24!r}"
                    )
                    ok24 = False
            print(f"[03.24] remove_dot_segments on path: {'PASS' if ok24 else 'FAIL'}")

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
