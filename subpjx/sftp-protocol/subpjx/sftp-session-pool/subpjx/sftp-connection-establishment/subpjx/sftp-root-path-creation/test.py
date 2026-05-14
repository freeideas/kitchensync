#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercise SFTP root-path creation through the MCP wrapper."""

from __future__ import annotations

import json
import os
import socket
import subprocess
import sys
import threading
import time
from pathlib import Path
from typing import Any, Dict, List, Optional

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
        raise RuntimeError("MCP wrapper did not emit MCP_PORT=<n>")

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
                raise RuntimeError("socket closed while waiting for MCP response")
            buffer += chunk
            while b"\n" in buffer:
                line, _, rest = buffer.partition(b"\n")
                buffer = rest
                line = line.strip()
                if not line:
                    continue
                return json.loads(line.decode("utf-8"))

        raise TimeoutError("timed out waiting for MCP response")


def _env(*names: str, default: Optional[str] = None) -> Optional[str]:
    for name in names:
        value = os.environ.get(name)
        if value:
            return value
    return default


def _env_int(raw: Optional[str], fallback: int) -> int:
    try:
        return int(raw) if raw is not None else fallback
    except ValueError:
        return fallback


def _error_text(payload: Any) -> str:
    if payload is None:
        return ""
    if isinstance(payload, str):
        return payload
    if isinstance(payload, (int, float, bool)):
        return str(payload)
    if isinstance(payload, list):
        return " ".join(_error_text(item) for item in payload)
    if isinstance(payload, dict):
        parts: List[str] = []
        for key in ("message", "error", "code", "data", "category", "details"):
            if key in payload:
                parts.append(_error_text(payload[key]))
        return " ".join(parts)
    return str(payload)


def _is_error(response: Optional[Dict[str, Any]]) -> bool:
    return isinstance(response, dict) and response.get("error") is not None


def _error_category(response: Dict[str, Any]) -> Optional[str]:
    if not isinstance(response, dict):
        return None

    texts: List[str] = []
    if "error" in response:
        texts.append(_error_text(response.get("error")))
    if "result" in response:
        texts.append(_error_text(response.get("result")))

    lowered = " ".join(texts).lower()
    for category in ("io_error", "permission_denied", "not_found", "invalid_argument"):
        if category in lowered:
            return category
    return None


def _looks_like_arg_error(response: Dict[str, Any]) -> bool:
    if not isinstance(response, dict):
        return False
    text = _error_text(response.get("error")).lower()
    return any(token in text for token in ("invalid", "missing", "required", "argument", "parameters"))


def _parse_json_text(raw: Any) -> Any:
    if not isinstance(raw, str):
        return raw
    text = raw.strip()
    if not text:
        return ""
    if (text.startswith("{") and text.endswith("}")) or (text.startswith("[") and text.endswith("]")):
        try:
            return json.loads(text)
        except json.JSONDecodeError:
            return text
    return text


def _normalize_name(value: Any) -> str:
    return "".join(ch if ch.isalnum() else " " for ch in str(value).lower())


def _find_tool(tools: List[Dict[str, Any]], candidates: List[str]) -> Optional[Dict[str, Any]]:
    if not isinstance(tools, list):
        return None

    normalized = {_normalize_name(candidate) for candidate in candidates}
    for tool in tools:
        if _normalize_name(tool.get("name", "")) in normalized:
            return tool

    for tool in tools:
        name = str(tool.get("name", "")).lower()
        description = str(tool.get("description", "")).lower()
        haystack = f"{name} {description}"
        for candidate in candidates:
            simple = candidate.replace("_", " ").replace("-", " ").lower()
            if all(part in haystack for part in simple.split()):
                return tool

    return None


def _build_peer() -> Dict[str, Any]:
    peer_json = _env("AITC_SFTP_PEER", "SFTP_PEER", "SFTP_TEST_PEER")
    if peer_json and peer_json.strip().startswith("{"):
        try:
            parsed = json.loads(peer_json)
            if isinstance(parsed, dict):
                if "root_path" not in parsed:
                    parsed["root_path"] = _env("AITC_SFTP_ROOT", "SFTP_ROOT", default="/tmp/aitc")
                return parsed
        except json.JSONDecodeError:
            pass

    return {
        "user": _env("AITC_SFTP_USER", "SFTP_USER", default="aitc"),
        "password": _env("AITC_SFTP_PASSWORD", "SFTP_PASSWORD", default=""),
        "host": _env("AITC_SFTP_HOST", "SFTP_HOST", default="127.0.0.1"),
        "port": _env_int(_env("AITC_SFTP_PORT", "SFTP_PORT", default="22"), 22),
        "root_path": _env("AITC_SFTP_ROOT", "SFTP_ROOT", default="/tmp/aitc"),
    }


