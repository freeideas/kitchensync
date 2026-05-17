#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

from __future__ import annotations

import json
import socket
import subprocess
import sys
import threading
import time
from pathlib import Path
from typing import Any

PROJECT_DIR = Path("C:/Users/human/Desktop/prjx/kitchensync/subpjx/sync-decision-engine")
JAVA = Path("C:/Users/human/Desktop/prjx/kitchensync/tools/compiler/jdk/bin/java.exe")
MCP_JAR = Path("C:/Users/human/Desktop/prjx/kitchensync/subpjx/sync-decision-engine/released/sync-decision-engine_MCP.jar")

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

# Shared timestamp constants
T0 = "2026-05-15T10:00:00Z"
T_PLUS_4 = "2026-05-15T10:00:04Z"
T_PLUS_5 = "2026-05-15T10:00:05Z"
T_PLUS_6 = "2026-05-15T10:00:06Z"
T_PLUS_20 = "2026-05-15T10:00:20Z"
T_OLD = "2026-05-15T09:00:00Z"
T_SEEN = "2026-05-15T11:00:00Z"


# ---------------------------------------------------------------------------
# MCP harness
# ---------------------------------------------------------------------------

def drain(stream: Any, sink: list[str] | None = None) -> None:
    for line in stream:
        if sink is not None:
            sink.append(line)


def launch_mcp() -> tuple[subprocess.Popen[str], int, list[str], list[str]]:
    proc = subprocess.Popen(
        [str(JAVA), "-jar", str(MCP_JAR)],
        cwd=str(PROJECT_DIR),
        stdin=subprocess.DEVNULL,
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

    startup_stdout: list[str] = []
    port: int | None = None
    deadline = time.time() + 30
    while time.time() < deadline and proc.poll() is None:
        line = proc.stdout.readline()
        startup_stdout.append(line)
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
            f"--- stdout ---\n{''.join(startup_stdout)}\n"
            f"--- stderr ---\n{''.join(stderr_buf)}"
        )

    stdout_buf: list[str] = []
    threading.Thread(target=drain, args=(proc.stdout, stdout_buf), daemon=True).start()
    return proc, port, stdout_buf, stderr_buf


def rpc(sock: socket.socket, method: str, params: Any = None, rpc_id: int = 1) -> dict[str, Any]:
    msg: dict[str, Any] = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
    if params is not None:
        msg["params"] = params
    sock.sendall((json.dumps(msg, separators=(",", ":")) + "\n").encode("utf-8"))
    data = b""
    deadline = time.time() + 10
    while b"\n" not in data and time.time() < deadline:
        chunk = sock.recv(65536)
        if not chunk:
            break
        data += chunk
    line, _, _ = data.partition(b"\n")
    if not line:
        raise RuntimeError(f"No response for {method}")
    return json.loads(line.decode("utf-8"))


def rpc_raw(sock: socket.socket, raw_line: str) -> dict[str, Any]:
    sock.sendall((raw_line + "\n").encode("utf-8"))
    data = b""
    deadline = time.time() + 10
    while b"\n" not in data and time.time() < deadline:
        chunk = sock.recv(65536)
        if not chunk:
            break
        data += chunk
    line, _, _ = data.partition(b"\n")
    if not line:
        raise RuntimeError("No response")
    return json.loads(line.decode("utf-8"))


def shutdown_mcp(proc: subprocess.Popen[str], port: int) -> None:
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=3) as sock:
            rpc(sock, "aitc/shutdown", rpc_id=999)
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


# ---------------------------------------------------------------------------
# Input builders
# ---------------------------------------------------------------------------

def peers(*args: tuple[str, str]) -> dict[str, str]:
    """Build ordered peers map: peers(("A", "normal"), ("B", "canon"))."""
    return {p: r for p, r in args}


def file_entry(mod_time: str, byte_size: int) -> dict[str, Any]:
    return {"kind": "file", "mod_time": mod_time, "byte_size": byte_size}


def dir_entry(mod_time: str) -> dict[str, Any]:
    return {"kind": "directory", "mod_time": mod_time, "byte_size": -1}


def file_row(
    mod_time: str,
    byte_size: int,
    last_seen: str | None = T_SEEN,
    deleted_time: str | None = None,
) -> dict[str, Any]:
    row: dict[str, Any] = {"kind": "file", "mod_time": mod_time, "byte_size": byte_size}
    if last_seen is not None:
        row["last_seen"] = last_seen
    if deleted_time is not None:
        row["deleted_time"] = deleted_time
    return row


