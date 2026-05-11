#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercises matcher construction requirements (01.1–01.7) via the MCP wrapper."""

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
        chunk = sock.recv(8192)
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


ONE_PATTERN_SET = [
    {"body": "*.log", "is_negation": False, "is_anchored": False, "is_dir_only": False}
]
EMPTY_PATTERN_SET = []


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []

            # Create the base empty matcher used by all assertions.
            r_em = _call(s, "empty-matcher")
            if r_em.get("error"):
                print(f"[setup] empty-matcher failed: {r_em['error']}")
                return 1
            em = _result(r_em)["matcher"]

            # --- 01.1: empty_matcher() layer_count is zero ---
            r_lc_em = _call(s, "layer-count", {"matcher": em})
            count_em = _result(r_lc_em).get("count", -1)
            print(f"[01.1] empty_matcher() layer_count = {count_em}")
            if count_em != 0:
                failures.append(f"01.1: expected layer_count 0, got {count_em}")

            # --- 01.2: push_scope returns layer_count(parent) + 1 ---
            r_p1 = _call(s, "push-scope", {
                "matcher": em,
                "scope_dir": "a",
                "pattern_set": ONE_PATTERN_SET,
            })
            m1 = None
            if r_p1.get("error"):
                print(f"[01.2] push-scope error: {r_p1['error']}")
                failures.append(f"01.2: push-scope failed: {r_p1['error']}")
            else:
                m1 = _result(r_p1)["matcher"]
                r_lc1 = _call(s, "layer-count", {"matcher": m1})
                count1 = _result(r_lc1).get("count", -1)
                print(f"[01.2] push_scope on empty_matcher() layer_count = {count1}")
                if count1 != 1:
                    failures.append(f"01.2: expected layer_count 1, got {count1}")

            # --- 01.3: push_scope does not mutate parent ---
            # Check empty parent (em) is unchanged after creating m1.
            r_lc_em2 = _call(s, "layer-count", {"matcher": em})
            count_em2 = _result(r_lc_em2).get("count", -1)
            print(f"[01.3] empty parent layer_count after push_scope = {count_em2}")
            if count_em2 != 0:
                failures.append(f"01.3: parent mutated — expected layer_count 0, got {count_em2}")

            # --- 01.4: newly pushed layer sits at index layer_count(parent) (shallowest-first) ---
            if m1 is not None:
                r_p2 = _call(s, "push-scope", {
                    "matcher": m1,
                    "scope_dir": "a/b",
                    "pattern_set": ONE_PATTERN_SET,
                })
                if r_p2.get("error"):
                    print(f"[01.4] second push-scope error: {r_p2['error']}")
                    failures.append(f"01.4: second push-scope failed: {r_p2['error']}")
                else:
                    m2 = _result(r_p2)["matcher"]
                    # layer_count(m1) was 1, so the new layer must be at index 1
                    r_la_new = _call(s, "layer-at", {"matcher": m2, "index": 1})
                    la_new = _result(r_la_new)
                    print(f"[01.4] newly pushed layer at index 1: scope_dir = {la_new.get('scope_dir')!r}")
                    if la_new.get("scope_dir") != "a/b":
                        failures.append(
                            f"01.4: expected scope_dir 'a/b' at index 1, got {la_new.get('scope_dir')!r}"
                        )

                    # --- 01.3 (non-empty parent): m1 layer_count and layer contents unchanged after push ---
                    r_lc_m1_post = _call(s, "layer-count", {"matcher": m1})
                    count_m1_post = _result(r_lc_m1_post).get("count", -1)
                    print(f"[01.3] non-empty parent layer_count after push_scope = {count_m1_post}")
                    if count_m1_post != 1:
                        failures.append(
                            f"01.3: non-empty parent mutated — expected layer_count 1, got {count_m1_post}"
                        )
                    r_la_m1_post = _call(s, "layer-at", {"matcher": m1, "index": 0})
                    la_m1_post = _result(r_la_m1_post)
                    if la_m1_post.get("scope_dir") != "a":
                        failures.append(
                            f"01.3: non-empty parent layer contents mutated — "
                            f"expected scope_dir 'a', got {la_m1_post.get('scope_dir')!r}"
                        )
            else:
                failures.append("01.4: skipped — prerequisite push-scope (01.2) failed")
                print("[01.4] skipped — prerequisite failed")

            # --- 01.5: layer_at returns the scope_dir and PatternSet recorded when pushed ---
            if m1 is not None:
                r_la0 = _call(s, "layer-at", {"matcher": m1, "index": 0})
                la0 = _result(r_la0)
                returned_ps = la0.get("pattern_set")
                print(
                    f"[01.5] layer_at(m1, 0): scope_dir = {la0.get('scope_dir')!r}, "
                    f"pattern_set = {returned_ps!r}"
                )
                if la0.get("scope_dir") != "a":
                    failures.append(f"01.5: expected scope_dir 'a' at index 0, got {la0.get('scope_dir')!r}")
                if returned_ps != ONE_PATTERN_SET:
                    failures.append(
                        f"01.5: pattern_set mismatch: expected {ONE_PATTERN_SET!r}, got {returned_ps!r}"
                    )
            else:
                failures.append("01.5: skipped — prerequisite push-scope (01.2) failed")
                print("[01.5] skipped — prerequisite failed")

            # --- 01.6: push_scope with empty PatternSet produces a visible layer ---
            r_p_empty = _call(s, "push-scope", {
                "matcher": em,
                "scope_dir": "dir6",
                "pattern_set": EMPTY_PATTERN_SET,
            })
            if r_p_empty.get("error"):
                print(f"[01.6] push-scope with empty PatternSet error: {r_p_empty['error']}")
                failures.append(f"01.6: push-scope with empty PatternSet rejected: {r_p_empty['error']}")
            else:
                m_ep = _result(r_p_empty)["matcher"]
                r_lc_ep = _call(s, "layer-count", {"matcher": m_ep})
                count_ep = _result(r_lc_ep).get("count", -1)
                r_la_ep = _call(s, "layer-at", {"matcher": m_ep, "index": 0})
                la_ep = _result(r_la_ep)
                returned_ps_ep = la_ep.get("pattern_set")
                print(
                    f"[01.6] empty PatternSet push: layer_count = {count_ep}, "
                    f"layer_at scope_dir = {la_ep.get('scope_dir')!r}, "
                    f"pattern_set = {returned_ps_ep!r}"
                )
                if count_ep != 1:
                    failures.append(
                        f"01.6: expected layer_count 1 after empty PatternSet push, got {count_ep}"
                    )
                if la_ep.get("scope_dir") != "dir6":
                    failures.append(
                        f"01.6: expected scope_dir 'dir6' at index 0, got {la_ep.get('scope_dir')!r}"
                    )
                if returned_ps_ep != EMPTY_PATTERN_SET:
                    failures.append(
                        f"01.6: expected empty pattern_set in layer_at, got {returned_ps_ep!r}"
                    )

            # --- 01.7: scope_dir of empty string is accepted (denotes sync root) ---
            r_p_root = _call(s, "push-scope", {
                "matcher": em,
                "scope_dir": "",
                "pattern_set": EMPTY_PATTERN_SET,
            })
            if r_p_root.get("error"):
                print(f"[01.7] push-scope with scope_dir='' error: {r_p_root['error']}")
                failures.append(f"01.7: push-scope with empty scope_dir rejected: {r_p_root['error']}")
            else:
                m_root = _result(r_p_root)["matcher"]
                r_la_root = _call(s, "layer-at", {"matcher": m_root, "index": 0})
                la_root = _result(r_la_root)
                print(f"[01.7] scope_dir='' accepted; layer_at scope_dir = {la_root.get('scope_dir')!r}")
                if la_root.get("scope_dir") != "":
                    failures.append(
                        f"01.7: expected scope_dir '' at index 0, got {la_root.get('scope_dir')!r}"
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
