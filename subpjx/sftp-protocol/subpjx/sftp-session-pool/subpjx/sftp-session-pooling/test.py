#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Validate SFTP session pooling behavior through the MCP wrapper."""

from __future__ import annotations

import json
import os
import socket
import subprocess
import threading
import time
from pathlib import Path
from typing import Any, Dict, List, Optional, Sequence


BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")


def _drain(stream) -> None:
    for _ in stream:
        pass


def _launch_mcp() -> tuple[subprocess.Popen[str], int]:
    proc = subprocess.Popen(
        [str(UV), "run", "--script", str(BUILD_PY), "launch-mcp", PROJECT],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
    )

    port: Optional[int] = None
    deadline = time.time() + 30.0
    while time.time() < deadline and proc.poll() is None:
        line = proc.stdout.readline() if proc.stdout is not None else ""
        if not line:
            continue
        line = line.strip()
        if not line.startswith("MCP_PORT="):
            continue
        try:
            port = int(line.split("=", 1)[1])
        except ValueError:
            continue
        break

    if port is None:
        proc.terminate()
        raise RuntimeError("did not receive MCP_PORT=<n> from wrapper")

    threading.Thread(target=_drain, args=(proc.stdout,), daemon=True).start()
    threading.Thread(target=_drain, args=(proc.stderr,), daemon=True).start()
    return proc, port


class RpcClient:
    def __init__(self, sock: socket.socket):
        self.sock = sock
        self.request_id = 1

    def call(self, method: str, params: Optional[Dict[str, Any]]) -> Dict[str, Any]:
        payload = {"jsonrpc": "2.0", "id": self.request_id, "method": method}
        if params is not None:
            payload["params"] = params
        self.request_id += 1
        self.sock.sendall((json.dumps(payload) + "\n").encode("utf-8"))

        buffer = b""
        deadline = time.time() + 12.0
        while time.time() < deadline:
            self.sock.settimeout(1.0)
            try:
                chunk = self.sock.recv(8192)
            except TimeoutError:
                continue
            if not chunk:
                raise RuntimeError("connection closed while waiting for MCP response")
            buffer += chunk
            while b"\n" in buffer:
                line, _, rest = buffer.partition(b"\n")
                buffer = rest
                line = line.strip()
                if not line:
                    continue
                return json.loads(line.decode("utf-8"))
        raise TimeoutError("timed out waiting for MCP response")


def _tool_list_failure(msg: str, failures: List[str], detail: Any) -> str:
    return f"{msg}: {detail}"


def _is_error(response: Dict[str, Any]) -> bool:
    return isinstance(response, dict) and response.get("error") is not None


def _error_text(response: Dict[str, Any]) -> str:
    if not isinstance(response, dict):
        return ""
    error = response.get("error")
    if isinstance(error, dict):
        return str(error.get("message", ""))
    if error is None:
        return ""
    return str(error)


def _error_category(response: Dict[str, Any]) -> Optional[str]:
    text = _error_text(response).lower()
    if "io_error" in text or "io error" in text:
        return "io_error"
    if "not implemented" in text:
        return "not_implemented"
    if "invalid argument" in text:
        return "invalid_argument"
    return None


def _normalize(s: str) -> str:
    return "".join(ch if ch.isalnum() else " " for ch in s.lower())


def _find_tool(tools: List[Dict[str, Any]], candidates: Sequence[Sequence[str]]) -> Optional[Dict[str, Any]]:
    if not isinstance(tools, list):
        return None

    # exact name first
    for tool in tools:
        name = str(tool.get("name", ""))
        for candidate in candidates:
            for term in candidate:
                if name == term:
                    return tool

    best = None
    best_score = -1
    for tool in tools:
        haystack = _normalize(str(tool.get("name", "")) + " " + str(tool.get("description", "")))
        tokens = haystack.split()
        score = 0
        for candidate in candidates:
            hit = 0
            for term in candidate:
                if term in tokens:
                    hit += 1
            score += hit
        if score > best_score and score > 0:
            best_score = score
            best = tool
    return best


def _read_env(*names: str, default: Optional[str] = None) -> Optional[str]:
    for name in names:
        value = os.environ.get(name)
        if value:
            return value
    return default


def _read_int_env(names: Sequence[str], default: int) -> int:
    for name in names:
        raw = os.environ.get(name)
        if raw is None:
            continue
        try:
            return int(raw)
        except ValueError:
            continue
    return default


