#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = [
#   "asyncssh==2.22.0",
# ]
# ///
"""Exercise the SFTP protocol MCP wrapper against the required SFTP account."""

from __future__ import annotations

import asyncio
import base64
import json
import os
import shlex
import socket
import subprocess
import sys
import tempfile
import threading
import time
from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path
from typing import Any

import asyncssh


BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

REMOTE_USER = "ace"
REMOTE_HOST = "ordinarydata.com"
REMOTE_PORT = 22
REMOTE_BASE = "/tmp/testks"

REQUIRED_TOOLS = {
    "open-unpooled",
    "close-filesystem",
    "list-dir",
    "stat",
    "open-read",
    "read",
    "close-read",
    "open-write",
    "write",
    "close-write",
    "rename",
    "delete-file",
    "create-dir",
    "delete-dir",
    "set-mod-time",
    "pool-for",
    "pool-acquire",
    "pool-events",
    "close-pool-registry",
}


@dataclass
class FixtureState:
    symlink_created: bool = False
    special_created: bool = False
    permission_fixture_attempted: bool = False
    skip_notes: list[str] = field(default_factory=list)


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
        self.sock.sendall((json.dumps(message, separators=(",", ":")) + "\n").encode("utf-8"))

        deadline = time.time() + 30
        while b"\n" not in self.buffer:
            remaining = deadline - time.time()
            if remaining <= 0:
                raise TimeoutError(f"timed out waiting for {method}")
            self.sock.settimeout(remaining)
            chunk = self.sock.recv(65536)
            if not chunk:
                raise ConnectionError(f"connection closed waiting for {method}")
            self.buffer += chunk

        line, _, self.buffer = self.buffer.partition(b"\n")
        response = json.loads(line.decode("utf-8"))
        if response.get("id") != rpc_id:
            raise RuntimeError(f"response id mismatch for {method}: {response}")
        return response

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
        proc.terminate()
        raise RuntimeError("MCP server pipes were not created")

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


def settings(max_connections: int = 2, connect_timeout_ms: int = 10_000, idle_ttl_ms: int = 600) -> dict[str, Any]:
    return {
        "max_connections": max_connections,
        "connect_timeout": connect_timeout_ms,
        "idle_keep_alive_ttl": idle_ttl_ms,
    }


def location(root_path: str, host: str = REMOTE_HOST, port: int | None = REMOTE_PORT) -> dict[str, Any]:
    value: dict[str, Any] = {
        "user": REMOTE_USER,
        "host": host,
        "root_path": root_path,
    }
    if port is not None:
        value["port"] = port
    return value


def auth_config(known_hosts_path: Path) -> dict[str, Any]:
    return {"known_hosts_path": str(known_hosts_path)}


def auth_config_without_credentials(known_hosts_path: Path, agent_socket: Path) -> dict[str, Any]:
    return {
        "known_hosts_path": str(known_hosts_path),
        "ssh_agent_socket": str(agent_socket),
        "private_key_paths": [],
    }


def tool_args(root_path: str, known_hosts_path: Path, max_connections: int = 2) -> dict[str, Any]:
    return {
        "location": location(root_path),
        "settings": settings(max_connections=max_connections),
        "auth_config": auth_config(known_hosts_path),
    }


def result_id(response: dict[str, Any], *names: str) -> str | None:
    result = response.get("result")
    if not isinstance(result, dict):
        return None
    for name in names:
        value = result.get(name)
        if isinstance(value, str) and value:
            return value
    value = result.get("id")
    return value if isinstance(value, str) and value else None


def error_category(response: dict[str, Any]) -> str:
    error = response.get("error")
    if not isinstance(error, dict):
        return ""
    parts = [error["message"]] if isinstance(error.get("message"), str) else []
    data = error.get("data")
    if isinstance(data, dict):
        for key in ("category", "error", "code"):
            value = data.get(key)
            if isinstance(value, str):
                parts.append(value)
    return " ".join(parts)


def expect_success(failures: list[str], label: str, response: dict[str, Any]) -> dict[str, Any]:
    if "error" in response:
        failures.append(f"{label}: expected success, got {response['error']}")
        return {}
    result = response.get("result")
    if not isinstance(result, dict):
        failures.append(f"{label}: expected object result, got {response}")
        return {}
    return result


def expect_error(failures: list[str], label: str, response: dict[str, Any], category: str) -> None:
    error = response.get("error")
    if not isinstance(error, dict):
        failures.append(f"{label}: expected {category} error, got {response}")
        return
    if category not in error_category(response):
        failures.append(f"{label}: expected {category} in error message or data, got {error}")


def expect_tool_error(failures: list[str], label: str, response: dict[str, Any]) -> None:
    if not isinstance(response.get("error"), dict):
        failures.append(f"{label}: expected tool error, got {response}")