def _build_settings() -> Dict[str, Any]:
    return {
        "connection_timeout_seconds": _env_int(
            _env("AITC_SFTP_TIMEOUT_SECONDS", "SFTP_TIMEOUT", default="30"),
            30,
        )
    }


def _as_rooted_path(path: str) -> str:
    if not path.startswith("/"):
        return f"/{path}"
    return path


def _extract_result(result: Any) -> Any:
    if isinstance(result, dict) and "result" in result and len(result) == 1:
        return result["result"]
    if isinstance(result, dict) and "content" in result:
        content = result["content"]
        if isinstance(content, list):
            for item in content:
                if not isinstance(item, dict):
                    continue
                if "json" in item:
                    parsed = _parse_json_text(item["json"])
                    if parsed is not None:
                        return parsed
                if "text" in item:
                    text = item["text"]
                    if isinstance(text, str):
                        stripped = text.strip()
                        if stripped:
                            return stripped
    return result


def _extract_status(result: Any) -> Optional[str]:
    if isinstance(result, dict):
        for key in ("status", "value", "result", "root_path_status", "state"):
            value = result.get(key)
            if isinstance(value, str):
                return value.lower()
        if len(result) == 1:
            only = next(iter(result.values()))
            if isinstance(only, str):
                return only.lower()
        return None
    if isinstance(result, str):
        return result.lower()
    return None


def _extract_handle(result: Any) -> Any:
    if isinstance(result, dict):
        for key in ("session", "handle", "value", "result"):
            if key in result:
                return result[key]
        content = result.get("content")
        if isinstance(content, list):
            for item in content:
                if not isinstance(item, dict):
                    continue
                if "json" in item:
                    return _parse_json_text(item["json"])
                if "text" in item and isinstance(item["text"], str):
                    return item["text"]
        return result
    return result


def _value_for_field(field_name: str, schema: Dict[str, Any], overrides: Dict[str, Any]) -> Any:
    lowered = field_name.lower()

    if "session" in lowered or "handle" in lowered or "filesystem" in lowered:
        for alias in ("session", "handle", "filesystem", "sftp_filesystem"):
            if alias in overrides:
                return overrides[alias]

    if "peer" in lowered:
        return _build_peer()
    if "settings" in lowered:
        return _build_settings()
    if "root" in lowered and "path" in lowered:
        return overrides.get("root_path", "/tmp/aitc")
    if "path" in lowered:
        return overrides.get("path", "/tmp/aitc")

    schema_type = schema.get("type")
    if schema_type == "integer" or schema_type == "number":
        if "port" in lowered or "seconds" in lowered or "max" in lowered:
            return 30
        return 1
    if schema_type == "boolean":
        return False
    if schema_type == "array":
        item_schema = schema.get("items")
        if isinstance(item_schema, dict):
            return [_value_for_field(f"{field_name}_item", item_schema, overrides)]
        return ["item"]
    if schema_type == "object":
        props = schema.get("properties", {})
        result: Dict[str, Any] = {}
        if isinstance(props, dict):
            for nested_name, nested_schema in props.items():
                if isinstance(nested_name, str):
                    result[nested_name] = _value_for_field(
                        nested_name,
                        nested_schema if isinstance(nested_schema, dict) else {},
                        {},
                    )
            return result

    if "user" in lowered:
        return _env("AITC_SFTP_USER", "SFTP_USER", default="aitc")
    if "host" in lowered:
        return _env("AITC_SFTP_HOST", "SFTP_HOST", default="127.0.0.1")
    if "password" in lowered:
        return _env("AITC_SFTP_PASSWORD", "SFTP_PASSWORD", default="")
    if "port" in lowered:
        return 22

    if schema_type == "string" or schema_type is None:
        return overrides.get(field_name, "value")

    return overrides.get(field_name, "value")


def _build_arguments(tool: Dict[str, Any], explicit: Dict[str, Any]) -> Dict[str, Any]:
    schema = tool.get("inputSchema", {}) if isinstance(tool, dict) else {}
    if not isinstance(schema, dict):
        schema = {}

    properties = schema.get("properties", {}) if isinstance(schema, dict) else {}
    if not isinstance(properties, dict):
        properties = {}

    required = schema.get("required", []) if isinstance(schema, dict) else []
    if not isinstance(required, list):
        required = []

    arguments: Dict[str, Any] = dict(explicit)
    for name in required:
        if name in arguments:
            continue
        name_schema = properties.get(name, {}) if isinstance(properties.get(name), dict) else {}
        if not isinstance(name_schema, dict):
            name_schema = {}
        arguments[name] = _value_for_field(name, name_schema, explicit)

    return arguments