def _peer(root_suffix: str = "") -> Dict[str, Any]:
    root_path = _read_env("AITC_SFTP_ROOT", "SFTP_ROOT", default="/tmp/aitc")
    if root_suffix:
        root_path = f"{root_path.rstrip('/')}/{root_suffix}".rstrip("/")
    return {
        "user": _read_env("AITC_SFTP_USER", "SFTP_USER", default="aitc"),
        "password": _read_env("AITC_SFTP_PASSWORD", "SFTP_PASSWORD", default=""),
        "host": _read_env("AITC_SFTP_HOST", "SFTP_HOST", default="127.0.0.1"),
        "port": _read_int_env(["AITC_SFTP_PORT", "SFTP_PORT"], 22),
        "root_path": root_path,
    }


def _settings(max_open_connections: int = 2, idle_keep_alive_seconds: int = 30) -> Dict[str, Any]:
    return {
        "max_open_connections": max_open_connections,
        "idle_keep_alive_seconds": idle_keep_alive_seconds,
    }


def _value_for_field(name: str, schema: Dict[str, Any], overrides: Dict[str, Any]) -> Any:
    lowered = name.lower()
    if "peer" in lowered:
        return _peer("peer")
    if "settings" in lowered:
        return _settings()
    if "session" in lowered or "handle" in lowered:
        return overrides.get(name, "session-token")

    schema_type = schema.get("type")
    if "enum" in schema and isinstance(schema["enum"], list) and schema["enum"]:
        return schema["enum"][0]

    if schema_type == "integer":
        if "port" in lowered or "max" in lowered or "seconds" in lowered:
            return 30
        return 1
    if schema_type == "number":
        return 1.0
    if schema_type == "boolean":
        return False
    if schema_type == "array":
        items = schema.get("items", {})
        if isinstance(items, dict):
            return [_value_for_field(f"{name}_item", items, {})]
        return []
    if schema_type == "object":
        props = schema.get("properties", {})
        if isinstance(props, dict):
            result: Dict[str, Any] = {}
            for child_name, child_schema in props.items():
                if isinstance(child_name, str):
                    result[child_name] = _value_for_field(child_name, child_schema if isinstance(child_schema, dict) else {}, {})
            return result
        return {}

    if "password" in lowered:
        return _read_env("AITC_SFTP_PASSWORD", "SFTP_PASSWORD", default="")
    if "host" in lowered:
        return _read_env("AITC_SFTP_HOST", "SFTP_HOST", default="127.0.0.1")
    if "user" in lowered:
        return _read_env("AITC_SFTP_USER", "SFTP_USER", default="aitc")
    if "port" in lowered:
        return _read_int_env(["AITC_SFTP_PORT", "SFTP_PORT"], 22)
    if "path" in lowered:
        return _read_env("AITC_SFTP_ROOT", "SFTP_ROOT", default="/tmp/aitc")
    if schema_type == "string" or schema_type is None:
        return ""
    return None


def _build_arguments(tool: Optional[Dict[str, Any]], overrides: Optional[Dict[str, Any]]) -> Dict[str, Any]:
    if not isinstance(tool, dict):
        return {}

    schema = tool.get("inputSchema")
    if not isinstance(schema, dict):
        schema = {}
    properties = schema.get("properties")
    if not isinstance(properties, dict):
        properties = {}
    required = schema.get("required", [])
    if not isinstance(required, list):
        required = []

    args: Dict[str, Any] = {}
    if overrides:
        args.update(overrides)

    for field_name in required:
        if field_name in args:
            continue
        child_schema = properties.get(field_name, {})
        if not isinstance(child_schema, dict):
            child_schema = {}
        args[field_name] = _value_for_field(field_name, child_schema, args)

    return args


def _extract_session(response: Dict[str, Any]) -> Any:
    result = response.get("result")
    if not isinstance(result, dict):
        return result
    if "session" in result:
        return result["session"]
    if "value" in result:
        return result["value"]
    return result


def _extract_pool_key(peer: Dict[str, Any]) -> str:
    user = str(peer.get("user", "")).strip()
    host = str(peer.get("host", "")).strip()
    if not user:
        return host
    return f"{user}@{host}"


def _as_event_list(payload: Any) -> List[Dict[str, Any]]:
    if payload is None:
        return []
    if isinstance(payload, dict):
        if "events" in payload and isinstance(payload["events"], list):
            return [ev for ev in payload["events"] if isinstance(ev, dict)]
        if "value" in payload and isinstance(payload["value"], list):
            return [ev for ev in payload["value"] if isinstance(ev, dict)]
        if "endpoint" in payload and ("connections" in payload or "max" in payload):
            return [payload]
        return []
    if isinstance(payload, list):
        return [ev for ev in payload if isinstance(ev, dict)]
    return []