def expect_mod_time_near(
    failures: list[str],
    label: str,
    entry: dict[str, Any],
    expected_iso: str,
) -> None:
    value = entry.get("mod_time")
    if not isinstance(value, str):
        failures.append(f"{label}: expected mod_time string, got {entry}")
        return
    try:
        actual = datetime.fromisoformat(value.replace("Z", "+00:00"))
        expected = datetime.fromisoformat(expected_iso.replace("Z", "+00:00"))
    except ValueError:
        failures.append(f"{label}: expected ISO-8601 mod_time, got {entry}")
        return
    actual_offset = actual.utcoffset()
    if actual_offset is None or actual_offset.total_seconds() != 0:
        failures.append(f"{label}: expected UTC mod_time, got {entry}")
        return
    if abs((actual - expected).total_seconds()) > 2:
        failures.append(f"{label}: expected mod_time near {expected_iso}, got {entry}")


def require_tools(failures: list[str], tools_result: dict[str, Any]) -> dict[str, dict[str, Any]]:
    tools = tools_result.get("tools")
    if not isinstance(tools, list):
        failures.append("01: tools/list result must contain a tools array")
        return {}

    by_name: dict[str, dict[str, Any]] = {}
    for tool in tools:
        if not isinstance(tool, dict):
            failures.append(f"01: tool entry must be an object, got {tool!r}")
            continue
        name = tool.get("name")
        if not isinstance(name, str):
            failures.append(f"01: tool name must be a string, got {tool!r}")
            continue
        by_name[name] = tool

    missing = sorted(REQUIRED_TOOLS - set(by_name))
    if missing:
        failures.append(f"01: missing required public API tools: {', '.join(missing)}")
    return by_name


def encoded(data: bytes) -> str:
    return base64.b64encode(data).decode("ascii")


def decoded_read_payload(result: dict[str, Any]) -> tuple[bytes, bool]:
    eof = bool(result.get("eof"))
    value = result.get("data_base64")
    if isinstance(value, str):
        return base64.b64decode(value.encode("ascii")), eof
    if value is None and eof:
        return b"", True
    return b"", eof


def write_known_hosts(path: Path, lines: list[str]) -> None:
    path.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")


def scan_known_hosts(failures: list[str]) -> list[str]:
    try:
        proc = subprocess.run(
            ["ssh-keyscan", "-T", "10", "-p", str(REMOTE_PORT), REMOTE_HOST],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            timeout=20,
            check=False,
        )
    except (FileNotFoundError, subprocess.TimeoutExpired) as exc:
        failures.append(f"00 host-key setup: ssh-keyscan failed for {REMOTE_HOST}: {exc}")
        return []

    lines = [
        line.strip()
        for line in proc.stdout.splitlines()
        if line.strip() and not line.startswith("#")
    ]
    if not lines:
        failures.append(f"00 host-key setup: ssh-keyscan returned no keys for {REMOTE_HOST}: {proc.stderr.strip()}")
    return lines


def fake_known_hosts_line() -> str:
    key = asyncssh.generate_private_key("ssh-rsa")
    return f"{REMOTE_HOST} {key.export_public_key('openssh').decode('ascii').strip()}"


def unused_local_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


async def prepare_external_fixtures(known_hosts: Path, root_path: str) -> FixtureState:
    state = FixtureState()
    try:
        async with asyncssh.connect(
            REMOTE_HOST,
            port=REMOTE_PORT,
            username=REMOTE_USER,
            known_hosts=str(known_hosts),
        ) as conn:
            async with conn.start_sftp_client() as sftp:
                try:
                    await sftp.symlink("file.bin", f"{root_path}/nested/link.bin")
                    state.symlink_created = True
                except (OSError, asyncssh.Error) as exc:
                    state.skip_notes.append(f"symlink exclusion skipped: remote symlink fixture unavailable ({exc})")

                try:
                    result = await conn.run(
                        f"mkfifo {shlex.quote(root_path + '/nested/pipe')}",
                        check=False,
                    )
                    if result.exit_status == 0:
                        state.special_created = True
                    else:
                        state.skip_notes.append(
                            "special-file exclusion skipped: remote account could not create a FIFO"
                        )
                except (OSError, asyncssh.Error) as exc:
                    state.skip_notes.append(f"special-file exclusion skipped: remote shell fixture unavailable ({exc})")

                try:
                    await sftp.chmod(f"{root_path}/denied", 0)
                    state.permission_fixture_attempted = True
                except (OSError, asyncssh.Error) as exc:
                    state.skip_notes.append(f"permission_denied skipped: chmod fixture unavailable ({exc})")
    except (OSError, asyncssh.Error) as exc:
        state.skip_notes.append(f"symlink/special/permission fixtures skipped: setup connection unavailable ({exc})")
    return state


async def cleanup_external_fixtures(known_hosts: Path, root_path: str, state: FixtureState) -> None:
    try:
        async with asyncssh.connect(
            REMOTE_HOST,
            port=REMOTE_PORT,
            username=REMOTE_USER,
            known_hosts=str(known_hosts),
        ) as conn:
            async with conn.start_sftp_client() as sftp:
                if state.permission_fixture_attempted:
                    try:
                        await sftp.chmod(f"{root_path}/denied", 0o700)
                    except (OSError, asyncssh.Error):
                        pass
                for name in ("nested/link.bin", "nested/pipe"):
                    try:
                        await sftp.remove(f"{root_path}/{name}")
                    except (OSError, asyncssh.Error):
                        pass
    except (OSError, asyncssh.Error):
        pass