def _call_with_candidates(
    client: RpcClient,
    tool: Dict[str, Any],
    candidate_maps: List[Dict[str, Any]],
) -> tuple[Dict[str, Any], Optional[Dict[str, Any]]]:
    last_response: Dict[str, Any] = {"error": "no response"}
    for args in candidate_maps:
        response = client.call("tools/call", {"name": tool["name"], "arguments": args})
        last_response = response
        if _is_error(response) and _looks_like_arg_error(response):
            continue
        return response, args
    return last_response, None


def _ensure_candidates(filesystem: Any, root_path: str) -> List[Dict[str, Any]]:
    return [
        {"sftp_filesystem": filesystem, "root_path": root_path},
        {"filesystem": filesystem, "root_path": root_path},
        {"session": filesystem, "root_path": root_path},
        {"sftp_filesystem": filesystem, "path": root_path},
        {"filesystem": filesystem, "path": root_path},
        {"session": filesystem, "path": root_path},
    ]


def _bootstrap_candidates(peer: Dict[str, Any], settings: Dict[str, Any]) -> List[Dict[str, Any]]:
    with_peer = {
        "inputSchema": {
            "required": ["peer"],
            "properties": {"peer": {"type": "object"}, "settings": {"type": "object"}},
        }
    }
    with_peer_and_settings = {
        "inputSchema": {
            "required": ["peer", "settings"],
            "properties": {"peer": {"type": "object"}, "settings": {"type": "object"}},
        }
    }
    return [
        _build_arguments(
            with_peer_and_settings,
            {"peer": peer, "settings": settings},
        ),
        _build_arguments(
            with_peer,
            {"peer": peer},
        ),
        {"sftp_peer": peer, "connection_settings": settings},
        {"peer": peer, "connection_timeout": settings.get("connection_timeout_seconds")},
        {"connection_timeout_seconds": settings.get("connection_timeout_seconds")},
    ]


