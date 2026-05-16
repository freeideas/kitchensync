#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercise the sync decision engine through its MCP wrapper."""

from __future__ import annotations

import json
import os
import socket
import subprocess
import sys
import threading
import time
from pathlib import Path
from typing import Any


BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

REQUIRED_TOOLS = {"decide-entry"}


class RpcClient:
    def __init__(self, sock: socket.socket) -> None:
        self.sock = sock
        self.next_id = 1
        self.buffer = b""

    def request(self, method: str, params: dict[str, Any] | None = None) -> dict[str, Any]:
        rpc_id = self.next_id
        self.next_id += 1
        message: dict[str, Any] = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
        if params is not None:
            message["params"] = params
        return self.raw_request(json.dumps(message, separators=(",", ":")))

    def raw_request(self, payload: str) -> dict[str, Any]:
        self.sock.sendall((payload + "\n").encode("utf-8"))
        deadline = time.time() + 20
        while b"\n" not in self.buffer:
            remaining = deadline - time.time()
            if remaining <= 0:
                raise TimeoutError(f"timed out waiting for response to {payload}")
            self.sock.settimeout(remaining)
            chunk = self.sock.recv(65536)
            if not chunk:
                raise ConnectionError(f"connection closed waiting for response to {payload}")
            self.buffer += chunk
        line, _, self.buffer = self.buffer.partition(b"\n")
        return json.loads(line.decode("utf-8"))

    def call_tool(self, name: str, arguments: dict[str, Any]) -> dict[str, Any]:
        return self.request("tools/call", {"name": name, "arguments": arguments})


def drain(stream: Any, sink: list[str]) -> None:
    for line in stream:
        sink.append(line)


def launch_mcp() -> tuple[subprocess.Popen[str], int, list[str], list[str]]:
    stdout_lines: list[str] = []
    stderr_lines: list[str] = []
    proc = subprocess.Popen(
        [str(UV), "run", "--script", str(BUILD_PY), "launch-mcp", PROJECT],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
    )
    if proc.stdout is None or proc.stderr is None:
        raise RuntimeError("MCP subprocess pipes were not created")

    port = None
    deadline = time.time() + 30
    while time.time() < deadline and proc.poll() is None:
        line = proc.stdout.readline()
        if not line:
            continue
        stdout_lines.append(line)
        stripped = line.strip()
        if stripped.startswith("MCP_PORT="):
            port = int(stripped.split("=", 1)[1])
            break

    if port is None:
        proc.terminate()
        raise RuntimeError("MCP server did not advertise MCP_PORT")

    threading.Thread(target=drain, args=(proc.stdout, stdout_lines), daemon=True).start()
    threading.Thread(target=drain, args=(proc.stderr, stderr_lines), daemon=True).start()
    return proc, port, stdout_lines, stderr_lines


def shutdown_mcp(proc: subprocess.Popen[str], port: int) -> None:
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=3) as sock:
            RpcClient(sock).request("aitc/shutdown")
    except Exception:
        pass

    try:
        proc.wait(timeout=5)
        return
    except subprocess.TimeoutExpired:
        proc.terminate()

    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait(timeout=5)


def expect_success(failures: list[str], label: str, response: dict[str, Any]) -> dict[str, Any]:
    if "error" in response:
        failures.append(f"{label}: expected success, got {response['error']}")
        return {}
    result = response.get("result")
    if not isinstance(result, dict):
        failures.append(f"{label}: expected object result, got {response}")
        return {}
    return result


def error_text(response: dict[str, Any]) -> str:
    error = response.get("error")
    if not isinstance(error, dict):
        return ""
    parts: list[str] = []
    if isinstance(error.get("message"), str):
        parts.append(error["message"])
    data = error.get("data")
    if isinstance(data, dict):
        for key, value in data.items():
            if isinstance(value, str):
                parts.append(f"{key}={value}")
    return " ".join(parts)


def expect_invalid_input(failures: list[str], label: str, response: dict[str, Any]) -> None:
    error = response.get("error")
    if not isinstance(error, dict):
        failures.append(f"{label}: expected invalid_input error, got {response}")
        return
    if "invalid_input" not in error_text(response):
        failures.append(f"{label}: expected invalid_input in error message or data, got {error}")
    if "result" in response:
        failures.append(f"{label}: invalid input must not return a partial result, got {response}")


def require_tools(failures: list[str], tools_result: dict[str, Any]) -> dict[str, dict[str, Any]]:
    tools = tools_result.get("tools")
    if not isinstance(tools, list):
        failures.append("01: tools/list result must contain a tools array")
        return {}

    by_name: dict[str, dict[str, Any]] = {}
    names: list[str] = []
    for tool in tools:
        if not isinstance(tool, dict):
            failures.append(f"01: tool entry must be an object, got {tool!r}")
            continue
        name = tool.get("name")
        if not isinstance(name, str):
            failures.append(f"01: tool name must be a string, got {tool!r}")
            continue
        names.append(name)
        by_name[name] = tool

    missing = sorted(REQUIRED_TOOLS - set(names))
    if missing:
        failures.append(f"01: missing required public API tools: {', '.join(missing)}")
    return by_name


def tool_uses_wrapped_input(tool: dict[str, Any]) -> bool:
    schema = tool.get("inputSchema")
    if not isinstance(schema, dict):
        return False
    props = schema.get("properties")
    if not isinstance(props, dict):
        return False
    return "input" in props and "relative_path" not in props


