#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Stack precedence: last matching pattern across all layers wins, with negation."""

from __future__ import annotations

import json, os, socket, subprocess, sys, threading, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

_rpc_id = [0]


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
    _rpc_id[0] += 1
    msg = {"jsonrpc": "2.0", "id": _rpc_id[0], "method": method}
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


def _call(sock, tool, arguments):
    return _rpc(sock, "tools/call", {"name": tool, "arguments": arguments})


def _empty_matcher(sock):
    r = _call(sock, "empty-matcher", {})
    if "error" in r:
        raise RuntimeError(f"empty-matcher error: {r['error']}")
    return (r.get("result") or {})["matcher"]


def _push_scope(sock, matcher, scope_dir, pattern_set):
    r = _call(sock, "push-scope", {
        "matcher": matcher,
        "scope_dir": scope_dir,
        "pattern_set": pattern_set,
    })
    if "error" in r:
        raise RuntimeError(f"push-scope error: {r['error']}")
    return (r.get("result") or {})["matcher"]


def _is_ignored(sock, matcher, path, is_dir):
    r = _call(sock, "is-ignored", {
        "matcher": matcher,
        "path": path,
        "is_dir": is_dir,
    })
    if "error" in r:
        raise RuntimeError(f"is-ignored error: {r['error']}")
    result = r.get("result") or {}
    if isinstance(result, bool):
        return result
    v = result.get("ignored") if "ignored" in result else result.get("is_ignored")
    return bool(v) if v is not None else None


def _pat(body, *, is_negation=False, is_anchored=False, is_dir_only=False):
    return {
        "body": body,
        "is_negation": is_negation,
        "is_anchored": is_anchored,
        "is_dir_only": is_dir_only,
    }


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []

            tl = _rpc(s, "tools/list")
            tools = (tl.get("result") or {}).get("tools", [])
            print(f"tools/list returned {len(tools)} tool(s): {[t['name'] for t in tools]}")

            # 03.1 — patterns from every layer are considered during evaluation
            try:
                m0 = _empty_matcher(s)
                # Layer 1 (shallow): has a matching pattern for "app.log"
                m1 = _push_scope(s, m0, "", [_pat("app.log")])
                # Layer 2 (deeper): empty — contributes no patterns
                m2 = _push_scope(s, m1, "sub", [])
                result = _is_ignored(s, m2, "app.log", False)
                ok = result is True
                print(f"[03.1] layer-1 pattern visible through empty layer-2: is_ignored={result} -> {'PASS' if ok else 'FAIL'}")
                if not ok:
                    failures.append("03.1: patterns from layer 1 not considered when layer 2 is empty")
            except Exception as e:
                failures.append(f"03.1: exception: {e}")
                print(f"[03.1] FAIL: {e}")

            # 03.2 — deepest layer that has an applying pattern is the deciding layer
            try:
                m0 = _empty_matcher(s)
                # Layer 1: negation (unignore "target") — shallow
                m1 = _push_scope(s, m0, "", [_pat("target", is_negation=True)])
                # Layer 2: non-negation (ignore "target") — deeper, should decide
                m2 = _push_scope(s, m1, "", [_pat("target")])
                result = _is_ignored(s, m2, "target", False)
                ok = result is True
                print(f"[03.2] deepest layer decides (layer-2 ignore beats layer-1 unignore): is_ignored={result} -> {'PASS' if ok else 'FAIL'}")
                if not ok:
                    failures.append("03.2: deepest applying layer did not decide")
            except Exception as e:
                failures.append(f"03.2: exception: {e}")
                print(f"[03.2] FAIL: {e}")

            # 03.3 — within the deciding layer, the last pattern in source order is the decision
            try:
                m0 = _empty_matcher(s)
                # Two patterns in one layer for the same path; last one (negation) must win
                m1 = _push_scope(s, m0, "", [
                    _pat("foo"),                      # pattern 1: ignore
                    _pat("foo", is_negation=True),    # pattern 2 (last): unignore
                ])
                result = _is_ignored(s, m1, "foo", False)
                ok = result is False
                print(f"[03.3] last pattern in source order wins (negation last): is_ignored={result} -> {'PASS' if ok else 'FAIL'}")
                if not ok:
                    failures.append("03.3: last pattern in source order did not win within a layer")
            except Exception as e:
                failures.append(f"03.3: exception: {e}")
                print(f"[03.3] FAIL: {e}")

            # 03.4 — last applying non-negation pattern → is_ignored returns true
            try:
                m0 = _empty_matcher(s)
                m1 = _push_scope(s, m0, "", [_pat("target")])
                result = _is_ignored(s, m1, "target", False)
                ok = result is True
                print(f"[03.4] non-negation pattern → is_ignored=true: is_ignored={result} -> {'PASS' if ok else 'FAIL'}")
                if not ok:
                    failures.append("03.4: is_ignored did not return true for non-negation last pattern")
            except Exception as e:
                failures.append(f"03.4: exception: {e}")
                print(f"[03.4] FAIL: {e}")

            # 03.5 — last applying negation pattern overrides earlier non-negation → is_ignored returns false
            try:
                m0 = _empty_matcher(s)
                # Layer 1: ignore "target"
                m1 = _push_scope(s, m0, "", [_pat("target")])
                # Layer 2 (deeper): unignore "target" — this negation must override
                m2 = _push_scope(s, m1, "", [_pat("target", is_negation=True)])
                result = _is_ignored(s, m2, "target", False)
                ok = result is False
                print(f"[03.5] negation overrides earlier non-negation: is_ignored={result} -> {'PASS' if ok else 'FAIL'}")
                if not ok:
                    failures.append("03.5: negation did not override earlier non-negation match")
            except Exception as e:
                failures.append(f"03.5: exception: {e}")
                print(f"[03.5] FAIL: {e}")

            # 03.6 — each layer's anchoring uses that layer's own scope_dir
            try:
                m0 = _empty_matcher(s)
                # Layer 1 at scope "a": anchored pattern "file.txt" → matches "a/file.txt"
                m1 = _push_scope(s, m0, "a", [_pat("file.txt", is_anchored=True)])
                # Layer 2 at scope "b": anchored pattern "other.txt" → matches "b/other.txt"
                m2 = _push_scope(s, m1, "b", [_pat("other.txt", is_anchored=True)])

                # "a/file.txt": layer-1 anchored at "a" → applies (suffix "file.txt" matches)
                r1 = _is_ignored(s, m2, "a/file.txt", False)
                # "b/other.txt": layer-2 anchored at "b" → applies (suffix "other.txt" matches)
                r2 = _is_ignored(s, m2, "b/other.txt", False)
                # "b/file.txt": layer-1 anchored at "a" → does NOT apply (not within "a");
                #              layer-2 anchored at "b" → does NOT apply ("file.txt" ≠ "other.txt")
                r3 = _is_ignored(s, m2, "b/file.txt", False)

                ok = (r1 is True) and (r2 is True) and (r3 is False)
                print(f"[03.6] scope-relative anchoring: a/file.txt={r1}, b/other.txt={r2}, b/file.txt={r3} -> {'PASS' if ok else 'FAIL'}")
                if not ok:
                    failures.append(
                        f"03.6: scope-relative anchoring wrong: a/file.txt={r1}, b/other.txt={r2}, b/file.txt={r3}"
                    )
            except Exception as e:
                failures.append(f"03.6: exception: {e}")
                print(f"[03.6] FAIL: {e}")

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
