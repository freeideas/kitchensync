#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Verify MCP read-stream behavior from the published contract."""

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
        raise RuntimeError("did not receive MCP_PORT=<n> from launch-mcp")

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
            except OSError as exc:
                raise RuntimeError(f"MCP socket error: {exc}")
            if not chunk:
                raise RuntimeError("MCP connection closed while waiting for response")
            buffer += chunk
            if b"\n" in buffer:
                line, _, _ = buffer.partition(b"\n")
                line = line.strip()
                if not line:
                    continue
                return json.loads(line.decode("utf-8"))
        raise TimeoutError("timed out waiting for MCP response")


def _env(*names: str, default: Optional[str] = None) -> Optional[str]:
    for name in names:
        value = os.environ.get(name)
        if value is not None and value != "":
            return value
    return default


def _to_int(raw: Optional[str], fallback: int) -> int:
    try:
        return int(raw) if raw is not None else fallback
    except ValueError:
        return fallback


def _tokenize(value: str) -> List[str]:
    return [token for token in "".join(ch.lower() if ch.isalnum() else " " for ch in value).split() if token]


def _compact(value: str) -> str:
    return "".join(ch.lower() for ch in value if ch.isalnum())


def _find_tool(tools: Any, candidates: Sequence[str], exclude: Sequence[str] = ()) -> Optional[Dict[str, Any]]:
    if not isinstance(tools, list):
        return None

    excluded = {term for term in _tokenize(" ".join(exclude))}
    exact = {_compact(candidate) for candidate in candidates}

    for tool in tools:
        name = str(tool.get("name", ""))
        if _compact(name) in exact:
            return tool

    for tool in tools:
        name = str(tool.get("name", ""))
        description = str(tool.get("description", ""))
        haystack = _tokenize(f"{name} {description}")
        if excluded and any(term in haystack for term in excluded):
            continue
        for candidate in candidates:
            terms = set(_tokenize(candidate))
            if terms and all(term in haystack for term in terms):
                return tool
    return None


def _is_error(response: Dict[str, Any]) -> bool:
    return isinstance(response, dict) and response.get("error") is not None


def _error_text(response: Dict[str, Any]) -> str:
    error = response.get("error")
    if error is None:
        return ""
    if isinstance(error, dict):
        parts = [str(error.get("message", "")), str(error.get("data", ""))]
        return " ".join(part for part in parts if part)
    return str(error)


def _error_category(response: Dict[str, Any]) -> str | None:
    text = _error_text(response).lower()
    for category in ("not_found", "permission_denied", "io_error"):
        if category in text:
            return category
    return None


def _parse_json_text(value: Any) -> Any:
    if not isinstance(value, str):
        return value
    text = value.strip()
    if not text:
        return ""
    if (text.startswith("{") and text.endswith("}")) or (text.startswith("[") and text.endswith("]")):
        try:
            return json.loads(text)
        except json.JSONDecodeError:
            return value
    return value


def _extract_result(response: Any) -> Any:
    if not isinstance(response, dict):
        return response

    result = response.get("result", response)
    if isinstance(result, dict):
        content = result.get("content")
        if isinstance(content, list):
            for item in content:
                if not isinstance(item, dict):
                    continue
                if "json" in item:
                    return _parse_json_text(item.get("json"))
                if "text" in item:
                    return _parse_json_text(item.get("text"))

        for key in ("session", "handle", "value", "result"):
            if key in result:
                return result[key]
    return result


def _extract_session(response: Any) -> Any:
    value = _extract_result(response)
    if isinstance(value, dict) and "session" in value:
        return value["session"]
    if isinstance(value, dict) and "value" in value:
        return value["value"]
    return value


def _extract_handle(response: Any) -> Any:
    value = _extract_result(response)
    if isinstance(value, dict):
        for key in ("handle", "value", "result"):
            if key in value:
                return value[key]
    return value


def _looks_like_eof(payload: Any) -> bool:
    if isinstance(payload, str):
        return payload.strip().upper() == "EOF"
    if payload is None:
        return False
    if isinstance(payload, dict):
        if any(key in payload and bool(payload[key]) for key in ("eof", "end_of_file", "at_eof")):
            return True
    return False


