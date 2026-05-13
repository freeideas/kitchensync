#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""record_decided writes pending-decision rows; confirm_present stamps last_seen on existing rows."""

from __future__ import annotations

import json, os, socket, subprocess, sys, threading, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

TMP = Path(PROJECT) / "tmp" / "testks" / "02-decide-and-confirm"

TS1 = "2024-03-01_10-00-00_000001Z"
TS2 = "2024-03-01_10-00-00_000002Z"
TS3 = "2024-03-01_10-00-00_000003Z"
TS4 = "2024-03-01_10-00-00_000004Z"


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


def _call(sock, name, arguments, rpc_id):
    return _rpc(sock, "tools/call", {"name": name, "arguments": arguments}, rpc_id=rpc_id)


def main() -> int:
    TMP.mkdir(parents=True, exist_ok=True)
    db = TMP / "test.db"
    if db.exists():
        db.unlink()

    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []
            rpc_id = iter(range(1, 1000))

            r = _call(s, "open", {"file": str(db)}, next(rpc_id))
            handle = (r.get("result") or {}).get("handle")

            # --- 02.1: record_decided inserts a new row with last_seen null ---
            _call(s, "record-decided", {
                "handle": handle, "path": "docs/readme.txt",
                "mod_time": TS1, "byte_size": 512, "is_dir": False,
            }, next(rpc_id))
            r = _call(s, "lookup", {"handle": handle, "path": "docs/readme.txt"}, next(rpc_id))
            rec = (r.get("result") or {}).get("record")
            print(f"[02.1] record_decided inserts new row with last_seen null: row={rec is not None}, last_seen={rec.get('last_seen') if rec else 'N/A'}")
            if rec is None:
                failures.append("02.1: no row found after record_decided on new path")
            elif rec.get("last_seen") is not None:
                failures.append(f"02.1: expected last_seen=null, got {rec.get('last_seen')!r}")

            # --- 02.2: row has supplied mod_time, byte_size, and deleted_time=null ---
            print(f"[02.2] record_decided row fields: mod_time={rec.get('mod_time') if rec else 'N/A'}, byte_size={rec.get('byte_size') if rec else 'N/A'}, deleted_time={rec.get('deleted_time') if rec else 'N/A'}")
            if rec is None:
                failures.append("02.2: no row found (blocked by 02.1 failure)")
            else:
                if rec.get("mod_time") != TS1:
                    failures.append(f"02.2: mod_time expected {TS1!r}, got {rec.get('mod_time')!r}")
                if rec.get("byte_size") != 512:
                    failures.append(f"02.2: byte_size expected 512, got {rec.get('byte_size')!r}")
                if rec.get("deleted_time") is not None:
                    failures.append(f"02.2: deleted_time expected null, got {rec.get('deleted_time')!r}")

            # 02.2 (is_dir branch): byte_size overridden to -1 for directories
            _call(s, "record-decided", {
                "handle": handle, "path": "media/photos",
                "mod_time": TS1, "byte_size": 9999, "is_dir": True,
            }, next(rpc_id))
            r = _call(s, "lookup", {"handle": handle, "path": "media/photos"}, next(rpc_id))
            dir_rec = (r.get("result") or {}).get("record")
            print(f"[02.2] is_dir=True substitutes byte_size=-1: mod_time={dir_rec.get('mod_time') if dir_rec else 'N/A'}, byte_size={dir_rec.get('byte_size') if dir_rec else 'N/A'}, deleted_time={dir_rec.get('deleted_time') if dir_rec else 'N/A'}")
            if dir_rec is None:
                failures.append("02.2: no row found for directory path")
            else:
                if dir_rec.get("mod_time") != TS1:
                    failures.append(f"02.2: directory mod_time expected {TS1!r}, got {dir_rec.get('mod_time')!r}")
                if dir_rec.get("byte_size") != -1:
                    failures.append(f"02.2: is_dir=True should store byte_size=-1, got {dir_rec.get('byte_size')!r}")
                if dir_rec.get("deleted_time") is not None:
                    failures.append(f"02.2: directory deleted_time expected null, got {dir_rec.get('deleted_time')!r}")

            # --- 02.3: record_decided on existing row updates mod_time/byte_size, leaves last_seen unchanged ---
            # Use upsert-observed to create a row that has a non-null last_seen
            _call(s, "upsert-observed", {
                "handle": handle, "path": "data/archive.zip",
                "mod_time": TS1, "byte_size": 1024, "is_dir": False, "now": TS1,
            }, next(rpc_id))
            r = _call(s, "lookup", {"handle": handle, "path": "data/archive.zip"}, next(rpc_id))
            before = (r.get("result") or {}).get("record")
            last_seen_before = before.get("last_seen") if before else None
            # Now update the same path via record_decided
            _call(s, "record-decided", {
                "handle": handle, "path": "data/archive.zip",
                "mod_time": TS2, "byte_size": 2048, "is_dir": False,
            }, next(rpc_id))
            r = _call(s, "lookup", {"handle": handle, "path": "data/archive.zip"}, next(rpc_id))
            after = (r.get("result") or {}).get("record")
            print(f"[02.3] record_decided updates mod_time/byte_size, preserves last_seen: mod_time={after.get('mod_time') if after else 'N/A'}, byte_size={after.get('byte_size') if after else 'N/A'}, last_seen={after.get('last_seen') if after else 'N/A'}")
            if after is None:
                failures.append("02.3: no row found after record_decided update")
            else:
                if after.get("mod_time") != TS2:
                    failures.append(f"02.3: mod_time not updated: expected {TS2!r}, got {after.get('mod_time')!r}")
                if after.get("byte_size") != 2048:
                    failures.append(f"02.3: byte_size not updated: expected 2048, got {after.get('byte_size')!r}")
                if after.get("last_seen") != last_seen_before:
                    failures.append(f"02.3: last_seen changed: {last_seen_before!r} -> {after.get('last_seen')!r}")

            # --- 02.4: confirm_present sets last_seen to now, leaving other fields unchanged ---
            before_confirm = after
            _call(s, "confirm-present", {
                "handle": handle, "path": "data/archive.zip", "now": TS3,
            }, next(rpc_id))
            r = _call(s, "lookup", {"handle": handle, "path": "data/archive.zip"}, next(rpc_id))
            confirmed = (r.get("result") or {}).get("record")
            print(f"[02.4] confirm_present sets last_seen only: last_seen={confirmed.get('last_seen') if confirmed else 'N/A'}")
            if confirmed is None:
                failures.append("02.4: no row found after confirm_present")
            elif before_confirm is None:
                failures.append("02.4: no pre-confirm row available for unchanged-field comparison")
            else:
                if confirmed.get("last_seen") != TS3:
                    failures.append(f"02.4: last_seen not updated: expected {TS3!r}, got {confirmed.get('last_seen')!r}")
                for field in ("id", "parent_id", "basename", "mod_time", "byte_size", "deleted_time"):
                    if confirmed.get(field) != before_confirm.get(field):
                        failures.append(
                            f"02.4: {field} changed unexpectedly: "
                            f"{before_confirm.get(field)!r} -> {confirmed.get(field)!r}"
                        )

            # --- 02.5: confirm_present is a no-op when no row exists at path ---
            _call(s, "upsert-observed", {
                "handle": handle, "path": "guard/stable.txt",
                "mod_time": TS1, "byte_size": 77, "is_dir": False, "now": TS1,
            }, next(rpc_id))
            r = _call(s, "lookup", {"handle": handle, "path": "guard/stable.txt"}, next(rpc_id))
            row_before_missing_confirm = (r.get("result") or {}).get("record")
            _call(s, "confirm-present", {
                "handle": handle, "path": "ghost/nonexistent.txt", "now": TS3,
            }, next(rpc_id))
            r = _call(s, "lookup", {"handle": handle, "path": "ghost/nonexistent.txt"}, next(rpc_id))
            ghost = (r.get("result") or {}).get("record")
            r = _call(s, "lookup", {"handle": handle, "path": "guard/stable.txt"}, next(rpc_id))
            row_after_missing_confirm = (r.get("result") or {}).get("record")
            print(f"[02.5] confirm_present on missing path is a no-op: row_inserted={ghost is not None}, other_row_changed={row_after_missing_confirm != row_before_missing_confirm}")
            if ghost is not None:
                failures.append(f"02.5: confirm_present inserted a row that should not exist: {ghost!r}")
            if row_before_missing_confirm is None:
                failures.append("02.5: no guard row available for no-op comparison")
            if row_after_missing_confirm != row_before_missing_confirm:
                failures.append(
                    "02.5: confirm_present on missing path modified another row: "
                    f"{row_before_missing_confirm!r} -> {row_after_missing_confirm!r}"
                )

            # --- 02.6: record_decided on a tombstoned row clears deleted_time ---
            _call(s, "record-decided", {
                "handle": handle, "path": "logs/old.log",
                "mod_time": TS1, "byte_size": 100, "is_dir": False,
            }, next(rpc_id))
            _call(s, "mark-subtree-deleted", {
                "handle": handle, "path": "logs/old.log", "deleted_time": TS2,
            }, next(rpc_id))
            r = _call(s, "lookup", {"handle": handle, "path": "logs/old.log"}, next(rpc_id))
            tombstoned = (r.get("result") or {}).get("record")
            tombstone_set = tombstoned is not None and tombstoned.get("deleted_time") is not None
            _call(s, "record-decided", {
                "handle": handle, "path": "logs/old.log",
                "mod_time": TS4, "byte_size": 200, "is_dir": False,
            }, next(rpc_id))
            r = _call(s, "lookup", {"handle": handle, "path": "logs/old.log"}, next(rpc_id))
            revived = (r.get("result") or {}).get("record")
            print(f"[02.6] record_decided clears deleted_time on tombstoned row: was_tombstoned={tombstone_set}, deleted_time_after={revived.get('deleted_time') if revived else 'N/A'}")
            if not tombstone_set:
                failures.append("02.6: precondition failed — row was not tombstoned before record_decided")
            if revived is None:
                failures.append("02.6: no row found after record_decided on tombstoned row")
            elif revived.get("deleted_time") is not None:
                failures.append(f"02.6: deleted_time not cleared: {revived.get('deleted_time')!r}")

            _call(s, "close", {"handle": handle}, next(rpc_id))

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
