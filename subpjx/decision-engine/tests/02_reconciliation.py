#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercise universal per-participant reconciliation through the MCP wrapper."""

from __future__ import annotations

import json
import os
import platform
import socket
import subprocess
import sys
import threading
import time
from pathlib import Path
from typing import Any


def _default_uv() -> Path:
    system = platform.system()
    if system == "Windows":
        return Path("./aitc/bin/uv.exe")
    if system == "Darwin":
        return Path("./aitc/bin/uv.mac")
    return Path("./aitc/bin/uv.linux")


BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", str(_default_uv())))
PROJECT = os.environ.get("AITC_PROJECT", ".")


def _drain(stream: Any) -> None:
    for _ in stream:
        pass


def _terminate(proc: subprocess.Popen[str]) -> None:
    proc.terminate()
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait()


def _launch() -> tuple[subprocess.Popen[str], int]:
    proc = subprocess.Popen(
        [str(UV), "run", "--script", str(BUILD_PY), "launch-mcp", PROJECT],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
    )
    port = None
    deadline = time.time() + 30
    while time.time() < deadline and proc.poll() is None:
        if proc.stdout is None:
            break
        line = proc.stdout.readline()
        if not line:
            continue
        line = line.strip()
        if line.startswith("MCP_PORT="):
            port = int(line.split("=", 1)[1])
            break
    if port is None:
        _terminate(proc)
        raise RuntimeError("MCP server did not advertise MCP_PORT")
    if proc.stdout is not None:
        threading.Thread(target=_drain, args=(proc.stdout,), daemon=True).start()
    if proc.stderr is not None:
        threading.Thread(target=_drain, args=(proc.stderr,), daemon=True).start()
    return proc, port


class JsonRpc:
    def __init__(self, sock: socket.socket) -> None:
        self.sock = sock
        self.next_id = 1

    def call(self, method: str, params: dict[str, Any] | None = None) -> dict[str, Any]:
        rpc_id = self.next_id
        self.next_id += 1
        message: dict[str, Any] = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
        if params is not None:
            message["params"] = params
        payload = json.dumps(message, separators=(",", ":")).encode("utf-8") + b"\n"
        self.sock.sendall(payload)

        data = b""
        deadline = time.time() + 10
        while time.time() < deadline:
            chunk = self.sock.recv(8192)
            if not chunk:
                break
            data += chunk
            if b"\n" in data:
                break
        line, _, _ = data.partition(b"\n")
        if not line:
            raise RuntimeError(f"no JSON-RPC response for {method}")
        return json.loads(line.decode("utf-8"))


def _file(mod_time: int, byte_size: int) -> dict[str, Any]:
    return {"kind": "File", "mod_time": mod_time, "byte_size": byte_size}


def _directory() -> dict[str, Any]:
    return {"kind": "Directory"}


def _absent() -> dict[str, Any]:
    return {"kind": "Absent"}


def _history(
    mod_time: int,
    byte_size: int,
    last_seen: int | None = None,
    deleted_time: int | None = None,
) -> dict[str, Any]:
    return {
        "mod_time": mod_time,
        "byte_size": byte_size,
        "last_seen": last_seen,
        "deleted_time": deleted_time,
    }


def _file_case() -> dict[str, Any]:
    return {
        "roles": {
            "winner": "contributing",
            "matching": "subordinate",
            "missing": "contributing",
            "stale": "contributing",
            "wrongsize": "contributing",
            "wrongdir": "contributing",
        },
        "observations": {
            "winner": _file(997, 64),
            "matching": _file(1000, 64),
            "missing": _absent(),
            "stale": _file(900, 64),
            "wrongsize": _file(1000, 32),
            "wrongdir": _directory(),
        },
        "histories": {},
        "tolerance": 5,
    }