def _session_id(session: Any) -> str:
    if isinstance(session, dict):
        for key in ("id", "handle", "key"):
            if key in session:
                return str(session[key])
    return str(session) if session is not None else "<none>"


def _is_pool_event(event: Dict[str, Any], expected_endpoint: str) -> bool:
    if not isinstance(event, dict):
        return False
    if "endpoint" not in event:
        return False
    endpoint = str(event.get("endpoint", ""))
    if endpoint != expected_endpoint:
        return False
    if "connections" not in event and "max" not in event:
        return False
    return True


def _call_tool(sock: socket.socket, tool: Dict[str, Any], args: Dict[str, Any], failures: List[str], label: str) -> Optional[Dict[str, Any]]:
    client = RpcClient(sock)
    response = client.call("tools/call", {"name": str(tool["name"]), "arguments": args})
    if _is_error(response):
        failures.append(f"{label}: call failed with {_error_category(response) or _error_text(response)}")
        return None
    return response


def _call_tool_once(port: int, tool: Dict[str, Any], args: Dict[str, Any]) -> Dict[str, Any]:
    with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
        client = RpcClient(s)
        return client.call("tools/call", {"name": str(tool["name"]), "arguments": args})


def _acquire_async(port: int, tool: Dict[str, Any], args: Dict[str, Any], out: Dict[str, Any]) -> None:
    try:
        out["response"] = _call_tool_once(port, tool, args)
    except Exception as exc:
        out["exception"] = str(exc)


