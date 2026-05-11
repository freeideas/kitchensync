#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercises 02.1–02.7 and 02.33: classify_file returns the correct classification for every input combination."""

from __future__ import annotations

import json, os, socket, subprocess, sys, threading, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

NOW = 1_700_000_000
TOLS = 5

VALID_CLASSIFICATIONS = {
    "unchanged", "modified", "resurrection", "new",
    "deleted", "absent_unconfirmed", "no_opinion",
}

_next_id = 0


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
    global _next_id
    _next_id += 1
    msg = {"jsonrpc": "2.0", "id": _next_id, "method": method}
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


def _classify(sock, listing_state, snapshot_row):
    return _rpc(sock, "tools/call", {
        "name": "classify-file",
        "arguments": {
            "listing_state": listing_state,
            "snapshot_row": snapshot_row,
            "timestamp_tolerance_seconds": TOLS,
            "now": NOW,
        },
    })


def _live_file(mod_time=NOW, byte_size=1024):
    return {"kind": "live_file", "mod_time": mod_time, "byte_size": byte_size}


def _absent():
    return {"kind": "absent"}


def _row(mod_time=NOW, byte_size=1024, last_seen=None, deleted_time=None):
    return {
        "mod_time": mod_time,
        "byte_size": byte_size,
        "last_seen": last_seen if last_seen is not None else mod_time,
        "deleted_time": deleted_time,
    }


def _classification(resp):
    return (resp.get("result") or {}).get("classification")


def main() -> int:
    proc, port = _launch()
    failures = []
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:

            # 02.1 — returned classification is one of the seven defined values
            r = _classify(s, _live_file(), _row())
            c = _classification(r)
            print(f"[02.1] classification={c!r} in valid set: {c in VALID_CLASSIFICATIONS}")
            if c not in VALID_CLASSIFICATIONS:
                failures.append(f"02.1: got {c!r}, want one of {sorted(VALID_CLASSIFICATIONS)}")

            # 02.2 — live_file, mod_time within tolerance, deleted_time null → unchanged
            r = _classify(s, _live_file(mod_time=1000), _row(mod_time=1002, deleted_time=None))
            c = _classification(r)
            print(f"[02.2] live_file mod_time within tolerance → {c!r}")
            if c != "unchanged":
                failures.append(f"02.2: expected unchanged, got {c!r}; response={r}")

            # 02.3 — live_file, mod_time beyond tolerance, deleted_time null → modified
            r = _classify(s, _live_file(mod_time=1100), _row(mod_time=1000, deleted_time=None))
            c = _classification(r)
            print(f"[02.3] live_file mod_time beyond tolerance → {c!r}")
            if c != "modified":
                failures.append(f"02.3: expected modified, got {c!r}; response={r}")

            # 02.4 — live_file, snapshot row has non-null deleted_time → resurrection
            r = _classify(s, _live_file(mod_time=1000), _row(mod_time=900, deleted_time=950))
            c = _classification(r)
            print(f"[02.4] live_file + tombstoned snapshot row → {c!r}")
            if c != "resurrection":
                failures.append(f"02.4: expected resurrection, got {c!r}; response={r}")

            # 02.5 — live_file, no snapshot row → new
            r = _classify(s, _live_file(mod_time=1000), None)
            c = _classification(r)
            print(f"[02.5] live_file + no snapshot row → {c!r}")
            if c != "new":
                failures.append(f"02.5: expected new, got {c!r}; response={r}")

            # 02.6 — absent, snapshot row has non-null deleted_time → deleted; estimate = deleted_time
            DELETED_TIME = 950
            r = _classify(s, _absent(), _row(mod_time=900, deleted_time=DELETED_TIME))
            result = r.get("result") or {}
            c = result.get("classification")
            estimate = result.get("estimate")
            print(f"[02.6] absent + tombstoned snapshot row → {c!r} estimate={estimate!r}")
            if c != "deleted":
                failures.append(f"02.6: expected deleted, got {c!r}; response={r}")
            elif estimate != DELETED_TIME:
                failures.append(f"02.6: estimate {estimate!r} != deleted_time {DELETED_TIME}; response={r}")

            # 02.7 — absent, snapshot row deleted_time null → absent_unconfirmed
            r = _classify(s, _absent(), _row(mod_time=1000, deleted_time=None))
            c = _classification(r)
            print(f"[02.7] absent + live snapshot row → {c!r}")
            if c != "absent_unconfirmed":
                failures.append(f"02.7: expected absent_unconfirmed, got {c!r}; response={r}")

            # 02.33 — absent, no snapshot row → no_opinion
            r = _classify(s, _absent(), None)
            c = _classification(r)
            print(f"[02.33] absent + no snapshot row → {c!r}")
            if c != "no_opinion":
                failures.append(f"02.33: expected no_opinion, got {c!r}; response={r}")

    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()

    if failures:
        print("\nFAILURES:")
        for f in failures:
            print(f"  - {f}")
        return 1
    print("\nAll assertions passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