def dir_row(
    mod_time: str,
    last_seen: str | None = T_SEEN,
    deleted_time: str | None = None,
) -> dict[str, Any]:
    row: dict[str, Any] = {"kind": "directory", "mod_time": mod_time, "byte_size": -1}
    if last_seen is not None:
        row["last_seen"] = last_seen
    if deleted_time is not None:
        row["deleted_time"] = deleted_time
    return row


def inp(
    path: str,
    peer_list: dict[str, str],
    live: dict[str, Any] | None = None,
    rows: dict[str, Any] | None = None,
) -> dict[str, Any]:
    return {
        "relative_path": path,
        "peers": peer_list,
        "live_entries": live or {},
        "snapshot_rows": rows or {},
    }


# ---------------------------------------------------------------------------
# Response parsing
# ---------------------------------------------------------------------------

def call_decide(sock: socket.socket, payload: dict[str, Any], rpc_id: int) -> dict[str, Any]:
    return rpc(sock, "tools/call", {"name": "decide-entry", "arguments": payload}, rpc_id=rpc_id)


def parse_result(response: dict[str, Any]) -> Any:
    if "error" in response:
        return response["error"]
    result = response.get("result", {})
    if isinstance(result, dict):
        if "structuredContent" in result:
            return result["structuredContent"]
        content = result.get("content", [])
        if isinstance(content, list) and content:
            text = "\n".join(
                item.get("text", "") for item in content
                if isinstance(item, dict) and item.get("type") == "text"
            ).strip()
            if text:
                try:
                    return json.loads(text)
                except json.JSONDecodeError:
                    return text
        if "result" in result:
            return result["result"]
    return result


def _effect_name(e: Any) -> str:
    if isinstance(e, str):
        return e.lower()
    if isinstance(e, dict):
        for k in ("effect", "kind", "type", "name"):
            if k in e:
                return str(e[k]).lower()
    return str(e).lower()


def fs(r: Any, peer: str) -> list[str]:
    if not isinstance(r, dict):
        return []
    effects = r.get("filesystem_effects", {}).get(peer, [])
    if isinstance(effects, (str, dict)):
        effects = [effects]
    return [_effect_name(e) for e in effects]


def snap(r: Any, peer: str) -> list[str]:
    if not isinstance(r, dict):
        return []
    effects = r.get("snapshot_effects", {}).get(peer, [])
    if isinstance(effects, (str, dict)):
        effects = [effects]
    return [_effect_name(e) for e in effects]


def auth_kind(r: Any) -> str:
    if not isinstance(r, dict):
        return ""
    state = r.get("authoritative_state", r)
    if isinstance(state, str):
        return state.lower()
    if isinstance(state, dict):
        return str(state.get("kind") or state.get("type") or "").lower()
    return ""


def auth_source(r: Any) -> str:
    if not isinstance(r, dict):
        return ""
    state = r.get("authoritative_state", r)
    if isinstance(state, dict):
        return str(state.get("source_peer") or state.get("sourcePeer") or "")
    return ""


def auth_byte_size(r: Any) -> int | None:
    if not isinstance(r, dict):
        return None
    state = r.get("authoritative_state", r)
    if isinstance(state, dict):
        v = state.get("byte_size") or state.get("byteSize")
        return int(v) if v is not None else None
    return None


def recurse(r: Any) -> set[str]:
    if not isinstance(r, dict):
        return set()
    return {str(p) for p in r.get("recurse_peers", [])}


def is_skipped(r: Any) -> bool:
    return bool(r.get("skipped")) if isinstance(r, dict) else False


def has_invalid_input(response: dict[str, Any]) -> bool:
    return "invalid_input" in json.dumps(response).lower()


# ---------------------------------------------------------------------------
# Assertion helpers
# ---------------------------------------------------------------------------

def check(failures: list[str], label: str, cond: bool, detail: str = "") -> None:
    status = "PASS" if cond else "FAIL"
    msg = f"{status} {label}"
    if not cond and detail:
        msg += f": {detail}"
    print(msg)
    if not cond:
        failures.append(label + (f": {detail}" if detail else ""))


# ---------------------------------------------------------------------------
# Test scenarios
# ---------------------------------------------------------------------------