def call_decide(rpc: RpcClient, decide_tool: dict[str, Any], decision_input: dict[str, Any]) -> dict[str, Any]:
    arguments = {"input": decision_input} if tool_uses_wrapped_input(decide_tool) else decision_input
    return rpc.call_tool("decide-entry", arguments)


def duplicate_peer_raw_payload(rpc_id: int, decide_tool: dict[str, Any]) -> str:
    decision = (
        '{"relative_path":"dup.txt",'
        '"peers":{"dup":"normal","dup":"normal"},'
        '"live_entries":{},'
        '"snapshot_rows":{}}'
    )
    arguments = f'{{"input":{decision}}}' if tool_uses_wrapped_input(decide_tool) else decision
    return (
        f'{{"jsonrpc":"2.0","id":{rpc_id},"method":"tools/call",'
        f'"params":{{"name":"decide-entry","arguments":{arguments}}}}}'
    )


def file_entry(mod_time: str, byte_size: int) -> dict[str, Any]:
    return {"kind": "file", "mod_time": mod_time, "byte_size": byte_size}


def dir_entry(mod_time: str = "2026-05-15T10:00:00Z") -> dict[str, Any]:
    return {"kind": "directory", "mod_time": mod_time, "byte_size": -1}


def file_row(mod_time: str, byte_size: int, last_seen: str, deleted_time: str | None = None) -> dict[str, Any]:
    row = {"kind": "file", "mod_time": mod_time, "byte_size": byte_size, "last_seen": last_seen}
    if deleted_time is not None:
        row["deleted_time"] = deleted_time
    return row


def dir_row(last_seen: str, deleted_time: str | None = None) -> dict[str, Any]:
    row = {"kind": "directory", "mod_time": "2026-05-15T10:00:00Z", "byte_size": -1, "last_seen": last_seen}
    if deleted_time is not None:
        row["deleted_time"] = deleted_time
    return row


def auth(decision: dict[str, Any]) -> dict[str, Any]:
    value = decision.get("authoritative_state")
    return value if isinstance(value, dict) else {}


def peer_values(container: Any, peer: str) -> list[Any]:
    if isinstance(container, dict):
        value = container.get(peer, [])
        return value if isinstance(value, list) else [value]
    if isinstance(container, list):
        for item in container:
            if not isinstance(item, dict):
                continue
            item_peer = item.get("peer") or item.get("peer_id") or item.get("peerId")
            if item_peer != peer:
                continue
            for key in ("effects", "filesystem_effects", "snapshot_effects"):
                value = item.get(key)
                if isinstance(value, list):
                    return value
            for key in ("effect", "kind", "type"):
                value = item.get(key)
                if value is not None:
                    return [value]
    return []


def value_name(value: Any) -> str:
    if isinstance(value, str):
        return value
    if isinstance(value, dict):
        for key in ("effect", "kind", "type", "name"):
            item = value.get(key)
            if isinstance(item, str):
                return item
    return repr(value)


def effects(decision: dict[str, Any], key: str, peer: str) -> list[str]:
    return [value_name(item) for item in peer_values(decision.get(key), peer)]


def effect_sources(decision: dict[str, Any], key: str, peer: str) -> list[Any]:
    sources: list[Any] = []
    for item in peer_values(decision.get(key), peer):
        sources.append(item.get("source_peer") if isinstance(item, dict) else None)
    return sources


def assert_equal(failures: list[str], label: str, actual: Any, expected: Any) -> None:
    if actual != expected:
        failures.append(f"{label}: expected {expected!r}, got {actual!r}")


def assert_contains_exactly(failures: list[str], label: str, actual: Any, expected: set[str]) -> None:
    if not isinstance(actual, list) or set(actual) != expected:
        failures.append(f"{label}: expected members {sorted(expected)!r}, got {actual!r}")


def assert_no_effects(failures: list[str], label: str, actual: Any) -> None:
    if actual not in ({}, []):
        failures.append(f"{label}: expected no effects, got {actual!r}")


