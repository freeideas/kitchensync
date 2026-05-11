#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Row upsert operations: confirmed-present, decided-but-unconfirmed, and mark-copy-completed."""

from __future__ import annotations

import json, os, socket, subprocess, sys, tempfile, threading, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

MOD_TIME_1 = "2024-01-15_10-30-00_000000Z"
MOD_TIME_2 = "2024-01-15_11-00-00_000000Z"
MOD_TIME_3 = "2024-01-15_12-00-00_000000Z"
CONF_TS_1  = "2024-01-15_10-30-00_000001Z"
CONF_TS_2  = "2024-01-15_11-00-00_000001Z"
CONF_TS_3  = "2024-01-15_12-00-00_000001Z"

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


def call(sock, tool, args=None):
    resp = _rpc(sock, "tools/call", {"name": tool, "arguments": args or {}})
    if "error" in resp:
        raise RuntimeError(f"{tool} failed: {resp['error'].get('message', resp['error'])}")
    return resp.get("result", {})


def _pick(tool_names, *candidates):
    for c in candidates:
        if c in tool_names:
            return c
    return None


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []

            tl = _rpc(s, "tools/list")
            tools = (tl.get("result") or {}).get("tools", [])
            tool_names = {t["name"] for t in tools}
            print(f"[info] tools: {sorted(tool_names)}")

            t_open  = _pick(tool_names, "open", "open-snapshot", "db-open")
            t_close = _pick(tool_names, "close", "close-snapshot", "db-close", "close-handle")
            t_hash  = _pick(tool_names, "hash-path", "hash")
            t_lookup = _pick(tool_names, "lookup-row", "lookup-row-by-id", "get-row")
            t_absent = _pick(tool_names, "mark-absent", "mark-deleted", "tombstone")
            # confirmed-present upsert: sets last_seen to the supplied ts
            t_confirmed = _pick(tool_names,
                                "upsert-confirmed", "upsert-confirmed-row",
                                "upsert-present", "upsert-confirmed-present")
            # decided-but-unconfirmed upsert: does NOT touch last_seen
            t_unconfirmed = _pick(tool_names,
                                  "upsert-unconfirmed", "upsert-unconfirmed-row",
                                  "upsert-planned", "upsert-decided")
            # mark-copy-completed: stamps last_seen on existing row
            t_completed = _pick(tool_names,
                                "mark-copy-completed", "copy-completed",
                                "mark-completed", "confirm-copy")

            if not t_open:
                print("[FATAL] no open-snapshot tool found; cannot proceed")
                failures.append("setup: no open-snapshot tool found")
                print("\nFAILURES:")
                for f in failures:
                    print(f"  - {f}")
                return 1

            with tempfile.TemporaryDirectory() as tmpdir:
                db_path = str(Path(tmpdir) / "row_upserts_test.db")

                open_resp = call(s, t_open, {"path": db_path})
                handle = open_resp.get("handle")
                if not handle:
                    print(f"[FATAL] open returned no handle: {open_resp!r}")
                    failures.append("setup: open returned no handle")
                    print("\nFAILURES:")
                    for f in failures:
                        print(f"  - {f}")
                    return 1

                def hash_path(path):
                    return call(s, t_hash, {"path": path}).get("id")

                def lookup(row_id):
                    return call(s, t_lookup, {"handle": handle, "id": row_id}).get("row")

                def upsert_confirmed(path, basename, mod_time, byte_size, ts):
                    args = {"handle": handle, "path": path, "basename": basename,
                            "mod_time": mod_time, "byte_size": byte_size}
                    # upsert-present uses 'last_seen'; other names use 'ts'
                    if t_confirmed in ("upsert-present", "upsert-confirmed-present"):
                        args["last_seen"] = ts
                    else:
                        args["ts"] = ts
                    call(s, t_confirmed, args)

                def upsert_unconfirmed(path, basename, mod_time, byte_size):
                    call(s, t_unconfirmed, {
                        "handle": handle, "path": path, "basename": basename,
                        "mod_time": mod_time, "byte_size": byte_size,
                    })

                def mark_copy_completed(path, ts):
                    call(s, t_completed, {"handle": handle, "path": path, "ts": ts})

                def mark_absent(path):
                    call(s, t_absent, {"handle": handle, "path": path})

                # --- 03.1: confirmed-present upsert produces row lookable by hash(path) ---
                img_row = None
                if not t_confirmed:
                    print("[03.1] SKIP: no confirmed-upsert tool")
                    failures.append("03.1: no confirmed-upsert tool found")
                else:
                    try:
                        upsert_confirmed("photos/img.jpg", "img.jpg", MOD_TIME_1, 12345, CONF_TS_1)
                        img_id = hash_path("photos/img.jpg")
                        img_row = lookup(img_id) if img_id else None
                        print("[03.1] confirmed-present upsert row lookable by hash(path) with correct fields")
                        if img_id is None:
                            failures.append("03.1: hash-path returned None for photos/img.jpg")
                        elif img_row is None:
                            failures.append("03.1: lookup returned None after confirmed upsert")
                        elif (img_row.get("basename") != "img.jpg"
                              or img_row.get("mod_time") != MOD_TIME_1
                              or img_row.get("byte_size") != 12345):
                            failures.append(f"03.1: row fields incorrect: {img_row!r}")
                    except RuntimeError as e:
                        failures.append(f"03.1: {e}")
                        print(f"[03.1] error: {e}")
                        img_row = None

                # --- 03.2: confirmed-present upsert sets last_seen to supplied ts ---
                print("[03.2] confirmed-present upsert sets last_seen to the supplied confirmation timestamp")
                if not t_confirmed:
                    failures.append("03.2: no confirmed-upsert tool found")
                elif img_row is None:
                    failures.append("03.2: row unavailable (03.1 failed)")
                elif img_row.get("last_seen") != CONF_TS_1:
                    failures.append(f"03.2: last_seen={img_row.get('last_seen')!r}, want {CONF_TS_1!r}")

                # --- 03.3: confirmed-present upsert clears deleted_time even on a tombstone ---
                if not t_confirmed or not t_absent:
                    print("[03.3] SKIP: missing required tool")
                    failures.append("03.3: missing required tool (confirmed-upsert or mark-absent)")
                else:
                    try:
                        upsert_confirmed("docs/tbs.txt", "tbs.txt", MOD_TIME_1, 100, CONF_TS_1)
                        mark_absent("docs/tbs.txt")
                        tbs_id = hash_path("docs/tbs.txt")
                        pre_tbs = lookup(tbs_id) if tbs_id else None
                        upsert_confirmed("docs/tbs.txt", "tbs.txt", MOD_TIME_2, 200, CONF_TS_2)
                        post_tbs = lookup(tbs_id) if tbs_id else None
                        print("[03.3] confirmed-present upsert clears deleted_time even when prior row was a tombstone")
                        if pre_tbs is None or pre_tbs.get("deleted_time") is None:
                            failures.append(f"03.3: setup failed — row not tombstoned before upsert: {pre_tbs!r}")
                        elif post_tbs is None or post_tbs.get("deleted_time") is not None:
                            failures.append(f"03.3: deleted_time not cleared after confirmed upsert: {post_tbs!r}")
                    except RuntimeError as e:
                        failures.append(f"03.3: {e}")
                        print(f"[03.3] error: {e}")

                # --- 03.4: confirmed-present upsert replaces mod_time and byte_size on existing row ---
                if not t_confirmed:
                    print("[03.4] SKIP: no confirmed-upsert tool")
                    failures.append("03.4: no confirmed-upsert tool found")
                else:
                    try:
                        upsert_confirmed("data/file.bin", "file.bin", MOD_TIME_1, 111, CONF_TS_1)
                        upsert_confirmed("data/file.bin", "file.bin", MOD_TIME_3, 999, CONF_TS_3)
                        file_id = hash_path("data/file.bin")
                        file_row = lookup(file_id) if file_id else None
                        print("[03.4] confirmed-present upsert on existing row replaces mod_time and byte_size")
                        if file_row is None:
                            failures.append("03.4: row not found after second confirmed upsert")
                        elif file_row.get("mod_time") != MOD_TIME_3 or file_row.get("byte_size") != 999:
                            failures.append(
                                f"03.4: want mod_time={MOD_TIME_3!r} byte_size=999, "
                                f"got mod_time={file_row.get('mod_time')!r} byte_size={file_row.get('byte_size')!r}"
                            )
                    except RuntimeError as e:
                        failures.append(f"03.4: {e}")
                        print(f"[03.4] error: {e}")

                # --- 03.5: decided-but-unconfirmed upsert produces row lookable by hash(path) ---
                song_row = None
                if not t_unconfirmed:
                    print("[03.5] SKIP: no unconfirmed-upsert tool")
                    failures.append("03.5: no unconfirmed-upsert tool found")
                else:
                    try:
                        upsert_unconfirmed("music/song.mp3", "song.mp3", MOD_TIME_1, 5000)
                        song_id = hash_path("music/song.mp3")
                        song_row = lookup(song_id) if song_id else None
                        print("[03.5] decided-but-unconfirmed upsert row lookable by hash(path) with correct fields")
                        if song_id is None:
                            failures.append("03.5: hash-path returned None for music/song.mp3")
                        elif song_row is None:
                            failures.append("03.5: lookup returned None after unconfirmed upsert")
                        elif (song_row.get("basename") != "song.mp3"
                              or song_row.get("mod_time") != MOD_TIME_1
                              or song_row.get("byte_size") != 5000):
                            failures.append(f"03.5: row fields incorrect: {song_row!r}")
                    except RuntimeError as e:
                        failures.append(f"03.5: {e}")
                        print(f"[03.5] error: {e}")
                        song_row = None

                # --- 03.6: decided-but-unconfirmed upsert clears any prior deleted_time ---
                if not t_unconfirmed or not t_confirmed or not t_absent:
                    print("[03.6] SKIP: missing required tool")
                    failures.append("03.6: missing required tool")
                else:
                    try:
                        upsert_confirmed("docs/del.txt", "del.txt", MOD_TIME_1, 50, CONF_TS_1)
                        mark_absent("docs/del.txt")
                        del_id = hash_path("docs/del.txt")
                        pre_del = lookup(del_id) if del_id else None
                        upsert_unconfirmed("docs/del.txt", "del.txt", MOD_TIME_2, 75)
                        post_del = lookup(del_id) if del_id else None
                        print("[03.6] decided-but-unconfirmed upsert clears any prior deleted_time")
                        if pre_del is None or pre_del.get("deleted_time") is None:
                            failures.append(f"03.6: setup failed — row not tombstoned before upsert: {pre_del!r}")
                        elif post_del is None or post_del.get("deleted_time") is not None:
                            failures.append(f"03.6: deleted_time not cleared after unconfirmed upsert: {post_del!r}")
                    except RuntimeError as e:
                        failures.append(f"03.6: {e}")
                        print(f"[03.6] error: {e}")

                # --- 03.7: mark-copy-completed sets last_seen to ts on existing row ---
                if not t_unconfirmed or not t_completed:
                    print("[03.7] SKIP: missing required tool (unconfirmed-upsert or mark-copy-completed)")
                    failures.append("03.7: missing required tool")
                else:
                    try:
                        upsert_unconfirmed("video/clip.mp4", "clip.mp4", MOD_TIME_1, 88888)
                        mark_copy_completed("video/clip.mp4", CONF_TS_3)
                        clip_id = hash_path("video/clip.mp4")
                        clip_row = lookup(clip_id) if clip_id else None
                        print("[03.7] mark-copy-completed sets last_seen to supplied ts on existing row")
                        if clip_row is None:
                            failures.append("03.7: row not found after mark-copy-completed")
                        elif clip_row.get("last_seen") != CONF_TS_3:
                            failures.append(f"03.7: last_seen={clip_row.get('last_seen')!r}, want {CONF_TS_3!r}")
                    except RuntimeError as e:
                        failures.append(f"03.7: {e}")
                        print(f"[03.7] error: {e}")

                # --- 03.15: decided-but-unconfirmed upsert preserves last_seen ---
                # sub-case A: new path → last_seen stays NULL
                # sub-case B: existing row with prior last_seen → last_seen preserved
                if not t_unconfirmed or not t_confirmed:
                    print("[03.15] SKIP: missing required tool")
                    failures.append("03.15: missing required tool")
                else:
                    try:
                        upsert_unconfirmed("new/fresh.dat", "fresh.dat", MOD_TIME_1, 1)
                        fresh_id = hash_path("new/fresh.dat")
                        fresh_row = lookup(fresh_id) if fresh_id else None

                        upsert_confirmed("archive/old.zip", "old.zip", MOD_TIME_1, 77777, CONF_TS_2)
                        upsert_unconfirmed("archive/old.zip", "old.zip", MOD_TIME_3, 88888)
                        zip_id = hash_path("archive/old.zip")
                        zip_row = lookup(zip_id) if zip_id else None

                        print(
                            "[03.15] decided-but-unconfirmed upsert leaves last_seen NULL on new path "
                            "and preserves prior last_seen on existing row"
                        )
                        if fresh_row is None:
                            failures.append("03.15: row not found after unconfirmed upsert on new path")
                        elif fresh_row.get("last_seen") is not None:
                            failures.append(
                                f"03.15: expected last_seen=NULL on new path, got {fresh_row.get('last_seen')!r}"
                            )
                        if zip_row is None:
                            failures.append("03.15: row not found after unconfirmed upsert on existing row")
                        elif zip_row.get("last_seen") != CONF_TS_2:
                            failures.append(
                                f"03.15: expected prior last_seen={CONF_TS_2!r} preserved, "
                                f"got {zip_row.get('last_seen')!r}"
                            )
                    except RuntimeError as e:
                        failures.append(f"03.15: {e}")
                        print(f"[03.15] error: {e}")

                if t_close:
                    try:
                        call(s, t_close, {"handle": handle})
                    except RuntimeError:
                        pass

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
