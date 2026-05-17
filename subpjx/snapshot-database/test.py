#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

from __future__ import annotations

import json
import re
import socket
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

PROJECT_DIR = Path("C:/Users/human/Desktop/prjx/kitchensync/subpjx/snapshot-database")
JAVA = Path("C:/Users/human/Desktop/prjx/kitchensync/tools/compiler/jdk/bin/java.exe")
MCP_JAR = Path("C:/Users/human/Desktop/prjx/kitchensync/subpjx/snapshot-database/released/snapshot-database_MCP.jar")

failures: list[str] = []
_rpc_id = 0


def next_id() -> int:
    global _rpc_id
    _rpc_id += 1
    return _rpc_id


def check(label: str, condition: bool, detail: str = "") -> None:
    if condition:
        print(f"pass: {label}")
    else:
        msg = f"FAIL: {label}" + (f"\n      {detail}" if detail else "")
        failures.append(msg)
        print(msg)


def drain(stream, sink=None) -> None:
    for line in stream:
        if sink is not None:
            sink.append(line)


def launch_mcp() -> tuple[subprocess.Popen[str], int]:
    proc = subprocess.Popen(
        [str(JAVA), "-jar", str(MCP_JAR)],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    if proc.stdout is None or proc.stderr is None:
        proc.terminate()
        raise RuntimeError("MCP server pipes were not created")

    stderr_buf: list[str] = []
    threading.Thread(target=drain, args=(proc.stderr, stderr_buf), daemon=True).start()

    stdout_buf: list[str] = []
    port = None
    deadline = time.time() + 30
    while time.time() < deadline and proc.poll() is None:
        line = proc.stdout.readline()
        stdout_buf.append(line)
        if line.startswith("MCP_PORT="):
            port = int(line.strip().split("=", 1)[1])
            break
    if port is None:
        proc.terminate()
        try:
            proc.wait(timeout=2)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait(timeout=2)
        raise RuntimeError(
            "MCP server did not advertise MCP_PORT\n"
            f"--- stdout ---\n{''.join(stdout_buf)}\n"
            f"--- stderr ---\n{''.join(stderr_buf)}"
        )

    threading.Thread(target=drain, args=(proc.stdout,), daemon=True).start()
    return proc, port


def rpc(sock: socket.socket, method: str, params=None) -> dict:
    msg_id = next_id()
    message = {"jsonrpc": "2.0", "id": msg_id, "method": method}
    if params is not None:
        message["params"] = params
    sock.sendall((json.dumps(message, separators=(",", ":")) + "\n").encode("utf-8"))
    data = b""
    deadline = time.time() + 10
    while b"\n" not in data and time.time() < deadline:
        chunk = sock.recv(65536)
        if not chunk:
            break
        data += chunk
    line, _, _ = data.partition(b"\n")
    return json.loads(line.decode("utf-8"))


def shutdown_mcp(proc: subprocess.Popen[str], port: int) -> None:
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=3) as s:
            rpc(s, "aitc/shutdown")
    except Exception:
        pass
    try:
        proc.wait(timeout=5)
        return
    except subprocess.TimeoutExpired:
        pass
    proc.terminate()
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait()


def call(sock: socket.socket, tool: str, args: dict) -> dict:
    return rpc(sock, "tools/call", {"name": tool, "arguments": args})


def text(resp: dict) -> str:
    result = resp.get("result")
    if not result:
        return ""
    return json.dumps(result)


def is_err(resp: dict) -> bool:
    return "error" in resp


