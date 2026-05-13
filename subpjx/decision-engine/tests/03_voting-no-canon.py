#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercise no-canon voting rules through the decision-engine MCP wrapper."""

from __future__ import annotations

import json
import os
import queue
import socket
import subprocess
import sys
import threading
import time
from pathlib import Path
from typing import Any


BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
PROJECT = os.environ.get("AITC_PROJECT", ".")


def _default_uv() -> Path:
    if "AITC_UV" in os.environ:
        return Path(os.environ["AITC_UV"])
    if sys.platform.startswith("win"):
        return Path("./aitc/bin/uv.exe")
    if sys.platform == "darwin":
        return Path("./aitc/bin/uv.mac")
    return Path("./aitc/bin/uv.linux")


UV = _default_uv()


def _collect_lines(stream: Any, lines: list[str], q: queue.Queue[str] | None = None) -> None:
    for line in stream:
        lines.append(line)
        if q is not None:
            q.put(line)


def _launch() -> tuple[subprocess.Popen[str], int]:
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

    stdout_lines: list[str] = []
    stderr_lines: list[str] = []
    stdout_q: queue.Queue[str] = queue.Queue()
    threading.Thread(
        target=_collect_lines, args=(proc.stdout, stdout_lines, stdout_q), daemon=True
    ).start()
    threading.Thread(target=_collect_lines, args=(proc.stderr, stderr_lines), daemon=True).start()

    deadline = time.time() + 30
    while time.time() < deadline:
        if proc.poll() is not None:
            break
        try:
            line = stdout_q.get(timeout=0.1).strip()
        except queue.Empty:
            continue
        if line.startswith("MCP_PORT="):
            return proc, int(line.split("=", 1)[1])

    proc.terminate()
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait()
    stderr_tail = "".join(stderr_lines[-20:]).strip()
    raise RuntimeError(f"MCP server did not advertise MCP_PORT; stderr={stderr_tail!r}")


class Rpc:
    def __init__(self, sock: socket.socket) -> None:
        self._sock = sock
        self._file = sock.makefile("rwb", buffering=0)
        self._next_id = 1

    def call(self, method: str, params: dict[str, Any] | None = None) -> dict[str, Any]:
        rpc_id = self._next_id
        self._next_id += 1
        msg: dict[str, Any] = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
        if params is not None:
            msg["params"] = params
        self._file.write((json.dumps(msg, sort_keys=True) + "\n").encode("utf-8"))
        line = self._file.readline()
        if not line:
            raise RuntimeError(f"MCP server closed connection during {method}")
        response = json.loads(line.decode("utf-8"))
        if response.get("id") != rpc_id:
            raise RuntimeError(f"unexpected JSON-RPC id for {method}: {response!r}")
        return response


def _choose_decide_tool(tools: list[dict[str, Any]]) -> str | None:
    names = [str(tool.get("name", "")) for tool in tools]
    if "decide-entry" in names:
        return "decide-entry"
    for name in names:
        if "decide" in name and "entry" in name:
            return name
    return None


def _file(mod_time: int, byte_size: int) -> dict[str, Any]:
    return {"kind": "File", "mod_time": mod_time, "byte_size": byte_size}


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


def _arguments(
    roles: dict[str, str],
    observations: dict[str, dict[str, Any]],
    histories: dict[str, dict[str, Any]],
    tolerance: int = 5,
) -> dict[str, Any]:
    return {
        "roles": roles,
        "observations": observations,
        "histories": histories,
        "tolerance": tolerance,
    }


def _decide(rpc: Rpc, tool_name: str | None, args: dict[str, Any]) -> dict[str, Any]:
    if tool_name is None:
        return {"_error": "decide-entry tool missing"}
    response = rpc.call("tools/call", {"name": tool_name, "arguments": args})
    if "error" in response:
        return {"_error": response["error"]}
    return dict(response.get("result") or {})


def _get(result: dict[str, Any], *names: str) -> Any:
    for name in names:
        if name in result:
            return result[name]
    return None


def _action(result: dict[str, Any], participant: str) -> Any:
    actions = _get(result, "actions", "participant_actions")
    if isinstance(actions, dict):
        return actions.get(participant)
    return None


def _kind(value: Any) -> str | None:
    if isinstance(value, str):
        return value
    if isinstance(value, dict):
        for key in ("kind", "type", "action"):
            if key in value:
                return str(value[key])
        if len(value) == 1:
            return str(next(iter(value)))
    return None


def _source(value: Any) -> str | None:
    if isinstance(value, dict):
        if "source" in value:
            return str(value["source"])
        for nested in value.values():
            if isinstance(nested, dict) and "source" in nested:
                return str(nested["source"])
    return None


def _entry_kind(result: dict[str, Any]) -> str | None:
    value = _get(result, "entry_kind", "entryKind")
    if value is None:
        return None
    return str(value)


def _winning_source(result: dict[str, Any]) -> str | None:
    value = _get(result, "winning_source", "winningSource")
    if value is None:
        return None
    return str(value)


def _winning_mod_time(result: dict[str, Any]) -> int | None:
    value = _get(result, "winning_mod_time", "winningModTime")
    return int(value) if value is not None else None


def _winning_byte_size(result: dict[str, Any]) -> int | None:
    value = _get(result, "winning_byte_size", "winningByteSize")
    return int(value) if value is not None else None


