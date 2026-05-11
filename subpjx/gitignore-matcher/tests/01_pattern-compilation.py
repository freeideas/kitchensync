#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercises req 01_pattern-compilation via the gitignore-matcher MCP wrapper."""

from __future__ import annotations

import json, os, socket, subprocess, sys, threading, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

_rpc_id = 0


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


def _rpc(sock, method, params=None):
    global _rpc_id
    _rpc_id += 1
    msg = {"jsonrpc": "2.0", "id": _rpc_id, "method": method}
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


def _call(sock, tool, arguments=None):
    """Call an MCP tool and return the unwrapped result dict."""
    resp = _rpc(sock, "tools/call", {"name": tool, "arguments": arguments or {}})
    if resp.get("error"):
        raise RuntimeError(f"{tool!r} RPC error: {resp['error']}")
    result = resp.get("result") or {}
    # Standard MCP content format
    content = result.get("content", [])
    if content:
        text = next((c["text"] for c in content if c.get("type") == "text"), None)
        if text:
            return json.loads(text)
    # Direct result format
    return result


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


def _compile(sock, t_compile, text):
    """Call compile_patterns and return (pattern_set, diagnostics)."""
    r = _call(sock, t_compile, {"text": text})
    ps = _get(r, "patternSet", "pattern_set")
    diag = _get(r, "diagnostics")
    return ps, diag


def _empty_matcher(sock, t_empty):
    r = _call(sock, t_empty)
    m = _get(r, "matcher")
    return m if m is not None else r


def _push(sock, t_push, matcher, scope_dir, pattern_set):
    r = _call(sock, t_push, {
        "matcher": matcher,
        "scope_dir": scope_dir,
        "pattern_set": pattern_set,
    })
    m = _get(r, "matcher")
    return m if m is not None else r


def _is_ignored(sock, t_ignored, matcher, path, is_dir):
    r = _call(sock, t_ignored, {"matcher": matcher, "path": path, "is_dir": is_dir})
    if isinstance(r, bool):
        return r
    for key in ("ignored", "is_ignored"):
        if isinstance(r, dict) and key in r:
            return r[key]
    raise RuntimeError(f"is-ignored result unrecognized: {r!r}")