def run_tests(sock: socket.socket, tmp: Path) -> None:

    # ---- schema creation and close idempotency ----
    db = str(tmp / "t1.db")
    r = call(sock, "open", {"db_path": db})
    check("open creates db", not is_err(r), text(r))
    r = call(sock, "has-rows", {"db_path": db})
    check("new db has no rows", not is_err(r) and "false" in text(r).lower(), text(r))
    # not reasonably testable: exact table name, index names, journal mode via tools/call
    r = call(sock, "close", {"db_path": db})
    check("close succeeds", not is_err(r), text(r))
    r = call(sock, "close", {"db_path": db})
    check("close is idempotent", not is_err(r), text(r))

    # ---- path IDs (SPEC examples) ----
    db = str(tmp / "t2.db")
    call(sock, "open", {"db_path": db})
    r = call(sock, "path-id", {"relative_path": "docs"})
    check("path_id(docs)=H41WPg3SlMv", not is_err(r) and "H41WPg3SlMv" in text(r), text(r))
    r = call(sock, "path-id", {"relative_path": "docs/readme.txt"})
    check("path_id(docs/readme.txt)=K5EzsWuLZ04", not is_err(r) and "K5EzsWuLZ04" in text(r), text(r))
    r = call(sock, "root-parent-id", {})
    check("root_parent_id=JyBskcNRrBK", not is_err(r) and "JyBskcNRrBK" in text(r), text(r))
    call(sock, "close", {"db_path": db})

    # ---- invalid path errors, no rows written ----
    db = str(tmp / "t3.db")
    call(sock, "open", {"db_path": db})
    ts = "2026-05-15_10-00-00_000000Z"

    for bad, label in [
        ("", "empty"),
        ("/abs", "leading-slash"),
        ("a/", "trailing-slash"),
        ("a//b", "empty-segment"),
        ("a\x00b", "nul-byte"),
    ]:
        r = call(sock, "record-present",
                 {"db_path": db, "relative_path": bad,
                  "kind": "file", "mod_time": ts, "byte_size": 10, "seen_at": ts})
        check(f"invalid_path {label}", is_err(r) or "invalid" in text(r).lower(), text(r))

    # root directory itself
    r = call(sock, "record-present",
             {"db_path": db, "relative_path": ".",
              "kind": "file", "mod_time": ts, "byte_size": 10, "seen_at": ts})
    check("invalid_path root-dir-itself", is_err(r) or "invalid" in text(r).lower(), text(r))

    # invalid timestamp in mod_time
    r = call(sock, "record-present",
             {"db_path": db, "relative_path": "f.txt",
              "kind": "file", "mod_time": "2026-05-15T10:00:00Z", "byte_size": 10, "seen_at": ts})
    check("invalid_timestamp in mod_time", is_err(r) or "invalid" in text(r).lower(), text(r))

    # invalid timestamp in seen_at
    r = call(sock, "record-present",
             {"db_path": db, "relative_path": "f.txt",
              "kind": "file", "mod_time": ts, "byte_size": 10, "seen_at": "not-a-ts"})
    check("invalid_timestamp in seen_at", is_err(r) or "invalid" in text(r).lower(), text(r))

    # invalid metadata: negative file size
    r = call(sock, "record-present",
             {"db_path": db, "relative_path": "f.txt",
              "kind": "file", "mod_time": ts, "byte_size": -5, "seen_at": ts})
    check("invalid_metadata negative file size", is_err(r) or "invalid" in text(r).lower(), text(r))

    # invalid metadata: directory byte_size not -1
    r = call(sock, "record-present",
             {"db_path": db, "relative_path": "d",
              "kind": "directory", "mod_time": ts, "byte_size": 0, "seen_at": ts})
    check("invalid_metadata dir byte_size not -1", is_err(r) or "invalid" in text(r).lower(), text(r))

    r = call(sock, "has-rows", {"db_path": db})
    check("invalid calls write no rows", not is_err(r) and "false" in text(r).lower(), text(r))
    call(sock, "close", {"db_path": db})

    # ---- record_present: insert, lookup fields, update, clear tombstone ----
    db = str(tmp / "t4.db")
    call(sock, "open", {"db_path": db})
    ts_mod = "2026-05-15_10-00-00_000000Z"
    ts_s1  = "2026-05-15_10-00-05_000000Z"

    r = call(sock, "record-present",
             {"db_path": db, "relative_path": "docs/readme.txt",
              "kind": "file", "mod_time": ts_mod, "byte_size": 12, "seen_at": ts_s1})
    check("record_present inserts file row", not is_err(r), text(r))

    r = call(sock, "lookup", {"db_path": db, "relative_path": "docs/readme.txt"})
    t = text(r)
    check("lookup id=K5EzsWuLZ04",        "K5EzsWuLZ04"  in t, t)
    check("lookup parent_id=H41WPg3SlMv", "H41WPg3SlMv"  in t, t)
    check("lookup basename=readme.txt",   "readme.txt"    in t, t)
    check("lookup kind=file",             "file"          in t.lower(), t)
    check("lookup mod_time",              ts_mod          in t, t)
    check("lookup byte_size=12",          "12"            in t, t)
    check("lookup last_seen",             ts_s1           in t, t)

    # directory row
    r = call(sock, "record-present",
             {"db_path": db, "relative_path": "docs",
              "kind": "directory", "mod_time": ts_mod, "byte_size": -1, "seen_at": ts_s1})
    check("record_present inserts dir row", not is_err(r), text(r))
    r = call(sock, "lookup", {"db_path": db, "relative_path": "docs"})
    t = text(r)
    check("dir kind=directory",          "directory"   in t.lower(), t)
    check("dir byte_size=-1",            "-1"          in t, t)
    check("dir parent_id=root sentinel", "JyBskcNRrBK" in t, t)

    # tombstone then record_present clears it and updates last_seen
    call(sock, "mark-absent",
         {"db_path": db, "relative_path": "docs/readme.txt"})
    ts_s2 = "2026-05-15_11-00-00_000000Z"
    r = call(sock, "record-present",
             {"db_path": db, "relative_path": "docs/readme.txt",
              "kind": "file", "mod_time": ts_mod, "byte_size": 12, "seen_at": ts_s2})
    check("record_present clears tombstone", not is_err(r), text(r))
    r = call(sock, "lookup", {"db_path": db, "relative_path": "docs/readme.txt"})
    t = text(r)
    check("record_present updated last_seen", ts_s2 in t, t)
    # ts_s1 was the deleted_time value set by mark_absent; after clearing it must not appear
    check("tombstone deleted_time cleared", ts_s1 not in t, t)
    call(sock, "close", {"db_path": db})

    # ---- record_copy_pending + confirm_copy_completed ----
    db = str(tmp / "t5.db")
    call(sock, "open", {"db_path": db})
    ts_mod2 = "2026-05-15_11-00-00_000000Z"

    r = call(sock, "record-copy-pending",
             {"db_path": db, "relative_path": "pending.bin",
              "kind": "file", "mod_time": ts_mod2, "byte_size": 5})
    check("record_copy_pending inserts", not is_err(r), text(r))

    r = call(sock, "lookup", {"db_path": db, "relative_path": "pending.bin"})
    t = text(r)
    check("pending id=IdWzugtOkpp",        "IdWzugtOkpp" in t, t)
    check("pending parent_id=JyBskcNRrBK", "JyBskcNRrBK" in t, t)
    check("pending last_seen=absent",       "absent" in t.lower() or "null" in t.lower(), t)

    # update metadata; last_seen must remain absent
    r = call(sock, "record-copy-pending",
             {"db_path": db, "relative_path": "pending.bin",
              "kind": "file", "mod_time": ts_mod2, "byte_size": 99})
    check("record_copy_pending update", not is_err(r), text(r))
    r = call(sock, "lookup", {"db_path": db, "relative_path": "pending.bin"})
    t = text(r)
    check("copy_pending last_seen still absent after update",
          "absent" in t.lower() or "null" in t.lower(), t)
    check("copy_pending updated byte_size", "99" in t, t)

    ts_comp = "2026-05-15_11-00-09_000000Z"
    r = call(sock, "confirm-copy-completed",
             {"db_path": db, "relative_path": "pending.bin", "seen_at": ts_comp})
    check("confirm_copy_completed succeeds", not is_err(r), text(r))
    r = call(sock, "lookup", {"db_path": db, "relative_path": "pending.bin"})
    t = text(r)
    check("copy completed sets last_seen", ts_comp in t, t)

    r = call(sock, "confirm-copy-completed",
             {"db_path": db, "relative_path": "missing.bin", "seen_at": ts_comp})
    check("confirm_copy_completed not_found",
          is_err(r) or "not_found" in text(r).lower(), text(r))
    call(sock, "close", {"db_path": db})

    # ---- mark_absent: set, idempotent, preserve existing tombstone, missing row ----
    db = str(tmp / "t6.db")
    call(sock, "open", {"db_path": db})
    ts_ls = "2026-05-15_09-00-01_000000Z"
    call(sock, "record-present",
         {"db_path": db, "relative_path": "f.txt",
          "kind": "file", "mod_time": ts_ls, "byte_size": 1, "seen_at": ts_ls})

    r = call(sock, "mark-absent", {"db_path": db, "relative_path": "f.txt"})
    check("mark_absent succeeds", not is_err(r), text(r))
    r = call(sock, "lookup", {"db_path": db, "relative_path": "f.txt"})
    t = text(r)
    # deleted_time is set to the prior last_seen value (ts_ls); ts_ls appears for both fields
    check("mark_absent deleted_time=last_seen", t.count(ts_ls) >= 2, t)

    # idempotent: second mark_absent must not change the tombstone
    r = call(sock, "mark-absent", {"db_path": db, "relative_path": "f.txt"})
    check("mark_absent idempotent", not is_err(r), text(r))
    r = call(sock, "lookup", {"db_path": db, "relative_path": "f.txt"})
    check("mark_absent preserves tombstone", ts_ls in text(r), text(r))

    r = call(sock, "mark-absent", {"db_path": db, "relative_path": "no-such.txt"})
    check("mark_absent missing row succeeds", not is_err(r), text(r))
    call(sock, "close", {"db_path": db})

    # ---- mark_displaced cascade (SPEC example) ----
    db = str(tmp / "t7.db")
    call(sock, "open", {"db_path": db})
    ts_a = "2026-05-15_09-00-00_000000Z"
    ts_r = "2026-05-15_09-00-01_000000Z"
    ts_j = "2026-05-15_09-00-02_000000Z"
    ts_o = "2026-05-15_08-00-00_000000Z"

    call(sock, "record-present",
         {"db_path": db, "relative_path": "album",
          "kind": "directory", "mod_time": ts_a, "byte_size": -1, "seen_at": ts_a})
    call(sock, "record-present",
         {"db_path": db, "relative_path": "album/raw",
          "kind": "directory", "mod_time": ts_r, "byte_size": -1, "seen_at": ts_r})
    call(sock, "record-present",
         {"db_path": db, "relative_path": "album/raw/a.jpg",
          "kind": "file", "mod_time": ts_j, "byte_size": 1024, "seen_at": ts_j})
    call(sock, "record-present",
         {"db_path": db, "relative_path": "old.txt",
          "kind": "file", "mod_time": ts_o, "byte_size": 50, "seen_at": ts_o})

    r = call(sock, "mark-displaced", {"db_path": db, "relative_path": "album"})
    check("mark_displaced succeeds", not is_err(r), text(r))

    # album: last_seen=ts_a and deleted_time=ts_a -- ts_a must appear at least twice
    r = call(sock, "lookup", {"db_path": db, "relative_path": "album"})
    t = text(r)
    check("album deleted_time=ts_a", t.count(ts_a) >= 2, t)

    # album/raw: last_seen=ts_r, deleted_time=ts_a -- ts_a is only from deleted_time
    r = call(sock, "lookup", {"db_path": db, "relative_path": "album/raw"})
    t = text(r)
    check("album/raw cascaded deleted_time=ts_a", ts_a in t, t)

    # album/raw/a.jpg: last_seen=ts_j, deleted_time=ts_a
    r = call(sock, "lookup", {"db_path": db, "relative_path": "album/raw/a.jpg"})
    t = text(r)
    check("album/raw/a.jpg cascaded deleted_time=ts_a", ts_a in t, t)

    # old.txt: last_seen=ts_o, deleted_time=absent -- ts_a must not appear
    r = call(sock, "lookup", {"db_path": db, "relative_path": "old.txt"})
    t = text(r)
    check("old.txt unaffected by cascade", not is_err(r) and ts_a not in t, t)

    r = call(sock, "mark-displaced", {"db_path": db, "relative_path": "nonexistent"})
    check("mark_displaced missing row succeeds", not is_err(r), text(r))
    call(sock, "close", {"db_path": db})

    # ---- purge: old tombstone, old live, absent last_seen, preserve fresh ----
    db = str(tmp / "t8.db")
    call(sock, "open", {"db_path": db})
    ts_stale = "2026-05-01_00-00-00_000000Z"
    ts_fresh = "2026-05-17_00-00-00_000000Z"
    ts_cut   = "2026-05-10_00-00-00_000000Z"

    call(sock, "record-present",
         {"db_path": db, "relative_path": "stale_live.txt",
          "kind": "file", "mod_time": ts_stale, "byte_size": 1, "seen_at": ts_stale})
    call(sock, "record-present",
         {"db_path": db, "relative_path": "stale_dead.txt",
          "kind": "file", "mod_time": ts_stale, "byte_size": 1, "seen_at": ts_stale})
    call(sock, "mark-absent",
         {"db_path": db, "relative_path": "stale_dead.txt"})
    call(sock, "record-copy-pending",
         {"db_path": db, "relative_path": "no_ls.txt",
          "kind": "file", "mod_time": ts_stale, "byte_size": 1})
    call(sock, "record-present",
         {"db_path": db, "relative_path": "fresh.txt",
          "kind": "file", "mod_time": ts_fresh, "byte_size": 1, "seen_at": ts_fresh})

    r = call(sock, "purge", {"db_path": db, "cutoff_time": ts_cut})
    check("purge succeeds", not is_err(r), text(r))
    check("purge deleted 3 rows", "3" in text(r), text(r))

    r = call(sock, "lookup", {"db_path": db, "relative_path": "fresh.txt"})
    check("purge preserves fresh row", not is_err(r) and ts_fresh in text(r), text(r))

    r = call(sock, "lookup", {"db_path": db, "relative_path": "stale_live.txt"})
    t = text(r)
    check("purge removed stale_live",
          is_err(r) or "absent" in t.lower() or "null" in t.lower() or not t.strip(), t)
    call(sock, "close", {"db_path": db})

    # ---- SnapshotTimestampGenerator: format and strict monotonicity ----
    r = call(sock, "generate-timestamp", {})
    t1 = r.get("result", {}).get("timestamp", "")
    check("timestamp format",
          bool(re.fullmatch(r"\d{4}-\d{2}-\d{2}_\d{2}-\d{2}-\d{2}_\d{6}Z", t1)),
          f"got {t1!r}")
    r = call(sock, "generate-timestamp", {})
    t2 = r.get("result", {}).get("timestamp", "")
    check("timestamp strictly increasing", t2 > t1, f"{t2!r} > {t1!r}")

    # ---- failed transaction leaves committed rows observable ----
    db = str(tmp / "t9.db")
    call(sock, "open", {"db_path": db})
    ts_x = "2026-05-15_10-00-00_000000Z"
    call(sock, "record-present",
         {"db_path": db, "relative_path": "good.txt",
          "kind": "file", "mod_time": ts_x, "byte_size": 7, "seen_at": ts_x})
    # invalid call (empty path) -- must fail without affecting the committed row
    call(sock, "record-present",
         {"db_path": db, "relative_path": "",
          "kind": "file", "mod_time": ts_x, "byte_size": 7, "seen_at": ts_x})
    r = call(sock, "lookup", {"db_path": db, "relative_path": "good.txt"})
    check("failed op leaves committed row observable",
          not is_err(r) and "7" in text(r), text(r))
    call(sock, "close", {"db_path": db})

    # not reasonably testable: no stdout/stderr from public operations -- the MCP wrapper
    # infrastructure writes to stdout (MCP_PORT=...); individual Java library calls cannot
    # be isolated from wrapper I/O through tools/call alone.


def main() -> None:
    proc, port = launch_mcp()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as sock:
            with tempfile.TemporaryDirectory() as tmpdir:
                run_tests(sock, Path(tmpdir))
    finally:
        shutdown_mcp(proc, port)

    if failures:
        print(f"\n{len(failures)} failure(s):")
        for f in failures:
            print(f"  {f}")
        sys.exit(1)
    else:
        print("\nAll checks passed.")
        sys.exit(0)


if __name__ == "__main__":
    main()
