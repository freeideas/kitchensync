#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercises matcher-basics requirements (02.1–02.6) via the MCP wrapper."""

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
    return _rpc(sock, "tools/call", {"name": tool, "arguments": arguments or {}})


def _result(resp):
    return resp.get("result") or {}


def _pat(body, is_negation=False, is_anchored=False, is_dir_only=False):
    return {
        "body": body,
        "is_negation": is_negation,
        "is_anchored": is_anchored,
        "is_dir_only": is_dir_only,
    }


def _empty_matcher(s):
    r = _call(s, "empty-matcher")
    if r.get("error"):
        raise RuntimeError(f"empty-matcher failed: {r['error']}")
    return _result(r)["matcher"]


def _push(s, m, scope_dir, patterns):
    r = _call(s, "push-scope", {"matcher": m, "scope_dir": scope_dir, "pattern_set": patterns})
    if r.get("error"):
        raise RuntimeError(f"push-scope failed: {r['error']}")
    return _result(r)["matcher"]


def _is_ignored(s, m, path, is_dir):
    r = _call(s, "is-ignored", {"matcher": m, "path": path, "is_dir": is_dir})
    if r.get("error"):
        raise RuntimeError(f"is-ignored failed: {r['error']}")
    res = _result(r)
    for key in ("ignored", "is_ignored"):
        if key in res:
            return res[key]
    raise RuntimeError(f"is-ignored result missing expected key: {res}")


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []

            # --- 02.1: empty_matcher is_ignored returns false for arbitrary non-built-in path ---
            try:
                m_empty = _empty_matcher(s)
                r = _is_ignored(s, m_empty, "some/arbitrary/path.txt", False)
                print(f"[02.1] empty matcher is_ignored('some/arbitrary/path.txt', false) = {r!r}")
                if r is not False:
                    failures.append(f"02.1: expected false, got {r!r}")
            except Exception as e:
                failures.append(f"02.1: {e}")
                print(f"[02.1] exception: {e}")

            # --- 02.2: push_scope returns new Matcher including set's patterns ---
            m_base = None
            try:
                m_base = _empty_matcher(s)
                m_pushed = _push(s, m_base, "", [_pat("special.txt")])
                r = _is_ignored(s, m_pushed, "special.txt", False)
                print(f"[02.2] push_scope with 'special.txt', is_ignored('special.txt') = {r!r}")
                if r is not True:
                    failures.append(f"02.2: expected true after pushing 'special.txt' pattern, got {r!r}")
            except Exception as e:
                failures.append(f"02.2: {e}")
                print(f"[02.2] exception: {e}")

            # --- 02.3: push_scope does not mutate the parent Matcher ---
            try:
                if m_base is None:
                    raise RuntimeError("prerequisite (02.2) failed")
                r = _is_ignored(s, m_base, "special.txt", False)
                print(f"[02.3] parent after push_scope: is_ignored('special.txt') = {r!r}")
                if r is not False:
                    failures.append(f"02.3: parent mutated — expected false, got {r!r}")
            except Exception as e:
                failures.append(f"02.3: {e}")
                print(f"[02.3] exception: {e}")

            # --- 02.4: is_ignored returns true for a literal (non-wildcard, non-negated) pattern ---
            try:
                m_lit = _push(s, _empty_matcher(s), "", [_pat("readme.md")])
                r = _is_ignored(s, m_lit, "readme.md", False)
                print(f"[02.4] literal pattern 'readme.md': is_ignored = {r!r}")
                if r is not True:
                    failures.append(f"02.4: expected true for literal pattern match, got {r!r}")
            except Exception as e:
                failures.append(f"02.4: {e}")
                print(f"[02.4] exception: {e}")

            # --- 02.5: negation pattern re-includes a previously excluded path ---
            try:
                # *.log excludes all .log; !build.log re-includes build.log
                m_neg = _push(s, _empty_matcher(s), "", [
                    _pat("*.log"),
                    _pat("build.log", is_negation=True),
                ])
                r_reincluded = _is_ignored(s, m_neg, "build.log", False)
                r_still_excluded = _is_ignored(s, m_neg, "other.log", False)
                print(
                    f"[02.5] negation: is_ignored('build.log')={r_reincluded!r}, "
                    f"is_ignored('other.log')={r_still_excluded!r}"
                )
                if r_reincluded is not False:
                    failures.append(
                        f"02.5: expected false (negation re-includes build.log), got {r_reincluded!r}"
                    )
                if r_still_excluded is not True:
                    failures.append(
                        f"02.5: expected true (other.log still excluded), got {r_still_excluded!r}"
                    )
            except Exception as e:
                failures.append(f"02.5: {e}")
                print(f"[02.5] exception: {e}")

            # --- 02.6: last matching pattern across the entire scope stack wins ---
            try:
                # Shallow scope excludes *.txt; deeper scope negates keep.txt
                m_shallow = _push(s, _empty_matcher(s), "", [_pat("*.txt")])
                m_deep = _push(s, m_shallow, "", [_pat("keep.txt", is_negation=True)])
                r_negated = _is_ignored(s, m_deep, "keep.txt", False)
                r_excluded = _is_ignored(s, m_deep, "other.txt", False)
                print(
                    f"[02.6] cross-scope last-match: is_ignored('keep.txt')={r_negated!r}, "
                    f"is_ignored('other.txt')={r_excluded!r}"
                )
                if r_negated is not False:
                    failures.append(
                        f"02.6: expected false (deeper negation overrides shallower exclude), got {r_negated!r}"
                    )
                if r_excluded is not True:
                    failures.append(
                        f"02.6: expected true (shallower exclude, no deeper override), got {r_excluded!r}"
                    )
            except Exception as e:
                failures.append(f"02.6: {e}")
                print(f"[02.6] exception: {e}")

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