def main() -> int:
    failures: List[str] = []
    proc: Optional[subprocess.Popen[str]] = None

    try:
        proc, port = _launch_mcp()
    except Exception as exc:
        failures.append(f"00: unable to launch MCP wrapper: {exc}")
        print("FAILURES:")
        for fail in failures:
            print(f"- {fail}")
        return 1

    try:
        try:
            sock = socket.create_connection(("127.0.0.1", port), timeout=10.0)
        except Exception as exc:
            failures.append(f"01: unable to connect to MCP server on 127.0.0.1:{port}: {exc}")
            sock = None

        if sock is None:
            print("FAILURES:")
            for fail in failures:
                print(f"- {fail}")
            return 1

        try:
            client = RpcClient(sock)
            list_response = client.call("tools/list", None)
            if _is_error(list_response):
                failures.append(f"01: tools/list returned protocol/tool error: {_error_text(list_response)}")
                tools: List[Dict[str, Any]] = []
            else:
                tools = list_response.get("result", {}).get("tools", [])
                if not isinstance(tools, list) or not tools:
                    failures.append("01: tools/list did not return a non-empty tools list")

            acquire_tool = _find_tool(
                tools,
                (("acquire",), ("borrow",), ("session",)),
            )
            release_tool = _find_tool(
                tools,
                (("release",), ("return",), ("session",)),
            )
            event_tool = _find_tool(
                tools,
                (("on", "pool", "event"), ("pool", "event"), ("on", "event")),
            )

            if acquire_tool is None:
                failures.append("02: acquire tool not found in tools/list")
            if release_tool is None:
                failures.append("03: release tool not found in tools/list")
            if event_tool is None:
                failures.append("04: on-pool-event tool not found in tools/list")

            if acquire_tool is None or release_tool is None:
                failures.append("05: cannot exercise pooling behavior without acquire and release tools")
            else:
                base_peer = _peer("primary")
                alt_peer = _peer("alt-root")
                alt_peer["root_path"] = alt_peer.get("root_path", "/tmp/aitc") + "-other"
                pool_key = _extract_pool_key(base_peer)

                pool_args = _build_arguments(
                    acquire_tool,
                    {
                        "peer": base_peer,
                        "settings": _settings(2, 30),
                    },
                )
                acquire_resp = _call_tool(sock, acquire_tool, pool_args, failures, "06")
                session1 = _extract_session(acquire_resp or {})
                if session1 is None:
                    failures.append("06: acquire returned no session value")

                # verify root_path does not split pool identity
                if session1 is not None:
                    wait_args = _build_arguments(acquire_tool, {"peer": alt_peer, "settings": _settings(1, 30)})
                    pending: Dict[str, Any] = {}
                    acquire_thread = threading.Thread(
                        target=_acquire_async,
                        args=(port, acquire_tool, wait_args, pending),
                        daemon=True,
                    )
                    acquire_thread.start()
                    acquire_thread.join(1.0)
                    if not acquire_thread.is_alive():
                        failures.append(
                            "07: acquire with same PoolKey and different root_path returned before release; root_path may be incorrectly part of pooling key"
                        )

                    if event_tool is not None:
                        try:
                            before_events = _as_event_list(_call_tool_once(port, event_tool, _build_arguments(event_tool, {})).get("result"))
                        except Exception as exc:
                            before_events = []
                            failures.append(f"04: failed to call on_pool_event before occupancy checks: {exc}")

                    release_args = _build_arguments(release_tool, {"session": session1})
                    _call_tool(sock, release_tool, release_args, failures, "08")

                    acquire_thread.join(8.0)
                    if acquire_thread.is_alive():
                        failures.append("08: acquire did not unblock after release when pool was at max connection limit")
                    elif pending.get("exception") is not None:
                        failures.append(f"08: async acquire failed: {pending['exception']}")
                    elif not isinstance(pending.get("response"), dict) or _is_error(pending.get("response", {})):
                        async_resp = pending.get("response") or {}
                        failures.append(
                            f"08: async acquire after release returned error: {_error_category(async_resp) or _error_text(async_resp) or 'missing response'}"
                        )
                    else:
                        session2 = _extract_session(pending["response"])
                        if session2 is None:
                            failures.append("08: async acquire returned no session after release")
                        else:
                            if _session_id(session1) == _session_id(session2):
                                # same object can be reused across waits; this is a valid implementation detail
                                pass
                            if event_tool is not None and session2 is not None:
                                try:
                                    after_events = _as_event_list(
                                        _call_tool_once(port, event_tool, _build_arguments(event_tool, {})).get("result")
                                    )
                                    gained_events = after_events[len(before_events):]
                                    if not any(_is_pool_event(event, pool_key) for event in gained_events):
                                        failures.append(
                                            "09: on_pool_event output did not include PoolEvent for acquire/release on this PoolKey"
                                        )
                                except Exception as exc:
                                    failures.append(f"09: failed to validate on_pool_event after acquire/release: {exc}")

                            # keep session open to test release always works
                            rel2_args = _build_arguments(release_tool, {"session": session2})
                            _call_tool(sock, release_tool, rel2_args, failures, "10")

                # verify failure path is reported as io_error when opening new session fails
                bad_peer = {
                    **_peer("primary"),
                    "host": "127.0.0.1",
                    "port": 1,
                }
                bad_args = _build_arguments(acquire_tool, {"peer": bad_peer, "settings": _settings(1, 5)})
                bad_resp = _call_tool(sock, acquire_tool, bad_args, failures, "11")
                if bad_resp is not None:
                    failures.append("11: acquire against a known unreachable port unexpectedly succeeded")
                else:
                    # _call_tool records no session error if present, but we still inspect it directly
                    bad_resp = None
                    try:
                        bad_resp = _call_tool_once(port, acquire_tool, bad_args)
                    except Exception as exc:
                        failures.append(f"11: acquire failure call could not be observed: {exc}")
                    if bad_resp is None or not _is_error(bad_resp):
                        failures.append("11: acquire failure case did not return a JSON-RPC error")
                    elif _error_category(bad_resp) != "io_error":
                        failures.append(
                            f"11: acquire failure error category expected io_error, got {_error_category(bad_resp) or _error_text(bad_resp)}"
                        )

                # idle keep-alive behavior: release + immediate reacquire
                quick_peer = _peer("primary")
                keep_args = _build_arguments(acquire_tool, {"peer": quick_peer, "settings": _settings(1, 2)})
                quick_acq = _call_tool(sock, acquire_tool, keep_args, failures, "12")
                quick_session = _extract_session(quick_acq or {})
                if quick_session is not None:
                    quick_rel = _build_arguments(release_tool, {"session": quick_session})
                    _call_tool(sock, release_tool, quick_rel, failures, "12")
                    time.sleep(0.5)
                    quick_acq2 = _call_tool(sock, acquire_tool, keep_args, failures, "12")
                    quick_session2 = _extract_session(quick_acq2 or {})
                    if quick_session2 is None:
                        failures.append("12: quick reacquire after release did not return a session")
                    else:
                        _call_tool(sock, release_tool, _build_arguments(release_tool, {"session": quick_session2}), failures, "12")

        except TimeoutError as exc:
            failures.append(f"99: MCP interaction timed out: {exc}")
        except Exception as exc:
            failures.append(f"99: unexpected MCP interaction error: {exc}")
        finally:
            if sock is not None:
                try:
                    sock.close()
                except Exception:
                    pass
    finally:
        if proc is not None:
            if proc.poll() is None:
                proc.terminate()
                try:
                    proc.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    proc.kill()
                    proc.wait()

    if failures:
        print("FAILURES:")
        for fail in failures:
            print(f"- {fail}")
        return 1

    print("All checks passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
