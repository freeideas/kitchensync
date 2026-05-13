#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercise canon participant override decisions through the MCP wrapper."""

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


def _drain(stream: Any) -> None:
    for _ in stream:
        pass


def _launch() -> tuple[subprocess.Popen[str], int]:
    proc = subprocess.Popen(
        [str(UV), "run", "--script", str(BUILD_PY), "launch-mcp", PROJECT],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
    )
    if proc.stdout is None:
        proc.terminate()
        raise RuntimeError("MCP server stdout was not captured")

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
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
        raise RuntimeError("MCP server did not advertise MCP_PORT")

    threading.Thread(target=_drain, args=(proc.stdout,), daemon=True).start()
    if proc.stderr is not None:
        threading.Thread(target=_drain, args=(proc.stderr,), daemon=True).start()
    return proc, port


def _rpc(sock: socket.socket, method: str, params: dict[str, Any] | None = None, rpc_id: int = 1) -> dict[str, Any]:
    msg: dict[str, Any] = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
    if params is not None:
        msg["params"] = params
    sock.sendall((json.dumps(msg, separators=(",", ":")) + "\n").encode("utf-8"))

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
    if not line:
        raise RuntimeError(f"no JSON-RPC response for {method}")
    return json.loads(line.decode("utf-8"))


def _call_decide(sock: socket.socket, rpc_id: int, arguments: dict[str, Any]) -> dict[str, Any]:
    response = _rpc(
        sock,
        "tools/call",
        {"name": "decide-entry", "arguments": arguments},
        rpc_id,
    )
    if "error" in response:
        raise RuntimeError(f"decide-entry failed: {response['error']}")
    result = response.get("result")
    if not isinstance(result, dict):
        raise RuntimeError(f"decide-entry returned non-object result: {result!r}")
    return result


def _decide_error(sock: socket.socket, rpc_id: int, arguments: dict[str, Any]) -> str | None:
    response = _rpc(
        sock,
        "tools/call",
        {"name": "decide-entry", "arguments": arguments},
        rpc_id,
    )
    error = response.get("error")
    if not isinstance(error, dict):
        return None
    message = error.get("message")
    return message if isinstance(message, str) else ""


def _file(mod_time: int, byte_size: int) -> dict[str, Any]:
    return {"kind": "File", "mod_time": mod_time, "byte_size": byte_size}


def _directory() -> dict[str, Any]:
    return {"kind": "Directory"}


def _absent() -> dict[str, Any]:
    return {"kind": "Absent"}


def _action_kind(decision: dict[str, Any], participant: str) -> str | None:
    actions = decision.get("actions")
    if not isinstance(actions, dict):
        return None
    action = actions.get(participant)
    if isinstance(action, str):
        return action
    if isinstance(action, dict):
        kind = action.get("kind") or action.get("action")
        if isinstance(kind, str):
            return kind
    return None


def _action_source(decision: dict[str, Any], participant: str) -> str | None:
    actions = decision.get("actions")
    if not isinstance(actions, dict):
        return None
    action = actions.get(participant)
    if isinstance(action, dict):
        source = action.get("source")
        if isinstance(source, str):
            return source
    return None


def _ok(failures: list[str], req_id: str, passed: bool, detail: str) -> None:
    status = "PASS" if passed else "FAIL"
    print(f"[{req_id}] {status} {detail}")
    if not passed:
        failures.append(f"{req_id}: {detail}")


def _base_args(observations: dict[str, dict[str, Any]]) -> dict[str, Any]:
    return {
        "roles": {participant: "contributing" for participant in observations},
        "observations": observations,
        "histories": {},
    }


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as sock:
            _rpc(sock, "tools/list", rpc_id=1)
            failures: list[str] = []

            try:
                duplicate_canon = _base_args(
                    {
                        "first": _file(100, 10),
                        "second": _file(200, 20),
                    }
                )
                duplicate_canon["roles"]["first"] = "canon"
                duplicate_canon["roles"]["second"] = "canon"
                duplicate_error = _decide_error(sock, 5, duplicate_canon)
            except Exception as exc:
                duplicate_error = None
                failures.append(f"duplicate canon scenario failed: {exc}")

            _ok(
                failures,
                "03.0",
                duplicate_error is not None,
                "more than one canon participant is rejected",
            )

            try:
                canon_file = _base_args(
                    {
                        "canon": _file(100, 10),
                        "newer": _file(500, 99),
                        "missing": _absent(),
                        "directory": _directory(),
                        "matching": _file(103, 10),
                    }
                )
                canon_file["roles"]["canon"] = "canon"
                file_decision = _call_decide(sock, 2, canon_file)
            except Exception as exc:
                file_decision = {}
                failures.append(f"canon file scenario failed: {exc}")

            _ok(
                failures,
                "03.1",
                file_decision.get("entry_kind") == "File",
                "canon File observation sets entry_kind to File despite newer file and directory observations",
            )
            _ok(
                failures,
                "03.2",
                file_decision.get("winning_source") == "canon",
                "canon File observation sets winning_source to the canon participant",
            )
            _ok(
                failures,
                "03.3",
                _action_kind(file_decision, "newer") == "ReceiveFile"
                and _action_source(file_decision, "newer") == "canon"
                and _action_kind(file_decision, "missing") == "ReceiveFile"
                and _action_source(file_decision, "missing") == "canon"
                and _action_kind(file_decision, "directory") == "ReceiveFile"
                and _action_source(file_decision, "directory") == "canon"
                and _action_kind(file_decision, "matching") == "NoOp",
                "nonmatching participants receive the canon file while an already matching file is left alone",
            )

            try:
                canon_directory = _base_args(
                    {
                        "canon": _directory(),
                        "missing": _absent(),
                        "file": _file(900, 77),
                        "other_directory": _directory(),
                    }
                )
                canon_directory["roles"]["canon"] = "canon"
                directory_decision = _call_decide(sock, 3, canon_directory)
            except Exception as exc:
                directory_decision = {}
                failures.append(f"canon directory scenario failed: {exc}")

            _ok(
                failures,
                "03.4",
                directory_decision.get("entry_kind") == "Directory",
                "canon Directory observation sets entry_kind to Directory despite file observations",
            )
            _ok(
                failures,
                "03.5",
                _action_kind(directory_decision, "missing") == "CreateDirectory",
                "participant lacking the canon directory gets CreateDirectory",
            )
            _ok(
                failures,
                "03.6",
                _action_kind(directory_decision, "file") == "Displace",
                "participant observing a file at the canon directory name gets Displace",
            )

            try:
                canon_absent = _base_args(
                    {
                        "canon": _absent(),
                        "file": _file(900, 77),
                        "directory": _directory(),
                        "missing": _absent(),
                    }
                )
                canon_absent["roles"]["canon"] = "canon"
                absent_decision = _call_decide(sock, 4, canon_absent)
            except Exception as exc:
                absent_decision = {}
                failures.append(f"canon absent scenario failed: {exc}")

            _ok(
                failures,
                "03.7",
                absent_decision.get("entry_kind") == "None",
                "canon Absent observation sets entry_kind to None despite live observations",
            )
            _ok(
                failures,
                "03.8",
                _action_kind(absent_decision, "file") == "Displace"
                and _action_kind(absent_decision, "directory") == "Displace"
                and _action_kind(absent_decision, "missing") == "NoOp",
                "participants holding entries are displaced when canon observes Absent",
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