def _extract_read_bytes(payload: Any) -> Optional[bytes]:
    if payload is None:
        return b""
    if _looks_like_eof(payload):
        return b""
    if isinstance(payload, bytes):
        return payload
    if isinstance(payload, str):
        return payload.encode("utf-8")
    if isinstance(payload, list) and all(isinstance(item, int) for item in payload):
        return bytes(payload)
    if isinstance(payload, dict):
        for key in ("bytes", "data", "chunk", "value"):
            if key in payload:
                value = _extract_read_bytes(payload[key])
                if value is not None:
                    return value
        if "content" in payload and isinstance(payload["content"], list):
            parts: List[str] = []
            for item in payload["content"]:
                if isinstance(item, str):
                    parts.append(item)
                elif isinstance(item, dict) and "text" in item:
                    parts.append(str(item["text"]))
            if parts:
                return "".join(parts).encode("utf-8")
    return None


def _default_peer() -> Dict[str, Any]:
    return {
        "user": _env("AITC_SFTP_USER", "SFTP_USER", default="aitc"),
        "password": _env("AITC_SFTP_PASSWORD", "SFTP_PASSWORD", default=""),
        "host": _env("AITC_SFTP_HOST", "SFTP_HOST", default="127.0.0.1"),
        "port": _to_int(_env("AITC_SFTP_PORT", "SFTP_PORT", default="22"), 22),
        "root_path": _env("AITC_SFTP_ROOT", "SFTP_ROOT", default="/tmp/aitc"),
    }


def _value_for_field(name: str, schema: Dict[str, Any], overrides: Dict[str, Any]) -> Any:
    lowered = name.lower()

    if "enum" in schema:
        choices = schema.get("enum")
        if isinstance(choices, list) and choices:
            return choices[0]

    schema_type = schema.get("type")
    if "peer" in lowered:
        return _default_peer()
    if "settings" in lowered or "poolsettings" in lowered:
        return {"connection_timeout_seconds": 30}

    if schema_type == "object":
        properties = schema.get("properties", {})
        if not isinstance(properties, dict):
            return {}
        required = schema.get("required", [])
        if not isinstance(required, list):
            required = []
        obj: Dict[str, Any] = {}
        for key in required:
            if key in properties and isinstance(properties[key], dict):
                obj[key] = _value_for_field(key, properties[key], overrides)
        if not obj and properties:
            for key, child_schema in properties.items():
                if isinstance(key, str) and isinstance(child_schema, dict):
                    obj[key] = _value_for_field(key, child_schema, overrides)
        return obj

    if schema_type == "array":
        items = schema.get("items")
        if isinstance(items, dict):
            return [_value_for_field(f"{name}_item", items, overrides)]
        return []

    if schema_type in {"integer", "number"}:
        if "max" in lowered or "bytes" in lowered or "seconds" in lowered:
            return 64
        if "port" in lowered:
            return 22
        return 1

    if schema_type == "boolean":
        return False

    if schema_type in (None, "string"):
        if "path" in lowered:
            return _env("AITC_SFTP_MISSING_PATH", "SFTP_MISSING_PATH", default="__aitc_missing_file__.txt")
        if "user" in lowered:
            return _default_peer()["user"]
        if "password" in lowered:
            return _default_peer()["password"]
        if "host" in lowered:
            return _default_peer()["host"]
        if "handle" in lowered:
            return "read-stream-handle"
        return "value"

    return "value"