def _directory_case() -> dict[str, Any]:
    return {
        "roles": {
            "dirwinner": "canon",
            "dirmatch": "contributing",
            "dirfile": "contributing",
            "dirmissing": "contributing",
        },
        "observations": {
            "dirwinner": _directory(),
            "dirmatch": _directory(),
            "dirfile": _file(2000, 64),
            "dirmissing": _absent(),
        },
        "histories": {
            "dirwinner": _history(2000, -1, 2000),
            "dirmatch": _history(1990, -1, 1990),
        },
        "tolerance": 5,
    }


def _none_case() -> dict[str, Any]:
    return {
        "roles": {
            "absentmatch": "contributing",
            "fileundernone": "subordinate",
            "dirundernone": "subordinate",
        },
        "observations": {
            "absentmatch": _absent(),
            "fileundernone": _file(3000, 16),
            "dirundernone": _directory(),
        },
        "histories": {},
        "tolerance": 5,
    }


def _unwrap_result(response: dict[str, Any]) -> dict[str, Any]:
    if "error" in response:
        return {"__error__": response["error"]}
    result = response.get("result")
    return result if isinstance(result, dict) else {"__error__": f"non-object result: {result!r}"}


def _decide(rpc: JsonRpc, arguments: dict[str, Any]) -> dict[str, Any]:
    try:
        response = rpc.call("tools/call", {"name": "decide-entry", "arguments": arguments})
        return _unwrap_result(response)
    except Exception as exc:
        return {"__error__": str(exc)}


def _entry_kind(decision: dict[str, Any]) -> Any:
    return decision.get("entry_kind")


def _has_field(decision: dict[str, Any], name: str) -> bool:
    return name in decision


def _field(decision: dict[str, Any], name: str) -> Any:
    return decision.get(name)


def _actions(decision: dict[str, Any]) -> dict[str, Any]:
    actions = decision.get("actions")
    return actions if isinstance(actions, dict) else {}


def _action_value(decision: dict[str, Any], participant: str) -> Any:
    return _actions(decision).get(participant)


def _action_kind(decision: dict[str, Any], participant: str) -> Any:
    action = _action_value(decision, participant)
    if isinstance(action, dict):
        return action.get("kind")
    return None


def _action_source(decision: dict[str, Any], participant: str) -> Any:
    action = _action_value(decision, participant)
    if isinstance(action, dict):
        return action.get("source")
    return None


def _check(
    req_id: str,
    description: str,
    passed: bool,
    failures: list[str],
    detail: str,
) -> None:
    status = "PASS" if passed else "FAIL"
    print(f"[{req_id}] {status} - {description}")
    if not passed:
        failures.append(f"{req_id}: {detail}")