def call_ignore(rpc: RpcClient, tool: str, args: dict[str, Any]) -> None:
    try:
        rpc.call_tool(tool, args)
    except Exception:
        pass


def cleanup_remote_tree(rpc: RpcClient, fs_id: str) -> None:
    for path in (
        "created-by-write/parent/file.bin",
        "nested/deep/grandchild.txt",
        "nested/renamed.bin",
        "nested/file.bin",
        "denied/secret.txt",
    ):
        call_ignore(rpc, "delete-file", {"filesystem_id": fs_id, "path": path})
    for path in (
        "created-by-write/parent",
        "created-by-write",
        "nested/deep",
        "nested",
        "denied",
        "",
    ):
        call_ignore(rpc, "delete-dir", {"filesystem_id": fs_id, "path": path})


def expect_invalid_path_errors(failures: list[str], rpc: RpcClient, fs_id: str) -> None:
    invalid_path = "nul\x00byte"
    cases = [
        ("list-dir", {"filesystem_id": fs_id, "path": invalid_path}),
        ("stat", {"filesystem_id": fs_id, "path": invalid_path}),
        ("open-read", {"filesystem_id": fs_id, "path": invalid_path}),
        ("open-write", {"filesystem_id": fs_id, "path": invalid_path}),
        ("rename invalid src", {"filesystem_id": fs_id, "src": invalid_path, "dst": "rename-target"}),
        ("rename invalid dst", {"filesystem_id": fs_id, "src": "", "dst": invalid_path}),
        ("delete-file", {"filesystem_id": fs_id, "path": invalid_path}),
        ("create-dir", {"filesystem_id": fs_id, "path": invalid_path}),
        ("delete-dir", {"filesystem_id": fs_id, "path": invalid_path}),
        (
            "set-mod-time",
            {"filesystem_id": fs_id, "path": invalid_path, "mod_time": "2026-05-15T10:30:00Z"},
        ),
    ]
    for label, args in cases:
        tool = label.split()[0]
        response = rpc.call_tool(tool, args)
        expect_error(failures, f"06 invalid path for {label}", response, "invalid_path")
        read_id = result_id(response, "read_handle_id")
        if read_id is not None:
            call_ignore(rpc, "close-read", {"read_handle_id": read_id})
        write_id = result_id(response, "write_handle_id")
        if write_id is not None:
            call_ignore(rpc, "close-write", {"write_handle_id": write_id})