def _build_arguments(tool: Optional[Dict[str, Any]], overrides: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
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
    if isinstance(overrides, dict):
        args.update(overrides)

    for field in required:
        if field in args:
            continue
        child_schema = properties.get(field, {})
        if not isinstance(child_schema, dict):
            child_schema = {}
        args[field] = _value_for_field(field, child_schema, args)
    return args


def _to_optional_path() -> Optional[str]:
    return _env(
        "SFTP_READ_TEST_FILE",
        "AITC_SFTP_READ_FILE",
        "SFTP_TEST_FILE",
        default=None,
    )


def _build_read_path_via_writer(
    client: RpcClient,
    session: Any,
    open_write_tool: Optional[Dict[str, Any]],
    write_tool: Optional[Dict[str, Any]],
    close_write_tool: Optional[Dict[str, Any]],
    failures: List[str],
) -> Optional[str]:
    if open_write_tool is None or write_tool is None or close_write_tool is None:
        failures.append(
            "09: no read fixture path configured and no write tools are available to prepare one"
        )
        return None

    path = _env(
        "AITC_SFTP_READ_STREAM_FIXTURE",
        "SFTP_READ_STREAM_FIXTURE",
        default="artifacts/read_streams_fixture.txt",
    )

    open_args = _build_arguments(open_write_tool, {"session": session, "path": path})
    open_resp = client.call(
        "tools/call",
        {"name": str(open_write_tool["name"]), "arguments": open_args},
    )
    if _is_error(open_resp):
        failures.append(
            f"10: open_write fallback failed while creating fixture: {_error_category(open_resp) or _error_text(open_resp)}"
        )
        return None

    handle = _extract_handle(open_resp)
    if handle is None:
        failures.append("10: open_write fallback returned no handle")
        return None

    write_args = _build_arguments(
        write_tool,
        {
            "session": session,
            "handle": handle,
            "bytes": "read-streams smoke test",
        },
    )
    write_resp = client.call(
        "tools/call",
        {"name": str(write_tool["name"]), "arguments": write_args},
    )
    if _is_error(write_resp):
        failures.append(
            f"10: write fallback failed while creating fixture: {_error_category(write_resp) or _error_text(write_resp)}"
        )
        return None

    close_args = _build_arguments(close_write_tool, {"session": session, "handle": handle})
    close_resp = client.call(
        "tools/call",
        {"name": str(close_write_tool["name"]), "arguments": close_args},
    )
    if _is_error(close_resp):
        failures.append(
            f"10: close_write fallback failed while creating fixture: {_error_category(close_resp) or _error_text(close_resp)}"
        )
        return None

    return path


def _check_error_category(response: Dict[str, Any], expected: str, label: str, failures: List[str]) -> None:
    if not _is_error(response):
        failures.append(f"{label}: expected error but call succeeded")
        return
    category = _error_category(response)
    if category != expected:
        failures.append(
            f"{label}: expected {expected} error, got {category or _error_text(response)}"
        )


def main() -> int:
    failures: List[str] = []
    proc: Optional[subprocess.Popen[str]] = None

    try:
        proc, port = _launch_mcp()
    except Exception as exc:
        failures.append(f"00: unable to launch MCP wrapper: {exc}")
        print("FAILURES:")
        for failure in failures:
            print(f"- {failure}")
        return 1

    try:
        sock: Optional[socket.socket] = None
        try:
            sock = socket.create_connection(("127.0.0.1", port), timeout=10.0)
            client = RpcClient(sock)

            list_resp = client.call("tools/list", None)
            if _is_error(list_resp):
                failures.append(f"01: tools/list returned protocol error: {_error_text(list_resp)}")
                tools: List[Dict[str, Any]] = []
            else:
                tools = list_resp.get("result", {}).get("tools", [])
                if not isinstance(tools, list) or not tools:
                    failures.append("01: tools/list returned no tools")
                    tools = []

            open_read_tool = _find_tool(tools, ("open_read", "open-read", "openread"))
            read_tool = _find_tool(tools, ("read",), exclude=("open", "close"))
            close_read_tool = _find_tool(tools, ("close_read", "close-read", "closeread"),)
            session_tool = _find_tool(tools, ("connect_listing", "connect-listing", "connect listing", "acquire", "connect"))
            write_open_tool = _find_tool(tools, ("open_write", "open-write", "openwrite"))
            write_tool = _find_tool(tools, ("write",), exclude=("open",))
            close_write_tool = _find_tool(tools, ("close_write", "close-write", "closewrite"))

            if open_read_tool is None:
                failures.append("02: missing open_read tool")
            if read_tool is None:
                failures.append("03: missing read tool")
            if close_read_tool is None:
                failures.append("04: missing close_read tool")
            if session_tool is None:
                failures.append("05: missing session bootstrap tool (acquire/connect_listing/connect)")

            session = None
            if session_tool is not None:
                session_resp = client.call(
                    "tools/call",
                    {"name": str(session_tool["name"]), "arguments": _build_arguments(session_tool, {})},
                )
                if _is_error(session_resp):
                    failures.append(
                        f"06: session bootstrap call failed: {_error_category(session_resp) or _error_text(session_resp)}"
                    )
                else:
                    session = _extract_session(session_resp)
                    if session is None:
                        failures.append("06: session bootstrap response had no session value")

            if session is None:
                failures.append("07: cannot run read-stream checks without a session")
            else:
                missing_path = _env(
                    "SFTP_MISSING_PATH",
                    "AITC_SFTP_MISSING_PATH",
                    default="__aitc_missing_file__.txt",
                )
                if open_read_tool is None:
                    failures.append("08: cannot validate open_read not_found without open_read tool")
                else:
                    open_missing_resp = client.call(
                        "tools/call",
                        {
                            "name": str(open_read_tool["name"]),
                            "arguments": _build_arguments(open_read_tool, {"session": session, "path": missing_path}),
                        },
                    )
                    _check_error_category(open_missing_resp, "not_found", "08", failures)

                path = _to_optional_path()
                if path is None:
                    path = _build_read_path_via_writer(
                        client,
                        session,
                        write_open_tool,
                        write_tool,
                        close_write_tool,
                        failures,
                    )

                if path is None:
                    failures.append("09: no fixture path to exercise open_read/read/close_read")
                elif open_read_tool is None:
                    failures.append("09: cannot exercise happy-path read without open_read tool")
                elif read_tool is None or close_read_tool is None:
                    failures.append("09: cannot exercise happy-path read without read and close_read tools")
                else:
                    read_open_resp = client.call(
                        "tools/call",
                        {
                            "name": str(open_read_tool["name"]),
                            "arguments": _build_arguments(open_read_tool, {"session": session, "path": path}),
                        },
                    )
                    if _is_error(read_open_resp):
                        failures.append(
                            f"09: open_read for fixture path failed: {_error_category(read_open_resp) or _error_text(read_open_resp)}"
                        )
                    else:
                        handle = _extract_handle(read_open_resp)
                        if handle is None:
                            failures.append("09: open_read fixture call returned no handle")
                        else:
                            read_max = 64
                            reads = 0
                            reached_eof = False
                            while reads < 64:
                                reads += 1
                                read_resp = client.call(
                                    "tools/call",
                                    {
                                        "name": str(read_tool["name"]),
                                        "arguments": _build_arguments(
                                            read_tool,
                                            {
                                                "session": session,
                                                "handle": handle,
                                                "max_bytes": read_max,
                                            },
                                        ),
                                    },
                                )
                                if _is_error(read_resp):
                                    failures.append(
                                        f"10: read call {reads} failed: {_error_category(read_resp) or _error_text(read_resp)}"
                                    )
                                    break

                                payload = _extract_result(read_resp)
                                if _looks_like_eof(payload):
                                    reached_eof = True
                                    break

                                chunk = _extract_read_bytes(payload)
                                if chunk is None:
                                    failures.append(f"10: read call {reads} returned unparseable payload: {payload}")
                                    break
                                if len(chunk) > read_max:
                                    failures.append(
                                        f"10: read call {reads} returned {len(chunk)} bytes, above requested max_bytes={read_max}"
                                    )
                                if len(chunk) == 0:
                                    reached_eof = True
                                    break

                            if not reached_eof:
                                failures.append("10: read did not report EOF within 64 reads")

                            close_resp = client.call(
                                "tools/call",
                                {
                                    "name": str(close_read_tool["name"]),
                                    "arguments": _build_arguments(
                                        close_read_tool,
                                        {
                                            "session": session,
                                            "handle": handle,
                                        },
                                    ),
                                },
                            )
                            if _is_error(close_resp):
                                failures.append(
                                    f"11: close_read failed: {_error_category(close_resp) or _error_text(close_resp)}"
                                )

        except TimeoutError as exc:
            failures.append(f"99: MCP communication timed out: {exc}")
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
        for failure in failures:
            print(f"- {failure}")
        return 1

    print("All checks passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