def _make_matcher(sock, t_empty, t_push, t_compile, text):
    """Compile text, push scope into empty matcher, return (matcher, ps, diag)."""
    ps, diag = _compile(sock, t_compile, text)
    em = _empty_matcher(sock, t_empty)
    m = _push(sock, t_push, em, "", ps if ps is not None else [])
    return m, ps, diag


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []

            tl = _rpc(s, "tools/list")
            tools = (tl.get("result") or {}).get("tools", [])
            tool_names = {t["name"] for t in tools}
            print(f"[setup] tools/list returned {len(tools)} tool(s): {sorted(tool_names)}")

            t_compile = _find_tool(tool_names,
                                   "compile-patterns", "compile_patterns", "compilePatterns")
            t_empty   = _find_tool(tool_names,
                                   "empty-matcher", "empty_matcher", "emptyMatcher")
            t_push    = _find_tool(tool_names,
                                   "push-scope", "push_scope", "pushScope")
            t_ignored = _find_tool(tool_names,
                                   "is-ignored", "is_ignored", "isIgnored")

            missing = [n for n, t in [
                ("compile_patterns", t_compile),
                ("empty_matcher",    t_empty),
                ("push_scope",       t_push),
                ("is_ignored",       t_ignored),
            ] if t is None]
            if missing:
                for n in missing:
                    failures.append(f"setup: required tool not found: {n}")
                    print(f"[setup] required tool missing: {n}")
                # Cannot exercise further reqs without these tools.
                print("\nFAILURES:")
                for f in failures:
                    print(f"  - {f}")
                return 1

            # --- 01.1: blank lines contribute no pattern ---
            try:
                m, ps, _ = _make_matcher(s, t_empty, t_push, t_compile, "\n\n\n")
                result = _is_ignored(s, t_ignored, m, "some/file.txt", False)
                print(f"[01.1] blank-only text, is_ignored('some/file.txt') = {result!r}")
                if result is not False:
                    failures.append(
                        f"01.1: blank lines should contribute no pattern; "
                        f"is_ignored returned {result!r} instead of False")
            except Exception as e:
                failures.append(f"01.1: {e}")
                print(f"[01.1] exception: {e}")

            # --- 01.2: lines beginning with # are comments ---
            try:
                m, ps, _ = _make_matcher(s, t_empty, t_push, t_compile,
                                         "# comment\n# another comment\n")
                result = _is_ignored(s, t_ignored, m, "some/file.txt", False)
                print(f"[01.2] comment-only text, is_ignored('some/file.txt') = {result!r}")
                if result is not False:
                    failures.append(
                        f"01.2: comment lines should contribute no pattern; "
                        f"is_ignored returned {result!r} instead of False")
            except Exception as e:
                failures.append(f"01.2: {e}")
                print(f"[01.2] exception: {e}")

            # --- 01.3: unescaped trailing whitespace stripped ---
            # "*.txt   " should parse identically to "*.txt" and match file.txt
            try:
                m, ps, _ = _make_matcher(s, t_empty, t_push, t_compile, "*.txt   \n")
                result = _is_ignored(s, t_ignored, m, "file.txt", False)
                print(f"[01.3] '*.txt   ' (trailing spaces), is_ignored('file.txt') = {result!r}")
                if result is not True:
                    failures.append(
                        f"01.3: unescaped trailing whitespace should be stripped so "
                        f"'*.txt   ' matches 'file.txt'; got {result!r}")
            except Exception as e:
                failures.append(f"01.3: {e}")
                print(f"[01.3] exception: {e}")

            # --- 01.4: trailing whitespace escaped with backslash is preserved ---
            # Pattern "foo\ " (backslash then space) preserves the trailing space.
            # The compiled pattern body is "foo " and should match path "foo " but not "foo".
            try:
                # Python string "foo\\ " represents file text "foo\ " (backslash + space)
                m_esc, _, _ = _make_matcher(s, t_empty, t_push, t_compile, "foo\\ \n")
                hit  = _is_ignored(s, t_ignored, m_esc, "foo ", False)
                miss = _is_ignored(s, t_ignored, m_esc, "foo",  False)
                print(f"[01.4] 'foo\\ ' (escaped space): "
                      f"is_ignored('foo ')={hit!r}, is_ignored('foo')={miss!r}")
                if hit is not True:
                    failures.append(
                        f"01.4: escaped trailing space preserved as part of pattern; "
                        f"'foo ' should be ignored, got {hit!r}")
                if miss is not False:
                    failures.append(
                        f"01.4: pattern 'foo ' (with space) should not match 'foo' "
                        f"(without space); got {miss!r}")
            except Exception as e:
                failures.append(f"01.4: {e}")
                print(f"[01.4] exception: {e}")

            # --- 01.5: malformed pattern line produces no pattern ---
            # An unclosed character class "[abc" is malformed; it should not match anything.
            try:
                m, ps, _ = _make_matcher(s, t_empty, t_push, t_compile, "[abc\n")
                result = _is_ignored(s, t_ignored, m, "abc", False)
                print(f"[01.5] '[abc' (malformed), is_ignored('abc') = {result!r}")
                if result is not False:
                    failures.append(
                        f"01.5: malformed pattern should produce no pattern; "
                        f"is_ignored returned {result!r} instead of False")
            except Exception as e:
                failures.append(f"01.5: {e}")
                print(f"[01.5] exception: {e}")

            # --- 01.6: remaining lines still compile when one line is malformed ---
            # "[abc" is malformed; "*.txt" is valid and should still match file.txt.
            try:
                m, ps, _ = _make_matcher(s, t_empty, t_push, t_compile, "[abc\n*.txt\n")
                result = _is_ignored(s, t_ignored, m, "file.txt", False)
                print(f"[01.6] '[abc\\n*.txt', is_ignored('file.txt') = {result!r}")
                if result is not True:
                    failures.append(
                        f"01.6: valid lines after a malformed line should still compile; "
                        f"'file.txt' should be ignored by '*.txt', got {result!r}")
            except Exception as e:
                failures.append(f"01.6: {e}")
                print(f"[01.6] exception: {e}")

            # --- 01.7: compilation returns diagnostics identifying skipped malformed lines ---
            try:
                ps, diag = _compile(s, t_compile, "[unclosed\n")
                print(f"[01.7] diagnostics for '[unclosed': {diag!r}")
                if not isinstance(diag, list):
                    failures.append(
                        f"01.7: diagnostics should be a list; got {type(diag).__name__}: {diag!r}")
                elif len(diag) == 0:
                    failures.append(
                        "01.7: diagnostics list should contain at least one entry "
                        "for the malformed line '[unclosed'")
                else:
                    entry = diag[0]
                    ln  = _get(entry, "line_number", "lineNumber")
                    lt  = _get(entry, "line_text",   "lineText", "text")
                    rsn = _get(entry, "reason")
                    print(f"[01.7] diagnostics[0]: line_number={ln!r}, "
                          f"line_text={lt!r}, reason={rsn!r}")
                    if ln is None:
                        failures.append("01.7: diagnostics entry missing line_number/lineNumber")
                    if lt is None:
                        failures.append("01.7: diagnostics entry missing line_text/lineText")
                    if rsn is None:
                        failures.append("01.7: diagnostics entry missing reason")
            except Exception as e:
                failures.append(f"01.7: {e}")
                print(f"[01.7] exception: {e}")

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