def _run_assertions(
    file_decision: dict[str, Any],
    directory_decision: dict[str, Any],
    none_decision: dict[str, Any],
    failures: list[str],
) -> None:
    file_error = file_decision.get("__error__")
    directory_error = directory_decision.get("__error__")
    none_error = none_decision.get("__error__")

    _check(
        "02.9",
        "matching file observation gets NoOp",
        not file_error and _entry_kind(file_decision) == "File" and _action_kind(file_decision, "matching") == "NoOp",
        failures,
        f"expected matching NoOp, got {file_error or _action_value(file_decision, 'matching')!r}",
    )
    _check(
        "02.10",
        "matching directory observation gets NoOp",
        not directory_error
        and _entry_kind(directory_decision) == "Directory"
        and _action_kind(directory_decision, "dirmatch") == "NoOp",
        failures,
        f"expected dirmatch NoOp, got {directory_error or _action_value(directory_decision, 'dirmatch')!r}",
    )
    _check(
        "02.11",
        "absent observation gets NoOp when entry_kind is None",
        not none_error and _entry_kind(none_decision) == "None" and _action_kind(none_decision, "absentmatch") == "NoOp",
        failures,
        f"expected absentmatch NoOp under None, got {none_error or _action_value(none_decision, 'absentmatch')!r}",
    )
    _check(
        "02.12",
        "missing participant receives winning file",
        not file_error
        and _entry_kind(file_decision) == "File"
        and _action_kind(file_decision, "missing") == "ReceiveFile"
        and _action_source(file_decision, "missing") == "winner",
        failures,
        f"expected missing ReceiveFile from winner, got {file_error or _action_value(file_decision, 'missing')!r}",
    )
    _check(
        "02.13",
        "missing participant creates decided directory",
        not directory_error
        and _entry_kind(directory_decision) == "Directory"
        and _action_kind(directory_decision, "dirmissing") == "CreateDirectory",
        failures,
        f"expected dirmissing CreateDirectory, got {directory_error or _action_value(directory_decision, 'dirmissing')!r}",
    )
    _check(
        "02.14",
        "non-matching file observation gets Displace",
        not file_error
        and not directory_error
        and not none_error
        and _entry_kind(file_decision) == "File"
        and _entry_kind(directory_decision) == "Directory"
        and _entry_kind(none_decision) == "None"
        and _action_kind(file_decision, "stale") == "Displace"
        and _action_kind(file_decision, "wrongsize") == "Displace"
        and _action_kind(directory_decision, "dirfile") == "Displace"
        and _action_kind(none_decision, "fileundernone") == "Displace",
        failures,
        "expected stale, wrongsize, dirfile, and fileundernone Displace, got "
        f"stale={file_error or _action_value(file_decision, 'stale')!r}, "
        f"wrongsize={file_error or _action_value(file_decision, 'wrongsize')!r}, "
        f"dirfile={directory_error or _action_value(directory_decision, 'dirfile')!r}, "
        f"fileundernone={none_error or _action_value(none_decision, 'fileundernone')!r}",
    )
    _check(
        "02.15",
        "file decision reports winning_mod_time from winning source",
        not file_error and _entry_kind(file_decision) == "File" and _field(file_decision, "winning_mod_time") == 997,
        failures,
        f"expected file winning_mod_time 997, got {file_error or file_decision!r}",
    )
    _check(
        "02.16",
        "file decision reports winning_byte_size from winning source",
        not file_error and _entry_kind(file_decision) == "File" and _field(file_decision, "winning_byte_size") == 64,
        failures,
        f"expected file winning_byte_size 64, got {file_error or file_decision!r}",
    )
    _check(
        "02.17",
        "file decision reports winning_source",
        not file_error and _entry_kind(file_decision) == "File" and _field(file_decision, "winning_source") == "winner",
        failures,
        f"expected file winning_source winner, got {file_error or file_decision!r}",
    )
    _check(
        "02.18",
        "directory decision reports winning_mod_time",
        not directory_error
        and _entry_kind(directory_decision) == "Directory"
        and isinstance(_field(directory_decision, "winning_mod_time"), int),
        failures,
        f"expected directory winning_mod_time field, got {directory_error or directory_decision!r}",
    )
    _check(
        "02.19",
        "directory decision reports byte-size sentinel",
        not directory_error
        and _entry_kind(directory_decision) == "Directory"
        and _field(directory_decision, "winning_byte_size") == -1,
        failures,
        f"expected directory winning_byte_size -1, got {directory_error or directory_decision!r}",
    )
    _check(
        "02.20",
        "directory decision omits winning_source",
        not directory_error and _entry_kind(directory_decision) == "Directory" and not _has_field(directory_decision, "winning_source"),
        failures,
        f"expected no directory winning_source, got {directory_error or directory_decision!r}",
    )
    _check(
        "02.21",
        "None decision omits winning_mod_time",
        not none_error and _entry_kind(none_decision) == "None" and not _has_field(none_decision, "winning_mod_time"),
        failures,
        f"expected no None winning_mod_time, got {none_error or none_decision!r}",
    )
    _check(
        "02.22",
        "None decision omits winning_byte_size",
        not none_error and _entry_kind(none_decision) == "None" and not _has_field(none_decision, "winning_byte_size"),
        failures,
        f"expected no None winning_byte_size, got {none_error or none_decision!r}",
    )
    _check(
        "02.23",
        "None decision omits winning_source",
        not none_error and _entry_kind(none_decision) == "None" and not _has_field(none_decision, "winning_source"),
        failures,
        f"expected no None winning_source, got {none_error or none_decision!r}",
    )
    _check(
        "02.24",
        "non-matching directory observation gets Displace",
        not file_error
        and not none_error
        and _entry_kind(file_decision) == "File"
        and _entry_kind(none_decision) == "None"
        and _action_kind(file_decision, "wrongdir") == "Displace"
        and _action_kind(none_decision, "dirundernone") == "Displace",
        failures,
        "expected wrongdir and dirundernone Displace, got "
        f"wrongdir={file_error or _action_value(file_decision, 'wrongdir')!r}, "
        f"dirundernone={none_error or _action_value(none_decision, 'dirundernone')!r}",
    )
    _check(
        "02.25",
        "decision reports an action for each participant identifier",
        not file_error
        and not directory_error
        and not none_error
        and set(_actions(file_decision)) == set(_file_case()["roles"])
        and set(_actions(directory_decision)) == set(_directory_case()["roles"])
        and set(_actions(none_decision)) == set(_none_case()["roles"]),
        failures,
        "expected actions for every participant, got "
        f"file={file_error or _actions(file_decision)!r}, "
        f"directory={directory_error or _actions(directory_decision)!r}, "
        f"none={none_error or _actions(none_decision)!r}",
    )
    _check(
        "02.26",
        "decision reports entry_kind as File, Directory, or None",
        not file_error
        and not directory_error
        and not none_error
        and {
            _entry_kind(file_decision),
            _entry_kind(directory_decision),
            _entry_kind(none_decision),
        }
        == {"File", "Directory", "None"},
        failures,
        "expected entry_kind values File, Directory, and None, got "
        f"file={file_error or _entry_kind(file_decision)!r}, "
        f"directory={directory_error or _entry_kind(directory_decision)!r}, "
        f"none={none_error or _entry_kind(none_decision)!r}",
    )