def _classification(result: dict[str, Any], participant: str) -> str | None:
    classifications = _get(result, "classifications", "participant_classifications")
    if isinstance(classifications, dict) and participant in classifications:
        return _kind(classifications[participant])
    return None


def _print_result(req_id: str, description: str, passed: bool, failures: list[str], detail: str) -> None:
    status = "PASS" if passed else "FAIL"
    print(f"[{req_id}] {description}: {status}")
    if not passed:
        failures.append(f"{req_id}: {detail}")


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as sock:
            sock.settimeout(10)
            rpc = Rpc(sock)
            tools_response = rpc.call("tools/list")
            tools = (tools_response.get("result") or {}).get("tools", [])
            tool_name = _choose_decide_tool(tools if isinstance(tools, list) else [])
            failures: list[str] = []

            unchanged = _decide(
                rpc,
                tool_name,
                _arguments(
                    roles={"alice": "contributing", "bob": "contributing"},
                    observations={"alice": _file(1000, 10), "bob": _file(1000, 10)},
                    histories={
                        "alice": _history(1000, 10, last_seen=1000),
                        "bob": _history(1000, 10, last_seen=1000),
                    },
                ),
            )
            actions_noop = all(
                _kind(_action(unchanged, participant)) == "NoOp"
                for participant in ("alice", "bob")
            )
            all_classified_unchanged = all(
                _classification(unchanged, participant) == "Unchanged"
                for participant in ("alice", "bob")
            )
            _print_result(
                "03.9",
                "all Unchanged contributors produce only NoOp actions",
                all_classified_unchanged and actions_noop,
                failures,
                f"result={unchanged!r}",
            )

            newest = _decide(
                rpc,
                tool_name,
                _arguments(
                    roles={
                        "older": "contributing",
                        "newer": "contributing",
                        "missing": "contributing",
                        "noise": "subordinate",
                    },
                    observations={
                        "older": _file(1000, 90),
                        "newer": _file(1020, 20),
                        "missing": _absent(),
                        "noise": _file(5000, 999),
                    },
                    histories={
                        "older": _history(990, 90, last_seen=990),
                        "newer": _history(990, 10, last_seen=990),
                    },
                ),
            )
            newest_wins = (
                _entry_kind(newest) == "File"
                and _winning_source(newest) == "newer"
                and _winning_mod_time(newest) == 1020
                and _winning_byte_size(newest) == 20
            )
            _print_result(
                "03.10",
                "newest contributing live observation provides file metadata",
                newest_wins,
                failures,
                f"result={newest!r}",
            )

            tolerance_tie = _decide(
                rpc,
                tool_name,
                _arguments(
                    roles={"larger": "contributing", "newer": "contributing"},
                    observations={"larger": _file(2000, 90), "newer": _file(2004, 10)},
                    histories={
                        "larger": _history(1900, 90, last_seen=1900),
                        "newer": _history(1900, 10, last_seen=1900),
                    },
                    tolerance=5,
                ),
            )
            within_tolerance_is_tied = (
                _entry_kind(tolerance_tie) == "File"
                and _winning_source(tolerance_tie) == "larger"
                and _winning_mod_time(tolerance_tie) == 2000
                and _winning_byte_size(tolerance_tie) == 90
            )
            _print_result(
                "03.11",
                "live observation within tolerance of maximum is eligible as tied",
                within_tolerance_is_tied,
                failures,
                f"result={tolerance_tie!r}",
            )

            byte_size_tie_break = _decide(
                rpc,
                tool_name,
                _arguments(
                    roles={"small": "contributing", "large": "contributing"},
                    observations={"small": _file(3000, 10), "large": _file(3000, 80)},
                    histories={
                        "small": _history(2900, 10, last_seen=2900),
                        "large": _history(2900, 80, last_seen=2900),
                    },
                ),
            )
            larger_size_wins = (
                _entry_kind(byte_size_tie_break) == "File"
                and _winning_source(byte_size_tie_break) == "large"
                and _winning_mod_time(byte_size_tie_break) == 3000
                and _winning_byte_size(byte_size_tie_break) == 80
            )
            _print_result(
                "03.12",
                "larger byte_size wins when mod_time is tied",
                larger_size_wins,
                failures,
                f"result={byte_size_tie_break!r}",
            )

            no_opinion = _decide(
                rpc,
                tool_name,
                _arguments(
                    roles={"source": "contributing", "observer": "contributing"},
                    observations={"source": _file(4000, 44), "observer": _absent()},
                    histories={"source": _history(3900, 44, last_seen=3900)},
                ),
            )
            observer_action = _action(no_opinion, "observer")
            no_opinion_ignored = (
                _entry_kind(no_opinion) == "File"
                and _winning_source(no_opinion) == "source"
                and _classification(no_opinion, "observer") == "NoOpinion"
                and _kind(observer_action) == "ReceiveFile"
                and _source(observer_action) == "source"
            )
            _print_result(
                "03.15",
                "NoOpinion participant does not influence the voting outcome",
                no_opinion_ignored,
                failures,
                f"result={no_opinion!r}",
            )

            if failures:
                print("\nFAILURES:")
                for failure in failures:
                    print(f"  - {failure}")
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
