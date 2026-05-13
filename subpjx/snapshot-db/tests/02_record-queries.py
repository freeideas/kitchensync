#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercises lookup and list_children record-query operations."""

from __future__ import annotations

import json, os, socket, subprocess, sys, threading, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY",
                              "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

DB_PATH = "/tmp/snapshot-db-test-02-record-queries.db"
TS = "2026-05-12_10-00-00_000000Z"


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


def _call(sock, tool, args, rid):
    return _rpc(sock, "tools/call", {"name": tool, "arguments": args}, rpc_id=rid)


def _unwrap(resp):
    result = resp.get("result", {})
    content = result.get("content", [])
    if content and content[0].get("type") == "text":
        text = content[0]["text"]
        try:
            return json.loads(text)
        except (json.JSONDecodeError, ValueError):
            return text
    return result


def main() -> int:
    # Idempotency: remove any leftover db from a prior run.
    Path(DB_PATH).unlink(missing_ok=True)

    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []
            rid = 1

            r = _call(s, "open", {"file": DB_PATH}, rid); rid += 1
            handle = _unwrap(r).get("handle")
            if handle is None:
                print("FATAL: open failed, cannot continue")
                return 1

            # ── 02.1: lookup returns the previously written row with correct fields ──
            _call(s, "upsert-observed",
                  {"handle": handle, "path": "docs/readme.txt",
                   "mod_time": TS, "byte_size": 42, "is_dir": False, "now": TS},
                  rid); rid += 1
            r = _call(s, "lookup", {"handle": handle, "path": "docs/readme.txt"}, rid); rid += 1
            rec = _unwrap(r).get("record")
            print(f"[02.1] lookup after write: record={rec}")
            if rec is None:
                failures.append("02.1: lookup returned no record for a written path")
            else:
                bad = []
                if rec.get("mod_time") != TS:
                    bad.append(f"mod_time={rec.get('mod_time')!r}")
                if rec.get("byte_size") != 42:
                    bad.append(f"byte_size={rec.get('byte_size')!r}")
                if rec.get("last_seen") != TS:
                    bad.append(f"last_seen={rec.get('last_seen')!r}")
                if rec.get("deleted_time") is not None:
                    bad.append(f"deleted_time={rec.get('deleted_time')!r}")
                if rec.get("basename") != "readme.txt":
                    bad.append(f"basename={rec.get('basename')!r}")
                if bad:
                    failures.append(f"02.1: lookup record has wrong fields: {', '.join(bad)}")

            # ── 02.2: lookup returns no record for a path never written ──────────────
            r = _call(s, "lookup", {"handle": handle, "path": "never/written.txt"}, rid); rid += 1
            rec2 = _unwrap(r).get("record")
            print(f"[02.2] lookup nonexistent path: record={rec2}")
            if rec2 is not None:
                failures.append(f"02.2: lookup should return no record for unwritten path, got: {rec2}")

            # ── 02.3: list_children returns every row matching identify(parent_path) ──
            _call(s, "upsert-observed",
                  {"handle": handle, "path": "projects/alpha.txt",
                   "mod_time": TS, "byte_size": 10, "is_dir": False, "now": TS},
                  rid); rid += 1
            _call(s, "upsert-observed",
                  {"handle": handle, "path": "projects/beta.txt",
                   "mod_time": TS, "byte_size": 20, "is_dir": False, "now": TS},
                  rid); rid += 1
            r = _call(s, "list-children", {"handle": handle, "parent_path": "projects"}, rid); rid += 1
            children = _unwrap(r).get("records", [])
            basenames = {c.get("basename") for c in children}
            print(f"[02.3] list_children('projects'): basenames={basenames}")
            if "alpha.txt" not in basenames or "beta.txt" not in basenames:
                failures.append(f"02.3: list_children missing expected children; got: {basenames}")

            # ── 02.4: list_children("/") returns root's immediate children only ──────
            _call(s, "upsert-observed",
                  {"handle": handle, "path": "root-a.txt",
                   "mod_time": TS, "byte_size": 5, "is_dir": False, "now": TS},
                  rid); rid += 1
            _call(s, "upsert-observed",
                  {"handle": handle, "path": "root-b.txt",
                   "mod_time": TS, "byte_size": 7, "is_dir": False, "now": TS},
                  rid); rid += 1
            _call(s, "upsert-observed",
                  {"handle": handle, "path": "subdir/nested.txt",
                   "mod_time": TS, "byte_size": 3, "is_dir": False, "now": TS},
                  rid); rid += 1
            r = _call(s, "list-children", {"handle": handle, "parent_path": "/"}, rid); rid += 1
            root_children = _unwrap(r).get("records", [])
            root_basenames = {c.get("basename") for c in root_children}
            print(f"[02.4] list_children('/'): basenames={root_basenames}")
            if "root-a.txt" not in root_basenames or "root-b.txt" not in root_basenames:
                failures.append(f"02.4: list_children('/') missing root entries; got: {root_basenames}")
            if "nested.txt" in root_basenames:
                failures.append(f"02.4: list_children('/') must not include deep entries; got: {root_basenames}")

            # ── 02.5: list_children("") returns the same set as list_children("/") ───
            r = _call(s, "list-children", {"handle": handle, "parent_path": ""}, rid); rid += 1
            empty_children = _unwrap(r).get("records", [])
            empty_basenames = {c.get("basename") for c in empty_children}
            print(f"[02.5] list_children(''): basenames={empty_basenames}")
            if empty_basenames != root_basenames:
                failures.append(
                    f"02.5: list_children('') != list_children('/'): "
                    f"'' returned {empty_basenames}, '/' returned {root_basenames}"
                )

            _call(s, "close", {"handle": handle}, rid); rid += 1

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