def main() -> int:
    failures: list[str] = []
    proc: subprocess.Popen[str] | None = None
    file_decision: dict[str, Any] = {}
    directory_decision: dict[str, Any] = {}
    none_decision: dict[str, Any] = {}

    try:
        proc, port = _launch()
        with socket.create_connection(("127.0.0.1", port), timeout=10) as sock:
            sock.settimeout(10)
            rpc = JsonRpc(sock)
            tools_response = rpc.call("tools/list")
            tools = (tools_response.get("result") or {}).get("tools", [])
            tool_names = {
                tool.get("name")
                for tool in tools
                if isinstance(tool, dict) and isinstance(tool.get("name"), str)
            }
            print(f"[setup] tools/list returned {len(tools)} tool(s)")
            if "decide-entry" not in tool_names:
                error = f"decide-entry tool missing from tools/list: {sorted(tool_names)!r}"
                file_decision = {"__error__": error}
                directory_decision = {"__error__": error}
                none_decision = {"__error__": error}
            else:
                file_decision = _decide(rpc, _file_case())
                directory_decision = _decide(rpc, _directory_case())
                none_decision = _decide(rpc, _none_case())
    except Exception as exc:
        error = f"setup failed: {exc}"
        file_decision = {"__error__": error}
        directory_decision = {"__error__": error}
        none_decision = {"__error__": error}
    finally:
        if proc is not None:
            _terminate(proc)

    _run_assertions(file_decision, directory_decision, none_decision, failures)

    if failures:
        print("\nFAILURES:")
        for failure in failures:
            print(f"  - {failure}")
        return 1
    print("\nAll assertions passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