def run_tests(sock: socket.socket, stdout_buf: list[str], stderr_buf: list[str]) -> list[str]:
    failures: list[str] = []
    rpc_id = [1]

    def nid() -> int:
        rpc_id[0] += 1
        return rpc_id[0]

    def decide(payload: dict[str, Any]) -> Any:
        return parse_result(call_decide(sock, payload, nid()))

    # -----------------------------------------------------------------------
    # SPEC example 1: modified file beats unchanged; filesystem and snapshot effects
    # -----------------------------------------------------------------------
    r = decide(inp(
        "notes/todo.txt",
        peers(("A", "normal"), ("B", "normal")),
        live={"A": file_entry(T0, 4), "B": file_entry("2026-05-15T10:05:30Z", 8)},
        rows={
            "A": file_row(T0, 4, last_seen="2026-05-15T10:01:00Z"),
            "B": file_row(T0, 4, last_seen="2026-05-15T10:01:00Z"),
        },
    ))
    check(failures, "spec_ex1/auth_file", auth_kind(r) == "file", repr(r))
    check(failures, "spec_ex1/source_B", auth_source(r) == "B", repr(r))
    check(failures, "spec_ex1/A_copy_file", fs(r, "A") == ["copy_file"], repr(fs(r, "A")))
    check(failures, "spec_ex1/B_keep", fs(r, "B") == ["keep"], repr(fs(r, "B")))
    check(failures, "spec_ex1/A_copy_pending", snap(r, "A") == ["copy_pending"], repr(snap(r, "A")))
    check(failures, "spec_ex1/B_confirm_present", snap(r, "B") == ["confirm_present"], repr(snap(r, "B")))
    check(failures, "spec_ex1/not_skipped", is_skipped(r) is False, repr(r))

    # -----------------------------------------------------------------------
    # SPEC example 2: absent_unconfirmed last_seen >5s later => deletion wins
    # -----------------------------------------------------------------------
    r = decide(inp(
        "old.txt",
        peers(("A", "normal"), ("B", "normal")),
        live={"B": file_entry(T0, 12)},
        rows={
            "A": file_row(T_OLD, 12, last_seen=T_SEEN),
            "B": file_row(T0, 12, last_seen="2026-05-15T10:01:00Z"),
        },
    ))
    check(failures, "spec_ex2/auth_absent", auth_kind(r) == "absent", repr(r))
    check(failures, "spec_ex2/A_keep", fs(r, "A") == ["keep"], repr(fs(r, "A")))
    check(failures, "spec_ex2/B_displace", fs(r, "B") == ["displace"], repr(fs(r, "B")))
    check(failures, "spec_ex2/A_mark_absent", snap(r, "A") == ["mark_absent"], repr(snap(r, "A")))
    check(failures, "spec_ex2/B_mark_displaced", snap(r, "B") == ["mark_displaced"], repr(snap(r, "B")))

    # -----------------------------------------------------------------------
    # SPEC example 3: directory with subordinate wrong-type peer
    # -----------------------------------------------------------------------
    r = decide(inp(
        "album",
        peers(("A", "normal"), ("B", "normal"), ("C", "subordinate")),
        live={
            "A": dir_entry("2026-05-15T12:00:00Z"),
            "C": file_entry("2026-05-15T12:30:00Z", 99),
        },
    ))
    check(failures, "spec_ex3/auth_dir", auth_kind(r) == "directory", repr(r))
    check(failures, "spec_ex3/A_keep", fs(r, "A") == ["keep"], repr(fs(r, "A")))
    check(failures, "spec_ex3/B_create_dir", fs(r, "B") == ["create_directory"], repr(fs(r, "B")))
    check(failures, "spec_ex3/C_displace_create", fs(r, "C") == ["displace", "create_directory"], repr(fs(r, "C")))
    check(failures, "spec_ex3/A_confirm_present", snap(r, "A") == ["confirm_present"], repr(snap(r, "A")))
    check(failures, "spec_ex3/B_create_dir_confirmed", snap(r, "B") == ["create_directory_confirmed"], repr(snap(r, "B")))
    check(failures, "spec_ex3/C_displaced_then_created", "mark_displaced" in snap(r, "C") and "create_directory_confirmed" in snap(r, "C"), repr(snap(r, "C")))
    check(failures, "spec_ex3/recurse_all", recurse(r) == {"A", "B", "C"}, repr(recurse(r)))

    # -----------------------------------------------------------------------
    # Canon file wins unconditionally (even when normal peer has newer file)
    # -----------------------------------------------------------------------
    r = decide(inp(
        "canon/doc.txt",
        peers(("canon", "canon"), ("peer", "normal")),
        live={"canon": file_entry(T0, 100), "peer": file_entry(T_PLUS_20, 200)},
    ))
    check(failures, "canon_file/source", auth_source(r) == "canon", repr(r))
    check(failures, "canon_file/byte_size", auth_byte_size(r) == 100, repr(r))
    check(failures, "canon_file/canon_keep", fs(r, "canon") == ["keep"], repr(fs(r, "canon")))
    check(failures, "canon_file/peer_copy", fs(r, "peer") == ["copy_file"], repr(fs(r, "peer")))
    check(failures, "canon_file/peer_copy_pending", snap(r, "peer") == ["copy_pending"], repr(snap(r, "peer")))

    # -----------------------------------------------------------------------
    # Canon directory wins over type conflict (normal has file)
    # -----------------------------------------------------------------------
    r = decide(inp(
        "canon/photos",
        peers(("canon", "canon"), ("peer", "normal")),
        live={"canon": dir_entry(T0), "peer": file_entry(T_PLUS_20, 50)},
    ))
    check(failures, "canon_dir/auth_dir", auth_kind(r) == "directory", repr(r))
    check(failures, "canon_dir/peer_displace_first", fs(r, "peer")[0:1] == ["displace"], repr(fs(r, "peer")))
    check(failures, "canon_dir/peer_create_dir", "create_directory" in fs(r, "peer"), repr(fs(r, "peer")))
    check(failures, "canon_dir/peer_snap_displaced", "mark_displaced" in snap(r, "peer"), repr(snap(r, "peer")))
    check(failures, "canon_dir/peer_snap_create", "create_directory_confirmed" in snap(r, "peer"), repr(snap(r, "peer")))

    # -----------------------------------------------------------------------
    # Canon absence wins; normal and subordinate live entries are displaced
    # -----------------------------------------------------------------------
    r = decide(inp(
        "canon/gone",
        peers(("canon", "canon"), ("B", "normal"), ("C", "subordinate")),
        live={"B": file_entry(T0, 4), "C": dir_entry(T0)},
    ))
    check(failures, "canon_absent/auth_absent", auth_kind(r) == "absent", repr(r))
    check(failures, "canon_absent/B_displace", fs(r, "B") == ["displace"], repr(fs(r, "B")))
    check(failures, "canon_absent/C_displace", fs(r, "C") == ["displace"], repr(fs(r, "C")))
    check(failures, "canon_absent/B_mark_displaced", snap(r, "B") == ["mark_displaced"], repr(snap(r, "B")))
    check(failures, "canon_absent/C_mark_displaced", snap(r, "C") == ["mark_displaced"], repr(snap(r, "C")))

    # -----------------------------------------------------------------------
    # No active contributing peers (only subordinate) => skipped=true, no effects
    # -----------------------------------------------------------------------
    r = decide(inp(
        "subtree/file.txt",
        peers(("S", "subordinate")),
        live={"S": file_entry(T0, 5)},
    ))
    check(failures, "no_contrib/skipped_true", is_skipped(r) is True, repr(r))
    check(failures, "no_contrib/no_fs", not r.get("filesystem_effects") if isinstance(r, dict) else True, repr(r))
    check(failures, "no_contrib/no_snap", not r.get("snapshot_effects") if isinstance(r, dict) else True, repr(r))

    # -----------------------------------------------------------------------
    # Unchanged files: matching metadata within 5s => keep + confirm_present
    # -----------------------------------------------------------------------
    r = decide(inp(
        "files/unchanged.txt",
        peers(("A", "normal"), ("B", "normal"), ("C", "normal")),
        live={"A": file_entry(T0, 4), "B": file_entry(T_PLUS_4, 4)},
        rows={"A": file_row(T0, 4), "B": file_row(T0, 4)},
    ))
    check(failures, "unchanged/A_keep", fs(r, "A") == ["keep"], repr(fs(r, "A")))
    check(failures, "unchanged/B_keep", fs(r, "B") == ["keep"], repr(fs(r, "B")))
    check(failures, "unchanged/A_confirm", snap(r, "A") == ["confirm_present"], repr(snap(r, "A")))
    check(failures, "unchanged/C_copy_file", fs(r, "C") == ["copy_file"], repr(fs(r, "C")))
    check(failures, "unchanged/C_copy_pending", snap(r, "C") == ["copy_pending"], repr(snap(r, "C")))

    # -----------------------------------------------------------------------
    # Modified file beats later unchanged file
    # -----------------------------------------------------------------------
    r = decide(inp(
        "files/modified.txt",
        peers(("A", "normal"), ("B", "normal")),
        live={"A": file_entry(T_PLUS_20, 20), "B": file_entry(T0, 8)},
        rows={"A": file_row(T_PLUS_20, 20), "B": file_row(T_OLD, 8)},
    ))
    check(failures, "modified/source_B", auth_source(r) == "B", repr(r))
    check(failures, "modified/A_copy", fs(r, "A") == ["copy_file"], repr(fs(r, "A")))
    check(failures, "modified/B_keep", fs(r, "B") == ["keep"], repr(fs(r, "B")))

    # -----------------------------------------------------------------------
    # New file (no snapshot row) -- wins and causes copy to missing peer
    # -----------------------------------------------------------------------
    r = decide(inp(
        "files/new.txt",
        peers(("A", "normal"), ("B", "normal")),
        live={"A": file_entry(T0, 30)},
    ))
    check(failures, "new_file/auth_file", auth_kind(r) == "file", repr(r))
    check(failures, "new_file/source_A", auth_source(r) == "A", repr(r))
    check(failures, "new_file/B_copy", fs(r, "B") == ["copy_file"], repr(fs(r, "B")))
    check(failures, "new_file/A_confirm", snap(r, "A") == ["confirm_present"], repr(snap(r, "A")))
    check(failures, "new_file/B_copy_pending", snap(r, "B") == ["copy_pending"], repr(snap(r, "B")))

    # -----------------------------------------------------------------------
    # Live file with tombstoned row is classified modified (not deleted)
    # -----------------------------------------------------------------------
    r = decide(inp(
        "files/tombstoned-live.txt",
        peers(("A", "normal"), ("B", "normal")),
        live={"A": file_entry(T0, 5), "B": file_entry(T_PLUS_20, 9)},
        rows={
            "A": file_row(T0, 5, last_seen=T_OLD, deleted_time=T_OLD),
            "B": file_row(T_PLUS_20, 9),
        },
    ))
    check(failures, "tombstoned_live/source_A", auth_source(r) == "A", repr(r))
    check(failures, "tombstoned_live/B_copy", fs(r, "B") == ["copy_file"], repr(fs(r, "B")))

    # -----------------------------------------------------------------------
    # Deleted (tombstone) beats live file only when >5s later
    # -----------------------------------------------------------------------
    r = decide(inp(
        "files/del-wins.txt",
        peers(("A", "normal"), ("B", "normal")),
        live={"B": file_entry(T0, 12)},
        rows={
            "A": file_row(T_OLD, 12, last_seen=T_OLD, deleted_time=T_PLUS_6),
            "B": file_row(T0, 12),
        },
    ))
    check(failures, "deleted_wins/absent", auth_kind(r) == "absent", repr(r))
    check(failures, "deleted_wins/B_displace", fs(r, "B") == ["displace"], repr(fs(r, "B")))
    check(failures, "deleted_wins/B_mark_displaced", snap(r, "B") == ["mark_displaced"], repr(snap(r, "B")))

    # -----------------------------------------------------------------------
    # Exactly 5s difference is a tie -- existence wins over deletion
    # -----------------------------------------------------------------------
    r = decide(inp(
        "files/tie-5s.txt",
        peers(("A", "normal"), ("B", "normal")),
        live={"B": file_entry(T0, 12)},
        rows={
            "A": file_row(T_OLD, 12, last_seen=T_PLUS_5),
            "B": file_row(T0, 12),
        },
    ))
    check(failures, "tie_5s/file_wins", auth_kind(r) == "file", repr(r))
    check(failures, "tie_5s/A_copy", fs(r, "A") == ["copy_file"], repr(fs(r, "A")))

    # -----------------------------------------------------------------------
    # 5s tombstone tie -- existence wins
    # -----------------------------------------------------------------------
    r = decide(inp(
        "files/tombstone-tie-5s.txt",
        peers(("A", "normal"), ("B", "normal")),
        live={"B": file_entry(T0, 12)},
        rows={
            "A": file_row(T_OLD, 12, last_seen=T_OLD, deleted_time=T_PLUS_5),
            "B": file_row(T0, 12),
        },
    ))
    check(failures, "tombstone_tie_5s/file_wins", auth_kind(r) == "file", repr(r))

    # -----------------------------------------------------------------------
    # absent_unconfirmed with last_seen NOT later than file time => does not vote deletion
    # -----------------------------------------------------------------------
    r = decide(inp(
        "files/absent-unconf-no-vote.txt",
        peers(("A", "normal"), ("B", "normal")),
        live={"B": file_entry(T0, 12)},
        rows={
            "A": file_row(T_OLD, 12, last_seen=T_OLD),
            "B": file_row(T0, 12),
        },
    ))
    check(failures, "absent_unconf_no_vote/file_wins", auth_kind(r) == "file", repr(r))
    check(failures, "absent_unconf_no_vote/A_copy", fs(r, "A") == ["copy_file"], repr(fs(r, "A")))

    # -----------------------------------------------------------------------
    # absent_unconfirmed with last_seen absent => does not vote deletion
    # -----------------------------------------------------------------------
    r = decide(inp(
        "files/absent-no-last-seen.txt",
        peers(("A", "normal"), ("B", "normal")),
        live={"B": file_entry(T0, 12)},
        rows={
            "A": file_row(T_OLD, 12, last_seen=None),
            "B": file_row(T0, 12),
        },
    ))
    check(failures, "absent_no_last_seen/file_wins", auth_kind(r) == "file", repr(r))

    # -----------------------------------------------------------------------
    # Same mod_time window (<=5s), different byte_size => larger wins
    # -----------------------------------------------------------------------
    r = decide(inp(
        "files/size-tie.txt",
        peers(("A", "normal"), ("B", "normal")),
        live={"A": file_entry(T0, 5), "B": file_entry(T_PLUS_5, 10)},
        rows={"A": file_row(T_OLD, 5), "B": file_row(T_OLD, 10)},
    ))
    check(failures, "size_tie/source_B", auth_source(r) == "B", repr(r))
    check(failures, "size_tie/byte_size_10", auth_byte_size(r) == 10, repr(r))
    check(failures, "size_tie/A_copy", fs(r, "A") == ["copy_file"], repr(fs(r, "A")))

    # -----------------------------------------------------------------------
    # Remaining tie (same window, same byte_size) => first peer in input order wins
    # -----------------------------------------------------------------------
    r = decide(inp(
        "files/order-tie.txt",
        peers(("B", "normal"), ("A", "normal")),
        live={"A": file_entry(T_PLUS_4, 10), "B": file_entry(T0, 10)},
        rows={"A": file_row(T_OLD, 10), "B": file_row(T_OLD, 10)},
    ))
    check(failures, "order_tie/source_B", auth_source(r) == "B", repr(r))
    check(failures, "order_tie/A_keep", fs(r, "A") == ["keep"], repr(fs(r, "A")))

    # -----------------------------------------------------------------------
    # Peer with matching winning file metadata gets keep, not copy_file
    # -----------------------------------------------------------------------
    r = decide(inp(
        "files/already-right.txt",
        peers(("A", "normal"), ("B", "normal")),
        live={"A": file_entry(T_PLUS_6, 77), "B": file_entry(T_PLUS_4, 77)},
        rows={"A": file_row(T_OLD, 20)},
    ))
    # A wins (modified, newer by 6s over T0 baseline). B has same byte_size, mod_time within 5s of A.
    check(failures, "already_right/B_keep", fs(r, "B") == ["keep"], repr(fs(r, "B")))

    # -----------------------------------------------------------------------
    # no_opinion (no row, no live) => does not vote deletion; peer receives copy
    # -----------------------------------------------------------------------
    r = decide(inp(
        "files/no-opinion.txt",
        peers(("A", "normal"), ("B", "normal")),
        live={"A": file_entry(T0, 10)},
    ))
    check(failures, "no_opinion/source_A", auth_source(r) == "A", repr(r))
    check(failures, "no_opinion/B_copy", fs(r, "B") == ["copy_file"], repr(fs(r, "B")))

    # -----------------------------------------------------------------------
    # No live file opinions from contributing peers => absent (not skipped)
    # -----------------------------------------------------------------------
    r = decide(inp(
        "files/no-votes.txt",
        peers(("A", "normal"), ("S", "subordinate")),
        live={"S": file_entry(T0, 3)},
    ))
    check(failures, "no_votes/not_skipped", is_skipped(r) is False, repr(r))
    check(failures, "no_votes/absent", auth_kind(r) == "absent", repr(r))
    check(failures, "no_votes/S_displace", fs(r, "S") == ["displace"], repr(fs(r, "S")))
    check(failures, "no_votes/S_mark_displaced", snap(r, "S") == ["mark_displaced"], repr(snap(r, "S")))

    # -----------------------------------------------------------------------
    # Directory decisions ignore modification time
    # -----------------------------------------------------------------------
    r = decide(inp(
        "dirs/ignore-mtime",
        peers(("A", "normal"), ("B", "normal")),
        live={"A": dir_entry(T_OLD), "B": dir_entry(T_PLUS_20)},
    ))
    check(failures, "dir_mtime/auth_dir", auth_kind(r) == "directory", repr(r))
    check(failures, "dir_mtime/A_keep", fs(r, "A") == ["keep"], repr(fs(r, "A")))
    check(failures, "dir_mtime/B_keep", fs(r, "B") == ["keep"], repr(fs(r, "B")))
    check(failures, "dir_mtime/recurse", recurse(r) == {"A", "B"}, repr(recurse(r)))

    # -----------------------------------------------------------------------
    # Directory tombstone deletes when no contributing peer has it live;
    # peer with no row does not block deletion
    # -----------------------------------------------------------------------
    r = decide(inp(
        "dirs/tombstone-gone",
        peers(("A", "normal"), ("B", "normal"), ("C", "subordinate")),
        live={"C": dir_entry(T0)},
        rows={"A": dir_row(T_OLD, last_seen=T_SEEN, deleted_time=T_PLUS_20)},
    ))
    check(failures, "dir_tombstone/absent", auth_kind(r) == "absent", repr(r))
    check(failures, "dir_tombstone/B_no_effects", fs(r, "B") in ([], ["keep"]) and snap(r, "B") in ([], ["no_snapshot_change"]), repr(r))
    check(failures, "dir_tombstone/C_displace", fs(r, "C") == ["displace"], repr(fs(r, "C")))

    # -----------------------------------------------------------------------
    # Type conflict without canon: file wins over directory; directory displaced
    # -----------------------------------------------------------------------
    r = decide(inp(
        "conflicts/no-canon",
        peers(("A", "normal"), ("B", "normal")),
        live={"A": file_entry(T0, 100), "B": dir_entry(T_PLUS_20)},
    ))
    check(failures, "conflict_no_canon/file_wins", auth_kind(r) == "file", repr(r))
    check(failures, "conflict_no_canon/A_keep", fs(r, "A") == ["keep"], repr(fs(r, "A")))
    check(failures, "conflict_no_canon/B_displace", "displace" in fs(r, "B"), repr(fs(r, "B")))

    # -----------------------------------------------------------------------
    # Type conflict with canon: canon state wins regardless of peer types
    # -----------------------------------------------------------------------
    r = decide(inp(
        "conflicts/canon-wins",
        peers(("canon", "canon"), ("B", "normal"), ("C", "normal")),
        live={"canon": dir_entry(T0), "B": file_entry(T_PLUS_20, 99)},
    ))
    check(failures, "conflict_canon/auth_dir", auth_kind(r) == "directory", repr(r))
    check(failures, "conflict_canon/B_displace_first", fs(r, "B")[0:1] == ["displace"], repr(fs(r, "B")))
    check(failures, "conflict_canon/C_create_dir", fs(r, "C") == ["create_directory"], repr(fs(r, "C")))

    # -----------------------------------------------------------------------
    # Subordinate with wrong type: displaced then conformed; does not influence decision
    # -----------------------------------------------------------------------
    r = decide(inp(
        "sub/conform.txt",
        peers(("A", "normal"), ("S", "subordinate")),
        live={"A": file_entry(T0, 20), "S": dir_entry(T0)},
    ))
    check(failures, "sub_conform/source_A", auth_source(r) == "A", repr(r))
    check(failures, "sub_conform/S_displace_copy", fs(r, "S") == ["displace", "copy_file"], repr(fs(r, "S")))
    check(failures, "sub_conform/S_snap", "mark_displaced" in snap(r, "S") and "copy_pending" in snap(r, "S"), repr(snap(r, "S")))

    # -----------------------------------------------------------------------
    # mark_absent: peer absent with untombstoned row when authoritative is absent
    # -----------------------------------------------------------------------
    r = decide(inp(
        "files/both-absent.txt",
        peers(("A", "normal"), ("B", "normal")),
        rows={
            "A": file_row(T_OLD, 5, last_seen=T_OLD, deleted_time=T_PLUS_20),
            "B": file_row(T_OLD, 5, last_seen=T_SEEN),
        },
    ))
    check(failures, "mark_absent/auth_absent", auth_kind(r) == "absent", repr(r))
    # B is absent live with untombstoned row => mark_absent
    check(failures, "mark_absent/B_mark_absent", snap(r, "B") == ["mark_absent"], repr(snap(r, "B")))

    # -----------------------------------------------------------------------
    # Invalid: more than one canon peer
    # -----------------------------------------------------------------------
    resp = call_decide(sock, inp("bad/canon.txt", peers(("A", "canon"), ("B", "canon"))), nid())
    check(failures, "invalid/two_canons", has_invalid_input(resp), repr(resp))
    check(failures, "invalid/two_canons_no_partial", "authoritative_state" not in json.dumps(resp), repr(resp))

    # -----------------------------------------------------------------------
    # Invalid: duplicate peer identifiers
    # -----------------------------------------------------------------------
    rid = nid()
    resp = rpc_raw(sock,
        '{"jsonrpc":"2.0","id":' + str(rid) + ',"method":"tools/call","params":{"name":"decide-entry",'
        '"arguments":{"relative_path":"bad/dup.txt","peers":{"A":"normal","A":"normal"},'
        '"live_entries":{},"snapshot_rows":{}}}}')
    check(failures, "invalid/dup_peers", has_invalid_input(resp), repr(resp))

    # -----------------------------------------------------------------------
    # Invalid: live_entries key not present in peers
    # -----------------------------------------------------------------------
    resp = call_decide(sock, inp("bad/live-key.txt", peers(("A", "normal")), live={"Z": file_entry(T0, 1)}), nid())
    check(failures, "invalid/live_unknown_peer", has_invalid_input(resp), repr(resp))

    # -----------------------------------------------------------------------
    # Invalid: snapshot_rows key not present in peers
    # -----------------------------------------------------------------------
    resp = call_decide(sock, inp("bad/snap-key.txt", peers(("A", "normal")), rows={"Z": file_row(T0, 1)}), nid())
    check(failures, "invalid/snap_unknown_peer", has_invalid_input(resp), repr(resp))

    # -----------------------------------------------------------------------
    # Invalid: file entry with negative byte_size
    # -----------------------------------------------------------------------
    resp = call_decide(sock, inp("bad/neg-size.txt", peers(("A", "normal")), live={"A": file_entry(T0, -5)}), nid())
    check(failures, "invalid/neg_file_size", has_invalid_input(resp), repr(resp))

    # -----------------------------------------------------------------------
    # Invalid: directory entry with byte_size != -1
    # -----------------------------------------------------------------------
    resp = call_decide(sock, inp("bad/dir-size.txt", peers(("A", "normal")), live={"A": {"kind": "directory", "mod_time": T0, "byte_size": 0}}), nid())
    check(failures, "invalid/dir_nonzero_size", has_invalid_input(resp), repr(resp))

    # -----------------------------------------------------------------------
    # Invalid: snapshot row with deleted_time present but no last_seen
    # -----------------------------------------------------------------------
    resp = call_decide(sock, inp("bad/no-last-seen.txt", peers(("A", "normal")), rows={"A": file_row(T0, 1, last_seen=None, deleted_time=T_PLUS_6)}), nid())
    check(failures, "invalid/deleted_no_last_seen", has_invalid_input(resp), repr(resp))

    # -----------------------------------------------------------------------
    # Invalid inputs must not produce stdout or stderr from the engine
    # (not reasonably testable through tools/call -- the MCP wrapper owns those streams)
    # -----------------------------------------------------------------------

    return failures


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main() -> int:
    proc: subprocess.Popen[str] | None = None
    port: int | None = None
    failures: list[str] = []

    try:
        proc, port, stdout_buf, stderr_buf = launch_mcp()
        with socket.create_connection(("127.0.0.1", port), timeout=10) as sock:
            failures = run_tests(sock, stdout_buf, stderr_buf)
    except Exception as exc:
        failures.append(f"harness error: {type(exc).__name__}: {exc}")
        print(f"FAIL harness: {exc}")
    finally:
        if proc is not None and port is not None:
            shutdown_mcp(proc, port)

    if failures:
        print(f"\n{len(failures)} check(s) failed:")
        for f in failures:
            print(f"  - {f}")
        return 1
    print("All checks passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