def main() -> int:
    failures: list[str] = []
    tmp_dir = Path(tempfile.mkdtemp(prefix="sftp-protocol-test-"))
    mcp_proc: subprocess.Popen[str] | None = None
    mcp_port = 0
    fs_id: str | None = None
    fixture_state = FixtureState()

    test_root = f"{REMOTE_BASE}/sftp-protocol-{os.getpid()}-{int(time.time() * 1000)}"

    try:
        known_hosts = tmp_dir / "known_hosts"
        mismatched_known_hosts = tmp_dir / "mismatched_known_hosts"
        empty_known_hosts = tmp_dir / "empty_known_hosts"
        host_key_lines = scan_known_hosts(failures)
        if host_key_lines:
            write_known_hosts(known_hosts, host_key_lines)
        write_known_hosts(mismatched_known_hosts, [fake_known_hosts_line()])
        empty_known_hosts.write_text("", encoding="utf-8", newline="\n")

        mcp_proc, mcp_port, stdout_lines, stderr_lines = launch_mcp()
        with socket.create_connection(("127.0.0.1", mcp_port), timeout=10) as sock:
            rpc = RpcClient(sock)

            tools_response = rpc.request("tools/list")
            tools_result = expect_success(failures, "01 tools/list", tools_response)
            tools = require_tools(failures, tools_result)
            print(f"[01] tools/list exposes {len(tools)} tool(s)")

            if not host_key_lines:
                print("[02-17] skipped behavior calls because host-key setup failed")
            elif REQUIRED_TOOLS - set(tools):
                print("[02-17] skipped behavior calls because required tools are missing")
            else:
                invalid_settings_cases = [
                    (
                        "max_connections",
                        {
                            "location": location(test_root),
                            "settings": settings(max_connections=0),
                            "auth_config": auth_config(known_hosts),
                        },
                    ),
                    (
                        "connect_timeout",
                        {
                            "location": location(test_root),
                            "settings": settings(connect_timeout_ms=0),
                            "auth_config": auth_config(known_hosts),
                        },
                    ),
                    (
                        "idle_keep_alive_ttl",
                        {
                            "location": location(test_root),
                            "settings": settings(idle_ttl_ms=0),
                            "auth_config": auth_config(known_hosts),
                        },
                    ),
                ]
                for field, args in invalid_settings_cases:
                    expect_tool_error(failures, f"02 invalid {field}", rpc.call_tool("pool-for", args))

                missing_user = location(test_root)
                missing_user.pop("user")
                missing_host = location(test_root)
                missing_host.pop("host")
                missing_root = location(test_root)
                missing_root.pop("root_path")
                for field, bad_location in (
                    ("user", missing_user),
                    ("host", missing_host),
                    ("root_path", missing_root),
                    ("relative root_path", location("relative-root")),
                ):
                    expect_tool_error(
                        failures,
                        f"02 invalid location {field}",
                        rpc.call_tool(
                            "pool-for",
                            {
                                "location": bad_location,
                                "settings": settings(),
                                "auth_config": auth_config(known_hosts),
                            },
                        ),
                    )
                print("[02] required data shapes reject invalid values")

                unknown_host = rpc.call_tool("open-unpooled", tool_args(test_root, empty_known_hosts))
                expect_error(failures, "03 unknown host key", unknown_host, "host_key_rejected")
                mismatched_host = rpc.call_tool("open-unpooled", tool_args(test_root, mismatched_known_hosts))
                expect_error(failures, "03 mismatched host key", mismatched_host, "host_key_rejected")
                print("[03] host-key verification rejects unknown and mismatched hosts")

                no_credentials = rpc.call_tool(
                    "open-unpooled",
                    {
                        "location": location(test_root),
                        "settings": settings(),
                        "auth_config": auth_config_without_credentials(known_hosts, tmp_dir / "missing-agent.sock"),
                    },
                )
                expect_error(failures, "04 no configured authentication method", no_credentials, "authentication_failed")
                print("[04] missing configured credentials reports authentication_failed")

                closed_port = unused_local_port()
                unreachable = rpc.call_tool(
                    "open-unpooled",
                    {
                        "location": location(test_root, host="127.0.0.1", port=closed_port),
                        "settings": settings(connect_timeout_ms=1_000),
                        "auth_config": auth_config(known_hosts),
                    },
                )
                expect_error(failures, "04 unreachable endpoint", unreachable, "io_error")
                print("[04] unreachable endpoints report connection failure")

                open_response = rpc.call_tool("open-unpooled", tool_args(test_root, known_hosts))
                fs_id = result_id(open_response, "filesystem_id")
                if fs_id is None:
                    failures.append(f"05 open-unpooled: expected filesystem_id, got {open_response}")
                else:
                    print("[05] passwordless default authentication opened an unpooled filesystem")

                    expect_success(
                        failures,
                        "06 create root",
                        rpc.call_tool("create-dir", {"filesystem_id": fs_id, "path": ""}),
                    )
                    root_stat = expect_success(
                        failures,
                        "06 stat root",
                        rpc.call_tool("stat", {"filesystem_id": fs_id, "path": ""}),
                    )
                    if root_stat.get("name") != Path(test_root).name:
                        failures.append(f"06 stat root: expected final root component, got {root_stat}")
                    if root_stat.get("is_dir") is not True:
                        failures.append(f"06 stat root: expected is_dir=true, got {root_stat}")
                    if root_stat.get("byte_size") != -1:
                        failures.append(f"06 stat root: expected directory byte_size=-1, got {root_stat}")

                    for bad_path in ("/absolute", "../escape", "nested/../escape", "nul\x00byte"):
                        expect_error(
                            failures,
                            f"06 invalid path {bad_path!r}",
                            rpc.call_tool("stat", {"filesystem_id": fs_id, "path": bad_path}),
                            "invalid_path",
                        )
                    expect_invalid_path_errors(failures, rpc, fs_id)
                    expect_error(
                        failures,
                        "06 missing path",
                        rpc.call_tool("stat", {"filesystem_id": fs_id, "path": "missing.txt"}),
                        "not_found",
                    )
                    print("[06] root, invalid paths on filesystem operations, and missing paths have specified results")

                    expect_success(
                        failures,
                        "07 create-dir recursive",
                        rpc.call_tool("create-dir", {"filesystem_id": fs_id, "path": "nested/deep"}),
                    )
                    expect_success(
                        failures,
                        "07 create-dir idempotent",
                        rpc.call_tool("create-dir", {"filesystem_id": fs_id, "path": "nested/deep"}),
                    )
                    stat_dir = expect_success(
                        failures,
                        "07 stat created directory",
                        rpc.call_tool("stat", {"filesystem_id": fs_id, "path": "nested/deep"}),
                    )
                    if stat_dir.get("name") != "deep" or stat_dir.get("is_dir") is not True:
                        failures.append(f"07 stat created directory: unexpected entry {stat_dir}")
                    if stat_dir.get("byte_size") != -1:
                        failures.append(f"07 stat created directory: expected byte_size=-1, got {stat_dir}")
                    expect_success(
                        failures,
                        "07 set-mod-time directory",
                        rpc.call_tool(
                            "set-mod-time",
                            {
                                "filesystem_id": fs_id,
                                "path": "nested/deep",
                                "mod_time": "2026-05-15T10:31:00Z",
                            },
                        ),
                    )
                    stat_touched_dir = expect_success(
                        failures,
                        "07 stat touched directory",
                        rpc.call_tool("stat", {"filesystem_id": fs_id, "path": "nested/deep"}),
                    )
                    expect_mod_time_near(
                        failures,
                        "07 set-mod-time directory",
                        stat_touched_dir,
                        "2026-05-15T10:31:00Z",
                    )
                    print("[07] create-dir is recursive and idempotent")

                    payload = b"hello\n\x00world"
                    write_open = rpc.call_tool("open-write", {"filesystem_id": fs_id, "path": "nested/file.bin"})
                    write_id = result_id(write_open, "write_handle_id")
                    if write_id is None:
                        failures.append(f"08 open-write: expected write_handle_id, got {write_open}")
                    else:
                        expect_success(
                            failures,
                            "08 write bytes",
                            rpc.call_tool("write", {"write_handle_id": write_id, "data_base64": encoded(payload)}),
                        )
                        expect_success(
                            failures,
                            "08 close-write",
                            rpc.call_tool("close-write", {"write_handle_id": write_id}),
                        )

                    parent_write_open = rpc.call_tool(
                        "open-write",
                        {"filesystem_id": fs_id, "path": "created-by-write/parent/file.bin"},
                    )
                    parent_write_id = result_id(parent_write_open, "write_handle_id")
                    if parent_write_id is None:
                        failures.append(f"08 open-write missing parents: expected write_handle_id, got {parent_write_open}")
                    else:
                        expect_success(
                            failures,
                            "08 write file with missing parents",
                            rpc.call_tool("write", {"write_handle_id": parent_write_id, "data_base64": encoded(b"x")}),
                        )
                        expect_success(
                            failures,
                            "08 close file with missing parents",
                            rpc.call_tool("close-write", {"write_handle_id": parent_write_id}),
                        )
                        parent_stat = expect_success(
                            failures,
                            "08 stat open-write-created parent",
                            rpc.call_tool("stat", {"filesystem_id": fs_id, "path": "created-by-write/parent"}),
                        )
                        if parent_stat.get("is_dir") is not True:
                            failures.append(f"08 open-write missing parents: expected parent directory, got {parent_stat}")
                    print("[08] open-write creates parents and writes binary content")

                    expect_success(
                        failures,
                        "09 set-mod-time",
                        rpc.call_tool(
                            "set-mod-time",
                            {
                                "filesystem_id": fs_id,
                                "path": "nested/file.bin",
                                "mod_time": "2026-05-15T10:30:00Z",
                            },
                        ),
                    )
                    stat_file = expect_success(
                        failures,
                        "09 stat written file",
                        rpc.call_tool("stat", {"filesystem_id": fs_id, "path": "nested/file.bin"}),
                    )
                    if stat_file.get("name") != "file.bin":
                        failures.append(f"09 stat file: expected final component name, got {stat_file}")
                    if stat_file.get("is_dir") is not False:
                        failures.append(f"09 stat file: expected is_dir=false, got {stat_file}")
                    if stat_file.get("byte_size") != len(payload):
                        failures.append(f"09 stat file: expected byte_size={len(payload)}, got {stat_file}")
                    expect_mod_time_near(
                        failures,
                        "09 stat file mod_time",
                        stat_file,
                        "2026-05-15T10:30:00Z",
                    )
                    print("[09] stat reports regular file metadata and modification time")

                    read_open = rpc.call_tool("open-read", {"filesystem_id": fs_id, "path": "nested/file.bin"})
                    read_id = result_id(read_open, "read_handle_id")
                    if read_id is None:
                        failures.append(f"10 open-read: expected read_handle_id, got {read_open}")
                    else:
                        first = expect_success(
                            failures,
                            "10 read first chunk",
                            rpc.call_tool("read", {"read_handle_id": read_id, "max_bytes": 5}),
                        )
                        second = expect_success(
                            failures,
                            "10 read second chunk",
                            rpc.call_tool("read", {"read_handle_id": read_id, "max_bytes": 100}),
                        )
                        eof = expect_success(
                            failures,
                            "10 read EOF",
                            rpc.call_tool("read", {"read_handle_id": read_id, "max_bytes": 100}),
                        )
                        first_data, first_eof = decoded_read_payload(first)
                        second_data, second_eof = decoded_read_payload(second)
                        eof_data, eof_seen = decoded_read_payload(eof)
                        if first_data != payload[:5]:
                            failures.append(f"10 read: expected max_bytes=5 chunk, got {first_data!r}")
                        if first_data + second_data != payload:
                            failures.append(f"10 read: expected {payload!r}, got {(first_data + second_data)!r}")
                        if first_eof or second_eof:
                            failures.append("10 read: data chunks must not be marked EOF")
                        if eof_seen is not True:
                            failures.append(f"10 read: EOF must be distinct, got {eof}")
                        if eof_data:
                            failures.append(f"10 read: EOF must not include data, got {eof_data!r}")
                        expect_success(
                            failures,
                            "10 close-read",
                            rpc.call_tool("close-read", {"read_handle_id": read_id}),
                        )
                    print("[10] open-read/read/EOF/close-read round-trip binary content")

                    grandchild_open = rpc.call_tool(
                        "open-write",
                        {"filesystem_id": fs_id, "path": "nested/deep/grandchild.txt"},
                    )
                    grandchild_id = result_id(grandchild_open, "write_handle_id")
                    if grandchild_id is not None:
                        expect_success(
                            failures,
                            "11 write grandchild",
                            rpc.call_tool("write", {"write_handle_id": grandchild_id, "data_base64": encoded(b"child\n")}),
                        )
                        expect_success(
                            failures,
                            "11 close grandchild",
                            rpc.call_tool("close-write", {"write_handle_id": grandchild_id}),
                        )
                    else:
                        failures.append(f"11 open grandchild fixture: expected write_handle_id, got {grandchild_open}")

                    expect_success(
                        failures,
                        "11 create denied dir",
                        rpc.call_tool("create-dir", {"filesystem_id": fs_id, "path": "denied"}),
                    )
                    denied_open = rpc.call_tool("open-write", {"filesystem_id": fs_id, "path": "denied/secret.txt"})
                    denied_id = result_id(denied_open, "write_handle_id")
                    if denied_id is not None:
                        expect_success(
                            failures,
                            "11 write denied fixture",
                            rpc.call_tool("write", {"write_handle_id": denied_id, "data_base64": encoded(b"secret")}),
                        )
                        expect_success(
                            failures,
                            "11 close denied fixture",
                            rpc.call_tool("close-write", {"write_handle_id": denied_id}),
                        )

                    fixture_state = asyncio.run(prepare_external_fixtures(known_hosts, test_root))
                    for note in fixture_state.skip_notes:
                        print(f"[11] {note}")

                    listing = expect_success(
                        failures,
                        "11 list-dir",
                        rpc.call_tool("list-dir", {"filesystem_id": fs_id, "path": "nested"}),
                    )
                    entries = listing.get("entries")
                    entry_by_name = {}
                    if isinstance(entries, list):
                        entry_by_name = {
                            entry.get("name"): entry
                            for entry in entries
                            if isinstance(entry, dict)
                        }
                    names = sorted(name for name in entry_by_name if isinstance(name, str))
                    if names != ["deep", "file.bin"]:
                        failures.append(f"11 list-dir: expected immediate regular file and dir only, got {listing}")
                    deep_entry = entry_by_name.get("deep")
                    file_entry = entry_by_name.get("file.bin")
                    if not isinstance(deep_entry, dict) or deep_entry.get("is_dir") is not True:
                        failures.append(f"11 list-dir: expected deep directory entry, got {deep_entry}")
                    elif deep_entry.get("byte_size") != -1:
                        failures.append(f"11 list-dir: expected directory byte_size=-1, got {deep_entry}")
                    if not isinstance(file_entry, dict) or file_entry.get("is_dir") is not False:
                        failures.append(f"11 list-dir: expected file.bin regular-file entry, got {file_entry}")
                    elif file_entry.get("byte_size") != len(payload):
                        failures.append(f"11 list-dir: expected file byte_size={len(payload)}, got {file_entry}")
                    if isinstance(file_entry, dict) and not file_entry.get("mod_time"):
                        failures.append(f"11 list-dir: expected file mod_time, got {file_entry}")
                    if fixture_state.symlink_created:
                        expect_error(
                            failures,
                            "11 stat symlink",
                            rpc.call_tool("stat", {"filesystem_id": fs_id, "path": "nested/link.bin"}),
                            "not_found",
                        )
                    if fixture_state.special_created:
                        expect_error(
                            failures,
                            "11 stat special file",
                            rpc.call_tool("stat", {"filesystem_id": fs_id, "path": "nested/pipe"}),
                            "not_found",
                        )
                    if fixture_state.permission_fixture_attempted:
                        denied = rpc.call_tool("list-dir", {"filesystem_id": fs_id, "path": "denied"})
                        if "permission_denied" in error_category(denied):
                            print("[11] permission-denied fixture reported permission_denied")
                        elif "error" not in denied:
                            print("[11] permission_denied skipped: remote server did not enforce chmod fixture")
                        else:
                            failures.append(f"11 permission_denied: expected permission_denied or unenforced fixture, got {denied}")
                    print("[11] list-dir/stat omit non-regular entries when fixtures are available")

                    asyncio.run(cleanup_external_fixtures(known_hosts, test_root, fixture_state))

                    expect_error(
                        failures,
                        "12 rename missing parent",
                        rpc.call_tool(
                            "rename",
                            {"filesystem_id": fs_id, "src": "nested/file.bin", "dst": "missing-parent/file.bin"},
                        ),
                        "not_found",
                    )
                    expect_success(
                        failures,
                        "12 rename existing parent",
                        rpc.call_tool(
                            "rename",
                            {"filesystem_id": fs_id, "src": "nested/file.bin", "dst": "nested/renamed.bin"},
                        ),
                    )
                    expect_error(
                        failures,
                        "12 old path not found",
                        rpc.call_tool("stat", {"filesystem_id": fs_id, "path": "nested/file.bin"}),
                        "not_found",
                    )
                    renamed_stat = expect_success(
                        failures,
                        "12 renamed path exists",
                        rpc.call_tool("stat", {"filesystem_id": fs_id, "path": "nested/renamed.bin"}),
                    )
                    if renamed_stat.get("byte_size") != len(payload):
                        failures.append(f"12 renamed path: expected byte_size={len(payload)}, got {renamed_stat}")
                    print("[12] rename moves entries without creating parents")

                    expect_success(
                        failures,
                        "13 delete-file",
                        rpc.call_tool("delete-file", {"filesystem_id": fs_id, "path": "nested/renamed.bin"}),
                    )
                    expect_error(
                        failures,
                        "13 deleted file missing",
                        rpc.call_tool("stat", {"filesystem_id": fs_id, "path": "nested/renamed.bin"}),
                        "not_found",
                    )
                    expect_success(
                        failures,
                        "13 delete grandchild",
                        rpc.call_tool("delete-file", {"filesystem_id": fs_id, "path": "nested/deep/grandchild.txt"}),
                    )
                    expect_success(
                        failures,
                        "13 delete-dir",
                        rpc.call_tool("delete-dir", {"filesystem_id": fs_id, "path": "nested/deep"}),
                    )
                    expect_error(
                        failures,
                        "13 deleted dir missing",
                        rpc.call_tool("stat", {"filesystem_id": fs_id, "path": "nested/deep"}),
                        "not_found",
                    )
                    print("[13] delete-file and delete-dir remove remote entries")

                pool_a = rpc.call_tool(
                    "pool-for",
                    {
                        "location": location(test_root, host=REMOTE_HOST.upper(), port=None),
                        "settings": settings(max_connections=1, idle_ttl_ms=1_500),
                        "auth_config": auth_config(known_hosts),
                        "record_events": True,
                    },
                )
                pool_b_location = location(f"{test_root}-other-root", host=REMOTE_HOST.lower(), port=22)
                pool_b_location["password"] = "ignored-for-pool-key"
                pool_b = rpc.call_tool(
                    "pool-for",
                    {
                        "location": pool_b_location,
                        "settings": settings(max_connections=2, idle_ttl_ms=5_000),
                        "auth_config": auth_config_without_credentials(
                            known_hosts,
                            tmp_dir / "missing-later-agent.sock",
                        ),
                        "record_events": False,
                    },
                )
                pool_id = result_id(pool_a, "pool_id")
                pool_b_id = result_id(pool_b, "pool_id")
                if pool_id is None or pool_b_id is None:
                    failures.append(f"14 pool-for: expected pool ids, got {pool_a} and {pool_b}")
                elif pool_id != pool_b_id:
                    failures.append(
                        "14 pool key: host case, omitted port 22, different roots, and different passwords must share a pool"
                    )
                else:
                    first = rpc.call_tool("pool-acquire", {"pool_id": pool_id})
                    first_fs = result_id(first, "filesystem_id")
                    if first_fs is None:
                        failures.append(f"14 first pool-acquire: expected filesystem_id, got {first}")
                    else:
                        expect_error(
                            failures,
                            "14 failed pooled operation",
                            rpc.call_tool("stat", {"filesystem_id": first_fs, "path": "missing-after-pool-acquire"}),
                            "not_found",
                        )

                        second_result: dict[str, Any] = {}
                        second_error: list[BaseException] = []

                        def acquire_pool(result: dict[str, Any], errors: list[BaseException]) -> None:
                            try:
                                with socket.create_connection(("127.0.0.1", mcp_port), timeout=10) as second_sock:
                                    second_rpc = RpcClient(second_sock)
                                    result.update(second_rpc.call_tool("pool-acquire", {"pool_id": pool_id}))
                            except BaseException as exc:  # pragma: no cover - reported below
                                errors.append(exc)

                        second_thread = threading.Thread(
                            target=acquire_pool,
                            args=(second_result, second_error),
                            daemon=True,
                        )
                        second_thread.start()
                        time.sleep(1.0)
                        if not second_thread.is_alive():
                            failures.append("14 pool acquire: max_connections=1 did not block a second borrower")

                        expect_success(
                            failures,
                            "14 close first pooled filesystem",
                            rpc.call_tool("close-filesystem", {"filesystem_id": first_fs}),
                        )
                        second_thread.join(timeout=20)
                        if second_thread.is_alive():
                            failures.append("14 second pool-acquire: did not finish after release")
                        if second_error:
                            failures.append(f"14 second pool-acquire: raised {second_error[0]}")
                        second_fs = result_id(second_result, "filesystem_id")
                        if not second_thread.is_alive() and not second_error and second_fs is None:
                            failures.append(f"14 second pool-acquire: expected filesystem_id, got {second_result}")
                        if second_fs is not None:
                            expect_success(
                                failures,
                                "14 second pooled stat",
                                rpc.call_tool("stat", {"filesystem_id": second_fs, "path": ""}),
                            )

                        if second_fs is not None:
                            expect_success(
                                failures,
                                "14 close second pooled filesystem",
                                rpc.call_tool("close-filesystem", {"filesystem_id": second_fs}),
                            )

                        time.sleep(2.0)
                        events = expect_success(
                            failures,
                            "14 pool-events",
                            rpc.call_tool("pool-events", {"pool_id": pool_id}),
                        ).get("events")
                        if not isinstance(events, list) or len(events) < 5:
                            failures.append(f"14 pool-events: expected acquire/release/idle-timeout events, got {events}")
                        else:
                            endpoint = f"{REMOTE_USER}@{REMOTE_HOST}:{REMOTE_PORT}"
                            open_counts: list[int] = []
                            for event in events:
                                if not isinstance(event, dict):
                                    failures.append(f"14 pool-events: event must be an object, got {event!r}")
                                    continue
                                if event.get("endpoint") != endpoint:
                                    failures.append(f"14 pool-events: expected endpoint {endpoint}, got {event}")
                                if event.get("max_connections") != 1:
                                    failures.append(f"14 pool-events: first settings must fix max_connections=1, got {event}")
                                if isinstance(event.get("open_connections"), int):
                                    open_counts.append(event["open_connections"])
                                else:
                                    failures.append(f"14 pool-events: open_connections must be an integer, got {event}")
                            if 0 not in open_counts:
                                failures.append(f"14 pool-events: expected idle-timeout close to report open_connections=0, got {open_counts}")
                            if 2 in open_counts:
                                failures.append(f"14 pool-events: max_connections=1 pool opened too many sessions, got {open_counts}")
                    print("[14] transfer pool identity, acquisition limit, blocking acquire, and events checked")

                if fs_id is not None:
                    asyncio.run(cleanup_external_fixtures(known_hosts, test_root, fixture_state))
                    cleanup_remote_tree(rpc, fs_id)
                    expect_success(failures, "15 close filesystem", rpc.call_tool("close-filesystem", {"filesystem_id": fs_id}))
                    fixture_state = FixtureState()
                    fs_id = None
                    print("[15] remote test tree cleaned up and filesystem closed")

                expect_success(failures, "16 close pool registry", rpc.call_tool("close-pool-registry", {}))
                expect_success(
                    failures,
                    "16 close pool registry idempotent",
                    rpc.call_tool("close-pool-registry", {}),
                )
                print("[16] pool registry close is idempotent")

                # Not exercised: auth-method ordering is not observable through
                # the public API without server-side authentication tracing.
                # Not exercised: exact SSH handshake timeout timing depends on
                # network scheduling; the unreachable-endpoint check above only
                # verifies the public connection-failure category.
                # Not exercised: broad thread-safety and concurrent distinct
                # filesystem use are not deterministically provable through
                # this wrapper; the blocking acquire check exercises the public
                # concurrency surface without depending on tight timing.
                # Not exercised: close_read-after-failed-read and protocol
                # corruption require sabotaged handles/transports rather than
                # normal caller inputs through the MCP request surface.
                # Not exercised: registry close of borrowed underlying
                # sessions is not distinguishable through the MCP wrapper,
                # which owns and removes public filesystem handles.
                # Permission-denied, symlink, and special-file behavior is
                # exercised above only when the required remote account allows
                # the corresponding fixture to be arranged.

        time.sleep(0.2)
        extra_stdout = stdout_lines[1:]
        if extra_stdout:
            failures.append(f"17 public operations must not write stdout, got {extra_stdout!r}")
        if stderr_lines:
            failures.append(f"17 public operations must not write stderr, got {stderr_lines!r}")
        print("[17] public operations produced no diagnostics")

        if failures:
            print("\nFAILURES:")
            for failure in failures:
                print(f"  - {failure}")
            return 1

        print("\nAll assertions passed.")
        return 0
    finally:
        if mcp_proc is not None and fs_id is not None:
            try:
                with socket.create_connection(("127.0.0.1", mcp_port), timeout=5) as cleanup_sock:
                    cleanup_rpc = RpcClient(cleanup_sock)
                    asyncio.run(cleanup_external_fixtures(known_hosts, test_root, fixture_state))
                    cleanup_remote_tree(cleanup_rpc, fs_id)
                    call_ignore(cleanup_rpc, "close-filesystem", {"filesystem_id": fs_id})
            except Exception:
                pass
        if mcp_proc is not None:
            shutdown_mcp(mcp_proc, mcp_port)
        try:
            for child in tmp_dir.iterdir():
                child.unlink()
            tmp_dir.rmdir()
        except OSError:
            pass


if __name__ == "__main__":
    sys.exit(main())