def main() -> int:
    failures: list[str] = []
    proc: subprocess.Popen[str] | None = None
    port = 0

    try:
        proc, port, stdout_lines, stderr_lines = launch_mcp()
        with socket.create_connection(("127.0.0.1", port), timeout=10) as sock:
            rpc = RpcClient(sock)

            tools_response = rpc.request("tools/list")
            tools_result = expect_success(failures, "01 tools/list", tools_response)
            tools = require_tools(failures, tools_result)
            print(f"[01] tools/list exposes {len(tools)} tool(s)")
            time.sleep(0.1)
            stdout_lines.clear()
            stderr_lines.clear()

            decide_tool = tools.get("decide-entry")
            if decide_tool is None:
                print("[02-17] skipped behavior calls because decide-entry is missing")
            else:
                skipped = expect_success(
                    failures,
                    "02 no contributors",
                    call_decide(
                        rpc,
                        decide_tool,
                        {
                            "relative_path": "sub-only.txt",
                            "peers": {"sub": "subordinate"},
                            "live_entries": {"sub": file_entry("2026-05-15T10:00:00Z", 3)},
                            "snapshot_rows": {},
                        },
                    ),
                )
                assert_equal(failures, "02 skipped flag", skipped.get("skipped"), True)
                assert_no_effects(failures, "02 filesystem effects", skipped.get("filesystem_effects"))
                assert_no_effects(failures, "02 snapshot effects", skipped.get("snapshot_effects"))
                print("[02] no contributing peers are skipped without effects")

                canon_file = expect_success(
                    failures,
                    "03 canon file",
                    call_decide(
                        rpc,
                        decide_tool,
                        {
                            "relative_path": "canon.txt",
                            "peers": {"canon": "canon", "peer": "normal", "dirpeer": "normal", "sub": "subordinate"},
                            "live_entries": {
                                "canon": file_entry("2026-05-15T10:00:00Z", 7),
                                "peer": file_entry("2026-05-15T09:00:00Z", 4),
                                "dirpeer": dir_entry("2026-05-15T09:00:00Z"),
                                "sub": file_entry("2026-05-15T10:00:00Z", 7),
                            },
                            "snapshot_rows": {
                                "peer": file_row("2026-05-15T09:00:00Z", 4, "2026-05-15T09:30:00Z"),
                                "dirpeer": dir_row("2026-05-15T09:30:00Z"),
                                "sub": file_row("2026-05-15T10:00:00Z", 7, "2026-05-15T10:01:00Z"),
                            },
                        },
                    ),
                )
                assert_equal(failures, "03 authoritative kind", auth(canon_file).get("kind"), "file")
                assert_equal(failures, "03 source peer", auth(canon_file).get("source_peer"), "canon")
                assert_equal(failures, "03 winning mod time", auth(canon_file).get("mod_time"), "2026-05-15T10:00:00Z")
                assert_equal(failures, "03 winning byte size", auth(canon_file).get("byte_size"), 7)
                assert_equal(failures, "03 canon keeps file", effects(canon_file, "filesystem_effects", "canon"), ["keep"])
                assert_equal(failures, "03 stale peer copies file", effects(canon_file, "filesystem_effects", "peer"), ["copy_file"])
                assert_equal(
                    failures,
                    "03 stale peer copy source",
                    effect_sources(canon_file, "filesystem_effects", "peer"),
                    ["canon"],
                )
                assert_equal(
                    failures,
                    "03 wrong-type peer displaced then copied",
                    effects(canon_file, "filesystem_effects", "dirpeer"),
                    ["displace", "copy_file"],
                )
                assert_equal(
                    failures,
                    "03 wrong-type peer copy source",
                    effect_sources(canon_file, "filesystem_effects", "dirpeer"),
                    [None, "canon"],
                )
                assert_equal(failures, "03 matching subordinate keeps file", effects(canon_file, "filesystem_effects", "sub"), ["keep"])
                assert_equal(failures, "03 canon snapshot confirmed", effects(canon_file, "snapshot_effects", "canon"), ["confirm_present"])
                assert_equal(failures, "03 copy snapshot", effects(canon_file, "snapshot_effects", "peer"), ["copy_pending"])
                assert_equal(
                    failures,
                    "03 wrong-type peer snapshot",
                    effects(canon_file, "snapshot_effects", "dirpeer"),
                    ["mark_displaced", "copy_pending"],
                )
                assert_equal(failures, "03 matching subordinate snapshot", effects(canon_file, "snapshot_effects", "sub"), ["confirm_present"])
                print("[03] canon file wins and peers are conformed")

                canon_directory = expect_success(
                    failures,
                    "04 canon directory",
                    call_decide(
                        rpc,
                        decide_tool,
                        {
                            "relative_path": "canon-dir",
                            "peers": {
                                "canon": "canon",
                                "missing": "normal",
                                "wrong": "normal",
                                "subwrong": "subordinate",
                            },
                            "live_entries": {
                                "canon": dir_entry("2026-05-15T12:00:00Z"),
                                "wrong": file_entry("2026-05-15T10:00:00Z", 2),
                                "subwrong": file_entry("2026-05-15T10:00:00Z", 2),
                            },
                            "snapshot_rows": {
                                "wrong": file_row("2026-05-15T10:00:00Z", 2, "2026-05-15T10:01:00Z"),
                                "subwrong": file_row("2026-05-15T10:00:00Z", 2, "2026-05-15T10:01:00Z"),
                            },
                        },
                    ),
                )
                assert_equal(failures, "04 authoritative directory", auth(canon_directory).get("kind"), "directory")
                assert_equal(failures, "04 canon directory keep", effects(canon_directory, "filesystem_effects", "canon"), ["keep"])
                assert_equal(
                    failures,
                    "04 missing directory created",
                    effects(canon_directory, "filesystem_effects", "missing"),
                    ["create_directory"],
                )
                assert_equal(
                    failures,
                    "04 wrong-type normal conformed",
                    effects(canon_directory, "filesystem_effects", "wrong"),
                    ["displace", "create_directory"],
                )
                assert_equal(
                    failures,
                    "04 wrong-type subordinate conformed",
                    effects(canon_directory, "filesystem_effects", "subwrong"),
                    ["displace", "create_directory"],
                )
                assert_equal(
                    failures,
                    "04 missing directory snapshot",
                    effects(canon_directory, "snapshot_effects", "missing"),
                    ["create_directory_confirmed"],
                )
                assert_contains_exactly(
                    failures,
                    "04 canon directory recurse peers",
                    canon_directory.get("recurse_peers"),
                    {"canon", "missing", "wrong", "subwrong"},
                )
                print("[04] canon directory wins and all peers are conformed for recursion")

                canon_absent = expect_success(
                    failures,
                    "05 canon absent",
                    call_decide(
                        rpc,
                        decide_tool,
                        {
                            "relative_path": "gone",
                            "peers": {
                                "canon": "canon",
                                "peer": "normal",
                                "sub": "subordinate",
                                "stale": "normal",
                                "already_deleted": "normal",
                            },
                            "live_entries": {"peer": dir_entry(), "sub": file_entry("2026-05-15T10:00:00Z", 5)},
                            "snapshot_rows": {
                                "peer": dir_row("2026-05-15T09:00:00Z"),
                                "sub": file_row("2026-05-15T09:00:00Z", 5, "2026-05-15T09:30:00Z"),
                                "stale": file_row("2026-05-15T09:00:00Z", 5, "2026-05-15T09:30:00Z"),
                                "already_deleted": file_row(
                                    "2026-05-15T09:00:00Z",
                                    5,
                                    "2026-05-15T09:30:00Z",
                                    "2026-05-15T09:45:00Z",
                                ),
                            },
                        },
                    ),
                )
                assert_equal(failures, "05 authoritative absent", auth(canon_absent).get("kind"), "absent")
                assert_equal(failures, "05 peer displaced", effects(canon_absent, "filesystem_effects", "peer"), ["displace"])
                assert_equal(failures, "05 subordinate displaced", effects(canon_absent, "filesystem_effects", "sub"), ["displace"])
                assert_equal(failures, "05 already absent peer kept", effects(canon_absent, "filesystem_effects", "stale"), ["keep"])
                assert_equal(failures, "05 displaced snapshot", effects(canon_absent, "snapshot_effects", "peer"), ["mark_displaced"])
                assert_equal(failures, "05 absent untombstoned snapshot", effects(canon_absent, "snapshot_effects", "stale"), ["mark_absent"])
                assert_equal(
                    failures,
                    "05 already tombstoned snapshot",
                    effects(canon_absent, "snapshot_effects", "already_deleted"),
                    ["no_snapshot_change"],
                )
                print("[05] missing canon path makes the path absent")

                unchanged_file = expect_success(
                    failures,
                    "06 unchanged file",
                    call_decide(
                        rpc,
                        decide_tool,
                        {
                            "relative_path": "stable.txt",
                            "peers": {"first": "normal", "second": "normal", "target": "normal"},
                            "live_entries": {
                                "first": file_entry("2026-05-15T10:00:00Z", 12),
                                "second": file_entry("2026-05-15T10:00:04Z", 12),
                            },
                            "snapshot_rows": {
                                "first": file_row("2026-05-15T10:00:00Z", 12, "2026-05-15T10:01:00Z"),
                                "second": file_row("2026-05-15T10:00:00Z", 12, "2026-05-15T10:01:00Z"),
                            },
                        },
                    ),
                )
                assert_equal(failures, "06 unchanged kind", auth(unchanged_file).get("kind"), "file")
                assert_equal(failures, "06 first matching source wins", auth(unchanged_file).get("source_peer"), "first")
                assert_equal(failures, "06 first file keep", effects(unchanged_file, "filesystem_effects", "first"), ["keep"])
                assert_equal(failures, "06 second file keep", effects(unchanged_file, "filesystem_effects", "second"), ["keep"])
                assert_equal(failures, "06 missing peer copy", effects(unchanged_file, "filesystem_effects", "target"), ["copy_file"])
                assert_equal(failures, "06 first snapshot confirmed", effects(unchanged_file, "snapshot_effects", "first"), ["confirm_present"])
                assert_equal(failures, "06 missing peer snapshot pending", effects(unchanged_file, "snapshot_effects", "target"), ["copy_pending"])
                print("[06] unchanged matching files are kept and copied only to missing peers")

                modified_file = expect_success(
                    failures,
                    "07 modified file",
                    call_decide(
                        rpc,
                        decide_tool,
                        {
                            "relative_path": "modified.txt",
                            "peers": {"old": "normal", "new": "normal", "target": "normal", "sub": "subordinate"},
                            "live_entries": {
                                "old": file_entry("2026-05-15T10:00:00Z", 12),
                                "new": file_entry("2026-05-15T10:00:10Z", 12),
                                "sub": file_entry("2026-05-15T11:00:00Z", 99),
                            },
                            "snapshot_rows": {
                                "old": file_row("2026-05-15T10:00:00Z", 12, "2026-05-15T10:01:00Z"),
                                "new": file_row("2026-05-15T10:00:00Z", 12, "2026-05-15T10:01:00Z"),
                                "sub": file_row("2026-05-15T11:00:00Z", 99, "2026-05-15T11:01:00Z"),
                            },
                        },
                    ),
                )
                assert_equal(failures, "07 modified kind", auth(modified_file).get("kind"), "file")
                assert_equal(failures, "07 latest modified source", auth(modified_file).get("source_peer"), "new")
                assert_equal(failures, "07 old peer copies latest file", effects(modified_file, "filesystem_effects", "old"), ["copy_file"])
                assert_equal(failures, "07 modified peer keeps file", effects(modified_file, "filesystem_effects", "new"), ["keep"])
                assert_equal(failures, "07 target copies latest file", effects(modified_file, "filesystem_effects", "target"), ["copy_file"])
                assert_equal(
                    failures,
                    "07 subordinate newer file does not influence and is copied over",
                    effects(modified_file, "filesystem_effects", "sub"),
                    ["copy_file"],
                )
                assert_equal(
                    failures,
                    "07 subordinate copy snapshot pending",
                    effects(modified_file, "snapshot_effects", "sub"),
                    ["copy_pending"],
                )
                print("[07] modified files beat unchanged files and subordinate files only receive conformance")

                file_tie = expect_success(
                    failures,
                    "08 file tie",
                    call_decide(
                        rpc,
                        decide_tool,
                        {
                            "relative_path": "tie.txt",
                            "peers": {"a": "normal", "b": "normal", "target": "normal"},
                            "live_entries": {
                                "a": file_entry("2026-05-15T10:00:00Z", 100),
                                "b": file_entry("2026-05-15T10:00:04Z", 200),
                            },
                            "snapshot_rows": {},
                        },
                    ),
                )
                assert_equal(failures, "08 tied file kind", auth(file_tie).get("kind"), "file")
                assert_equal(failures, "08 larger tied file wins", auth(file_tie).get("source_peer"), "b")
                assert_equal(failures, "08 target copy", effects(file_tie, "filesystem_effects", "target"), ["copy_file"])
                assert_equal(failures, "08 target copy source", effect_sources(file_tie, "filesystem_effects", "target"), ["b"])
                assert_equal(failures, "08 nonwinning file copy", effects(file_tie, "filesystem_effects", "a"), ["copy_file"])
                print("[08] new file ties use the larger byte_size before peer order")

                tombstoned_live_file = expect_success(
                    failures,
                    "09 tombstoned live file",
                    call_decide(
                        rpc,
                        decide_tool,
                        {
                            "relative_path": "restored.txt",
                            "peers": {"restored": "normal", "stable": "normal"},
                            "live_entries": {
                                "restored": file_entry("2026-05-15T10:00:00Z", 5),
                                "stable": file_entry("2026-05-15T10:01:00Z", 9),
                            },
                            "snapshot_rows": {
                                "restored": file_row(
                                    "2026-05-15T10:00:00Z",
                                    5,
                                    "2026-05-15T09:59:00Z",
                                    "2026-05-15T09:59:30Z",
                                ),
                                "stable": file_row("2026-05-15T10:01:00Z", 9, "2026-05-15T10:01:30Z"),
                            },
                        },
                    ),
                )
                assert_equal(failures, "09 tombstoned live file is authoritative", auth(tombstoned_live_file).get("kind"), "file")
                assert_equal(
                    failures,
                    "09 tombstoned live file is classified modified",
                    auth(tombstoned_live_file).get("source_peer"),
                    "restored",
                )
                assert_equal(
                    failures,
                    "09 newer unchanged file copies restored data",
                    effects(tombstoned_live_file, "filesystem_effects", "stable"),
                    ["copy_file"],
                )
                print("[09] live files with tombstoned snapshot rows are modified, not unchanged")

                new_file_beats_unchanged = expect_success(
                    failures,
                    "09b new file beats unchanged",
                    call_decide(
                        rpc,
                        decide_tool,
                        {
                            "relative_path": "new-beats-unchanged.txt",
                            "peers": {"new": "normal", "unchanged": "normal"},
                            "live_entries": {
                                "new": file_entry("2026-05-15T10:00:00Z", 5),
                                "unchanged": file_entry("2026-05-15T10:01:00Z", 9),
                            },
                            "snapshot_rows": {
                                "unchanged": file_row("2026-05-15T10:01:00Z", 9, "2026-05-15T10:01:30Z")
                            },
                        },
                    ),
                )
                assert_equal(failures, "09b new file is authoritative", auth(new_file_beats_unchanged).get("kind"), "file")
                assert_equal(
                    failures,
                    "09b new file participates like modified",
                    auth(new_file_beats_unchanged).get("source_peer"),
                    "new",
                )
                assert_equal(
                    failures,
                    "09b later unchanged file copies new data",
                    effects(new_file_beats_unchanged, "filesystem_effects", "unchanged"),
                    ["copy_file"],
                )
                print("[09b] new files participate like modified files")

                new_and_modified_newer_new = expect_success(
                    failures,
                    "09c newer new file beats modified",
                    call_decide(
                        rpc,
                        decide_tool,
                        {
                            "relative_path": "newer-new.txt",
                            "peers": {"modified": "normal", "new": "normal"},
                            "live_entries": {
                                "modified": file_entry("2026-05-15T10:00:00Z", 5),
                                "new": file_entry("2026-05-15T10:00:10Z", 7),
                            },
                            "snapshot_rows": {
                                "modified": file_row("2026-05-15T09:59:00Z", 5, "2026-05-15T10:00:01Z")
                            },
                        },
                    ),
                )
                assert_equal(
                    failures,
                    "09c newer new file wins within modified/new class",
                    auth(new_and_modified_newer_new).get("source_peer"),
                    "new",
                )
                assert_equal(
                    failures,
                    "09c modified peer copies newer new file",
                    effects(new_and_modified_newer_new, "filesystem_effects", "modified"),
                    ["copy_file"],
                )

                new_and_modified_newer_modified = expect_success(
                    failures,
                    "09d newer modified file beats new",
                    call_decide(
                        rpc,
                        decide_tool,
                        {
                            "relative_path": "newer-modified.txt",
                            "peers": {"modified": "normal", "new": "normal"},
                            "live_entries": {
                                "modified": file_entry("2026-05-15T10:00:10Z", 7),
                                "new": file_entry("2026-05-15T10:00:00Z", 5),
                            },
                            "snapshot_rows": {
                                "modified": file_row("2026-05-15T09:59:00Z", 7, "2026-05-15T10:00:01Z")
                            },
                        },
                    ),
                )
                assert_equal(
                    failures,
                    "09d newer modified file wins within modified/new class",
                    auth(new_and_modified_newer_modified).get("source_peer"),
                    "modified",
                )
                assert_equal(
                    failures,
                    "09d new peer copies newer modified file",
                    effects(new_and_modified_newer_modified, "filesystem_effects", "new"),
                    ["copy_file"],
                )
                print("[09c-09d] new and modified files share one latest-mod_time class")

                deletion_wins = expect_success(
                    failures,
                    "10 deletion wins",
                    call_decide(
                        rpc,
                        decide_tool,
                        {
                            "relative_path": "delete-me.txt",
                            "peers": {"file": "normal", "deleter": "normal"},
                            "live_entries": {"file": file_entry("2026-05-15T10:00:00Z", 10)},
                            "snapshot_rows": {
                                "file": file_row("2026-05-15T10:00:00Z", 10, "2026-05-15T10:00:01Z"),
                                "deleter": file_row(
                                    "2026-05-15T09:00:00Z",
                                    10,
                                    "2026-05-15T10:00:01Z",
                                    "2026-05-15T10:00:06Z",
                                ),
                            },
                        },
                    ),
                )
                assert_equal(failures, "10 deletion authoritative", auth(deletion_wins).get("kind"), "absent")
                assert_equal(failures, "10 absent deleting peer kept", effects(deletion_wins, "filesystem_effects", "deleter"), ["keep"])
                assert_equal(failures, "10 live file displaced", effects(deletion_wins, "filesystem_effects", "file"), ["displace"])

                deletion_tied = expect_success(
                    failures,
                    "10 deletion tied",
                    call_decide(
                        rpc,
                        decide_tool,
                        {
                            "relative_path": "keep-me.txt",
                            "peers": {"file": "normal", "deleter": "normal"},
                            "live_entries": {"file": file_entry("2026-05-15T10:00:00Z", 10)},
                            "snapshot_rows": {
                                "file": file_row("2026-05-15T10:00:00Z", 10, "2026-05-15T10:00:01Z"),
                                "deleter": file_row(
                                    "2026-05-15T09:00:00Z",
                                    10,
                                    "2026-05-15T10:00:01Z",
                                    "2026-05-15T10:00:05Z",
                                ),
                            },
                        },
                    ),
                )
                assert_equal(failures, "10 tied deletion keeps data", auth(deletion_tied).get("kind"), "file")
                assert_equal(failures, "10 tied deletion source", auth(deletion_tied).get("source_peer"), "file")
                print("[10] deletion estimates beat files only beyond the five-second tolerance")

                absent_unconfirmed_wins = expect_success(
                    failures,
                    "11 absent_unconfirmed wins",
                    call_decide(
                        rpc,
                        decide_tool,
                        {
                            "relative_path": "maybe-deleted.txt",
                            "peers": {"file": "normal", "absent": "normal"},
                            "live_entries": {"file": file_entry("2026-05-15T10:00:00Z", 10)},
                            "snapshot_rows": {
                                "file": file_row("2026-05-15T10:00:00Z", 10, "2026-05-15T10:00:01Z"),
                                "absent": file_row("2026-05-15T09:00:00Z", 10, "2026-05-15T10:00:06Z"),
                            },
                        },
                    ),
                )
                assert_equal(
                    failures,
                    "11 absent_unconfirmed later than tolerance deletes",
                    auth(absent_unconfirmed_wins).get("kind"),
                    "absent",
                )
                assert_equal(
                    failures,
                    "11 absent_unconfirmed displaces live file",
                    effects(absent_unconfirmed_wins, "filesystem_effects", "file"),
                    ["displace"],
                )

                absent_unconfirmed_ignored = expect_success(
                    failures,
                    "11 absent_unconfirmed ignored",
                    call_decide(
                        rpc,
                        decide_tool,
                        {
                            "relative_path": "copy-failed.txt",
                            "peers": {"file": "normal", "absent": "normal"},
                            "live_entries": {"file": file_entry("2026-05-15T10:00:00Z", 10)},
                            "snapshot_rows": {
                                "file": file_row("2026-05-15T10:00:00Z", 10, "2026-05-15T10:00:01Z"),
                                "absent": file_row("2026-05-15T09:00:00Z", 10, "2026-05-15T10:00:05Z"),
                            },
                        },
                    ),
                )
                assert_equal(
                    failures,
                    "11 absent_unconfirmed within tolerance keeps data",
                    auth(absent_unconfirmed_ignored).get("kind"),
                    "file",
                )
                assert_equal(
                    failures,
                    "11 absent_unconfirmed copies existing file",
                    effects(absent_unconfirmed_ignored, "filesystem_effects", "absent"),
                    ["copy_file"],
                )
                absent_unconfirmed_no_last_seen = expect_success(
                    failures,
                    "11 absent_unconfirmed without last_seen",
                    call_decide(
                        rpc,
                        decide_tool,
                        {
                            "relative_path": "unfinished-copy.txt",
                            "peers": {"file": "normal", "absent": "normal"},
                            "live_entries": {"file": file_entry("2026-05-15T10:00:00Z", 10)},
                            "snapshot_rows": {
                                "file": file_row("2026-05-15T10:00:00Z", 10, "2026-05-15T10:00:01Z"),
                                "absent": {
                                    "kind": "file",
                                    "mod_time": "2026-05-15T09:00:00Z",
                                    "byte_size": 10,
                                },
                            },
                        },
                    ),
                )
                assert_equal(
                    failures,
                    "11 absent_unconfirmed without last_seen keeps data",
                    auth(absent_unconfirmed_no_last_seen).get("kind"),
                    "file",
                )
                assert_equal(
                    failures,
                    "11 absent_unconfirmed without last_seen copies existing file",
                    effects(absent_unconfirmed_no_last_seen, "filesystem_effects", "absent"),
                    ["copy_file"],
                )
                print("[11] absent_unconfirmed rows vote for deletion only beyond the tolerance")

                directory = expect_success(
                    failures,
                    "12 directory",
                    call_decide(
                        rpc,
                        decide_tool,
                        {
                            "relative_path": "folder",
                            "peers": {
                                "a": "normal",
                                "b": "normal",
                                "deleted_later": "normal",
                                "sub": "subordinate",
                            },
                            "live_entries": {
                                "a": dir_entry("2026-05-15T08:00:00Z"),
                                "sub": file_entry("2026-05-15T10:00:00Z", 1),
                            },
                            "snapshot_rows": {
                                "deleted_later": dir_row("2026-05-15T12:00:00Z", "2026-05-15T12:30:00Z"),
                                "sub": file_row("2026-05-15T10:00:00Z", 1, "2026-05-15T10:01:00Z"),
                            },
                        },
                    ),
                )
                assert_equal(failures, "12 directory authoritative", auth(directory).get("kind"), "directory")
                assert_equal(failures, "12 existing directory keep", effects(directory, "filesystem_effects", "a"), ["keep"])
                assert_equal(failures, "12 missing directory created", effects(directory, "filesystem_effects", "b"), ["create_directory"])
                assert_equal(
                    failures,
                    "12 later directory tombstone does not beat a live directory",
                    effects(directory, "filesystem_effects", "deleted_later"),
                    ["create_directory"],
                )
                assert_equal(
                    failures,
                    "12 wrong-type subordinate conformed",
                    effects(directory, "filesystem_effects", "sub"),
                    ["displace", "create_directory"],
                )
                assert_equal(failures, "12 existing directory snapshot", effects(directory, "snapshot_effects", "a"), ["confirm_present"])
                assert_equal(
                    failures,
                    "12 created directory snapshot",
                    effects(directory, "snapshot_effects", "b"),
                    ["create_directory_confirmed"],
                )
                assert_equal(
                    failures,
                    "12 later tombstoned peer receives directory creation snapshot",
                    effects(directory, "snapshot_effects", "deleted_later"),
                    ["create_directory_confirmed"],
                )
                assert_equal(
                    failures,
                    "12 displaced subordinate snapshot",
                    effects(directory, "snapshot_effects", "sub"),
                    ["mark_displaced", "create_directory_confirmed"],
                )
                assert_contains_exactly(
                    failures,
                    "12 recurse peers",
                    directory.get("recurse_peers"),
                    {"a", "b", "deleted_later", "sub"},
                )
                print("[12] directory existence ignores times, conforms peers, and selects recurse peers")

                directory_absent = expect_success(
                    failures,
                    "13 directory absent",
                    call_decide(
                        rpc,
                        decide_tool,
                        {
                            "relative_path": "old-folder",
                            "peers": {"deleted": "normal", "no_opinion": "normal", "sub": "subordinate"},
                            "live_entries": {"sub": dir_entry("2026-05-15T11:00:00Z")},
                            "snapshot_rows": {
                                "deleted": dir_row("2026-05-15T09:00:00Z", "2026-05-15T09:30:00Z")
                            },
                        },
                    ),
                )
                assert_equal(failures, "13 tombstoned directory absent", auth(directory_absent).get("kind"), "absent")
                assert_equal(
                    failures,
                    "13 subordinate directory displaced",
                    effects(directory_absent, "filesystem_effects", "sub"),
                    ["displace"],
                )
                assert_equal(
                    failures,
                    "13 tombstoned directory unchanged",
                    effects(directory_absent, "snapshot_effects", "deleted"),
                    ["no_snapshot_change"],
                )
                assert_equal(
                    failures,
                    "13 displaced directory snapshot",
                    effects(directory_absent, "snapshot_effects", "sub"),
                    ["mark_displaced"],
                )
                # Descendant cascade for displaced directories is a caller-side
                # snapshot-store update and is not observable through this
                # one-entry decision API.
                print("[13] tombstoned directories are absent and no-opinion peers do not block deletion")

                no_votes = expect_success(
                    failures,
                    "14 no votes",
                    call_decide(
                        rpc,
                        decide_tool,
                        {
                            "relative_path": "subordinate-only-live",
                            "peers": {"empty": "normal", "sub": "subordinate"},
                            "live_entries": {"sub": dir_entry("2026-05-15T10:00:00Z")},
                            "snapshot_rows": {},
                        },
                    ),
                )
                assert_equal(failures, "14 no votes authoritative absent", auth(no_votes).get("kind"), "absent")
                assert_equal(
                    failures,
                    "14 subordinate live entry displaced",
                    effects(no_votes, "filesystem_effects", "sub"),
                    ["displace"],
                )
                print("[14] active peers with no live entry or snapshot row cast no vote")

                type_conflict = expect_success(
                    failures,
                    "15 type conflict",
                    call_decide(
                        rpc,
                        decide_tool,
                        {
                            "relative_path": "conflict",
                            "peers": {"file": "normal", "dir": "normal"},
                            "live_entries": {
                                "file": file_entry("2026-05-15T10:00:00Z", 8),
                                "dir": dir_entry("2026-05-15T12:00:00Z"),
                            },
                            "snapshot_rows": {},
                        },
                    ),
                )
                assert_equal(failures, "15 file wins type conflict", auth(type_conflict).get("kind"), "file")
                assert_equal(failures, "15 type conflict source", auth(type_conflict).get("source_peer"), "file")
                assert_equal(
                    failures,
                    "15 directory displaced then copied",
                    effects(type_conflict, "filesystem_effects", "dir"),
                    ["displace", "copy_file"],
                )
                assert_equal(
                    failures,
                    "15 directory copy source",
                    effect_sources(type_conflict, "filesystem_effects", "dir"),
                    [None, "file"],
                )
                assert_equal(failures, "15 no recursion for file", type_conflict.get("recurse_peers"), [])
                print("[15] file wins non-canon type conflicts")

                opaque_path = expect_success(
                    failures,
                    "16 opaque path",
                    call_decide(
                        rpc,
                        decide_tool,
                        {
                            "relative_path": "://not-normalized\\opaque name?.txt",
                            "peers": {"a": "normal"},
                            "live_entries": {"a": file_entry("2026-05-15T10:00:00Z", 1)},
                            "snapshot_rows": {},
                        },
                    ),
                )
                assert_equal(failures, "16 opaque path still decides file", auth(opaque_path).get("kind"), "file")
                assert_equal(failures, "16 opaque path source", auth(opaque_path).get("source_peer"), "a")
                print("[16] relative_path is treated as opaque text")

                invalid_inputs = [
                    (
                        "17 more than one canon",
                        {
                            "relative_path": "bad.txt",
                            "peers": {"a": "canon", "b": "canon"},
                            "live_entries": {},
                            "snapshot_rows": {},
                        },
                    ),
                    (
                        "17 live key outside peers",
                        {
                            "relative_path": "bad.txt",
                            "peers": {"a": "normal"},
                            "live_entries": {"missing": file_entry("2026-05-15T10:00:00Z", 1)},
                            "snapshot_rows": {},
                        },
                    ),
                    (
                        "17 snapshot key outside peers",
                        {
                            "relative_path": "bad.txt",
                            "peers": {"a": "normal"},
                            "live_entries": {},
                            "snapshot_rows": {"missing": file_row("2026-05-15T10:00:00Z", 1, "2026-05-15T10:01:00Z")},
                        },
                    ),
                    (
                        "17 negative live file size",
                        {
                            "relative_path": "bad.txt",
                            "peers": {"a": "normal"},
                            "live_entries": {"a": file_entry("2026-05-15T10:00:00Z", -1)},
                            "snapshot_rows": {},
                        },
                    ),
                    (
                        "17 negative snapshot file size",
                        {
                            "relative_path": "bad.txt",
                            "peers": {"a": "normal"},
                            "live_entries": {},
                            "snapshot_rows": {
                                "a": {
                                    "kind": "file",
                                    "mod_time": "2026-05-15T10:00:00Z",
                                    "byte_size": -1,
                                    "last_seen": "2026-05-15T10:01:00Z",
                                }
                            },
                        },
                    ),
                    (
                        "17 live directory size not -1",
                        {
                            "relative_path": "bad",
                            "peers": {"a": "normal"},
                            "live_entries": {"a": {"kind": "directory", "mod_time": "2026-05-15T10:00:00Z", "byte_size": 0}},
                            "snapshot_rows": {},
                        },
                    ),
                    (
                        "17 snapshot directory size not -1",
                        {
                            "relative_path": "bad",
                            "peers": {"a": "normal"},
                            "live_entries": {},
                            "snapshot_rows": {
                                "a": {
                                    "kind": "directory",
                                    "mod_time": "2026-05-15T10:00:00Z",
                                    "byte_size": 0,
                                    "last_seen": "2026-05-15T10:01:00Z",
                                }
                            },
                        },
                    ),
                    (
                        "17 tombstone without last_seen",
                        {
                            "relative_path": "bad.txt",
                            "peers": {"a": "normal"},
                            "live_entries": {},
                            "snapshot_rows": {
                                "a": {
                                    "kind": "file",
                                    "mod_time": "2026-05-15T10:00:00Z",
                                    "byte_size": 1,
                                    "deleted_time": "2026-05-15T10:01:00Z",
                                }
                            },
                        },
                    ),
                ]
                for label, bad_input in invalid_inputs:
                    expect_invalid_input(failures, label, call_decide(rpc, decide_tool, bad_input))

                raw_dup = duplicate_peer_raw_payload(rpc.next_id, decide_tool)
                rpc.next_id += 1
                expect_invalid_input(failures, "17 duplicate peer identifiers", rpc.raw_request(raw_dup))
                print("[17] invalid inputs return invalid_input without partial results")

            time.sleep(0.1)
            if stdout_lines:
                failures.append(f"18: library operations must not write stdout, got {stdout_lines!r}")
            if stderr_lines:
                failures.append(f"18: library operations must not write stderr, got {stderr_lines!r}")
            print(f"[18] decision calls wrote {len(stdout_lines)} stdout line(s) and {len(stderr_lines)} stderr line(s)")

        if failures:
            print("\nFAILURES:")
            for failure in failures:
                print(f"  - {failure}")
            return 1
        print("\nAll assertions passed.")
        return 0
    finally:
        if proc is not None:
            shutdown_mcp(proc, port)


if __name__ == "__main__":
    sys.exit(main())