def _run_assertions(client: RpcClient, tools: List[Dict[str, Any]], failures: List[str]) -> None:
    ensure_tool = _find_tool(tools, ["ensure_root_path", "ensure-root-path"])
    if ensure_tool is None:
        failures.append("01: tool list did not include ensure_root_path")
        return

    bootstrap_tool = _find_tool(
        tools,
        ["connect_listing", "connect-listing", "acquire", "connect", "open_connection", "open-connection"],
    )
    if bootstrap_tool is None:
        failures.append("02: missing bootstrap tool for creating a live SFTP filesystem/session")
        return

    peer = _build_peer()
    settings = _build_settings()
    bootstrap_response, _ = _call_with_candidates(
        client,
        bootstrap_tool,
        _bootstrap_candidates(peer, settings),
    )

    if _is_error(bootstrap_response):
        failures.append(
            f"03: bootstrap call failed ({_error_category(bootstrap_response) or _error_text(bootstrap_response.get('error'))})"
        )
        return

    session = _extract_handle(_extract_result(bootstrap_response.get("result")))
    if session is None:
        failures.append("03: bootstrap returned no session handle")
        return

    base_root = _as_rooted_path(str(peer.get("root_path", "/tmp/aitc"))).rstrip("/")
    marker = f"aitc-rp-{int(time.time())}-{os.getpid()}"
    nested_created = f"{base_root}/{marker}/deep/nested/path"

    first_existing, _ = _call_with_candidates(
        client,
        ensure_tool,
        _ensure_candidates(session, base_root),
    )
    second_existing, _ = _call_with_candidates(
        client,
        ensure_tool,
        _ensure_candidates(session, base_root),
    )

    first_status = _extract_status(_extract_result(first_existing.get("result")))
    second_status = _extract_status(_extract_result(second_existing.get("result")))

    if first_status is None:
        if _is_error(first_existing):
            failures.append(f"04: ensure_root_path base-root call failed: {_error_category(first_existing) or _error_text(first_existing.get('error'))}")
        else:
            failures.append(f"04: ensure_root_path base-root result was not parseable: {first_existing.get('result')}")
    elif first_status not in {"exists", "created"}:
        failures.append(f"04: ensure_root_path base-root status expected exists or created, got {first_status!r}")

    if second_status != "exists":
        if _is_error(second_existing):
            failures.append(
                f"04: ensure_root_path base-root idempotency call failed: {_error_category(second_existing) or _error_text(second_existing.get('error'))}"
            )
        else:
            failures.append(f"04: ensure_root_path base-root idempotency expected exists, got {second_status!r}")

    created_resp, _ = _call_with_candidates(
        client,
        ensure_tool,
        _ensure_candidates(session, nested_created),
    )
    created_status = _extract_status(_extract_result(created_resp.get("result")))
    if created_status is None:
        if _is_error(created_resp):
            failures.append(
                f"05: ensure_root_path creation-path call failed: {_error_category(created_resp) or _error_text(created_resp.get('error'))}"
            )
        else:
            failures.append(f"05: ensure_root_path creation-path result was not parseable: {created_resp.get('result')}")
    elif created_status != "created":
        failures.append(f"05: ensure_root_path missing path expected created, got {created_status!r}")

    idempotent_resp, _ = _call_with_candidates(
        client,
        ensure_tool,
        _ensure_candidates(session, nested_created),
    )
    idempotent_status = _extract_status(_extract_result(idempotent_resp.get("result")))
    if idempotent_status is None:
        if _is_error(idempotent_resp):
            failures.append(
                f"06: ensure_root_path idempotency call for {nested_created} failed: {_error_category(idempotent_resp) or _error_text(idempotent_resp.get('error'))}"
            )
        else:
            failures.append(
                f"06: ensure_root_path idempotency result for {nested_created} was not parseable: {idempotent_resp.get('result')}"
            )
    elif idempotent_status != "exists":
        failures.append(f"06: ensure_root_path idempotency expected exists, got {idempotent_status!r}")

    non_dir_parents = [
        _env("AITC_SFTP_NON_DIR_PARENT_PATH", default="/etc/hosts"),
        "/proc/version",
        "/dev/null",
        "/bin/sh",
    ]
    non_dir_error: Optional[Dict[str, Any]] = None
    non_dir_used = None
    for parent in non_dir_parents:
        candidate = f"{_as_rooted_path(parent)}/aitc-non-dir"
        response, _ = _call_with_candidates(
            client,
            ensure_tool,
            _ensure_candidates(session, candidate),
        )
        if _is_error(response):
            non_dir_error = response
            non_dir_used = candidate
            break

    if non_dir_error is None:
        failures.append(
            "07: failed to evaluate non-directory-component failure; no candidate returned an MCP error"
        )
    elif _error_category(non_dir_error) != "io_error":
        failures.append(
            f"07: non-directory component failure for {non_dir_used} expected io_error, got {_error_category(non_dir_error) or _error_text(non_dir_error.get('error'))}"
        )

    cannot_candidates = [
        _env("AITC_SFTP_UNCREATABLE_PATH"),
        f"{base_root}/{('x' * 500)}",
        f"/root/{('z' * 32)}/{('z' * 32)}",
    ]
    cannot_error: Optional[Dict[str, Any]] = None
    cannot_used = None
    for raw_path in cannot_candidates:
        if not raw_path:
            continue
        path = _as_rooted_path(raw_path)
        response, _ = _call_with_candidates(
            client,
            ensure_tool,
            _ensure_candidates(session, path),
        )
        if _is_error(response):
            cannot_error = response
            cannot_used = path
            break

    if cannot_error is None:
        failures.append(
            "08: failed to evaluate uncreatable-path failure; no candidate returned an MCP error"
        )
    else:
        category = _error_category(cannot_error)
        if category not in ("io_error", "permission_denied"):
            failures.append(
                f"08: uncreatable-path failure for {cannot_used} expected io_error (or permission_denied), got {category or _error_text(cannot_error.get('error'))}"
            )


def main() -> int:
    failures: List[str] = []
    proc: Optional[subprocess.Popen[str]] = None

    try:
        proc, port = _launch_mcp()
    except Exception as exc:
        print(f"FAILURES:\n- 00: failed to launch MCP wrapper: {exc}")
        return 1

    try:
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=10.0) as sock:
                client = RpcClient(sock)
                list_response = client.call("tools/list", None)
                if _is_error(list_response):
                    failures.append(
                        f"00: tools/list failed: {_error_category(list_response) or _error_text(list_response.get('error'))}"
                    )
                    tools: List[Dict[str, Any]] = []
                else:
                    tools = list_response.get("result", {}).get("tools", [])
                    if not isinstance(tools, list) or not tools:
                        failures.append("00: tools/list did not return a non-empty tools list")

                if tools:
                    _run_assertions(client, tools, failures)
        except TimeoutError as exc:
            failures.append(f"99: MCP communication timeout: {exc}")
        except Exception as exc:
            failures.append(f"99: unexpected MCP interaction error: {exc}")
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
    sys.exit(main())
