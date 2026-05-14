#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercise SFTP metadata operations through the MCP wrapper."""

from __future__ import annotations

import json
import os
import socket
import subprocess
import sys
import threading
import time
from pathlib import Path
from typing import Any, Dict, List, Optional, Sequence, Set, Tuple

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
        if line.startswith("MCP_PORT="):
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
    def __init__(self, sock: socket.socket) -> None:
        self.sock = sock
        self.request_id = 1

    def call(self, method: str, params: Optional[Dict[str, Any]]) -> Dict[str, Any]:
        payload: Dict[str, Any] = {"jsonrpc": "2.0", "id": self.request_id, "method": method}
        self.request_id += 1
        if params is not None:
            payload["params"] = params
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
        if value:
            return value
    return default


def _int(defaulted: Optional[str], fallback: int) -> int:
    try:
        return int(defaulted) if defaulted is not None else fallback
    except ValueError:
        return fallback


def _read_string(response: Any, keys: Sequence[str]) -> Optional[str]:
    if not isinstance(response, dict):
        return None
    for key in keys:
        value = response.get(key)
        if isinstance(value, str):
            return value
    return None


def _extract_any(response: Dict[str, Any]) -> Any:
    result = response.get("result")
    if isinstance(result, dict):
        if "value" in result:
            return result["value"]
        if "result" in result:
            return result["result"]
    return result


def _extract_session(response: Dict[str, Any]) -> Any:
    result = _extract_any(response)
    if isinstance(result, dict):
        for key in ("session", "value", "result"):
            if key in result:
                return result[key]
    return result


def _is_error(response: Dict[str, Any]) -> bool:
    return isinstance(response, dict) and response.get("error") is not None


def _error_text(response: Dict[str, Any]) -> str:
    error = response.get("error")
    if isinstance(error, dict):
        message = str(error.get("message", ""))
        data = error.get("data")
        if isinstance(data, dict):
            for extra in ("message", "reason", "category"):
                if extra in data:
                    message += " " + str(data[extra])
        elif isinstance(data, str):
            message += " " + data
        return message
    if error is None:
        return ""
    return str(error)


def _error_category(response: Dict[str, Any]) -> Optional[str]:
    text = _error_text(response).lower()
    for category in ("not_found", "permission_denied", "io_error"):
        if category in text:
            return category
    return None


def _normalize(text: Any) -> str:
    s = str(text).lower()
    out: List[str] = []
    for ch in s:
        if ch.isalnum():
            out.append(ch)
        else:
            out.append(" ")
    return "".join(out)


def _find_tool(
    tools: List[Dict[str, Any]],
    candidates: Sequence[Sequence[str]],
    must_contain: Optional[Set[str]] = None,
) -> Optional[Dict[str, Any]]:
    if not isinstance(tools, list):
        return None

    must_contain = set(must_contain or set())

    best: Optional[Dict[str, Any]] = None
    best_score = -1
    for tool in tools:
        if not isinstance(tool, dict):
            continue
        name = str(tool.get("name", "")).lower()
        desc = str(tool.get("description", "")).lower()
        hay = set(_normalize(f"{name} {desc}").split()) | set(_normalize(name).split())
        if must_contain and not must_contain.issubset(hay):
            continue

        exact_score = 0
        for candidate in candidates:
            if not candidate:
                continue
            candidate_tokens = [token.lower() for token in candidate if token]
            if all(token in hay for token in candidate_tokens):
                exact_score += len(candidate_tokens)

        if exact_score > 0:
            if exact_score > best_score:
                best = tool
                best_score = exact_score

    if best is not None:
        return best

    # fallback: exact name match or closest substring match
    for tool in tools:
        if not isinstance(tool, dict):
            continue
        name = str(tool.get("name", "")).lower()
        lowered_name = _normalize(name)
        for candidate in candidates:
            candidate_text = " ".join(c.lower() for c in candidate)
            if candidate_text and candidate_text in lowered_name:
                return tool
    return None


def _tool_required_fields(tool: Dict[str, Any]) -> List[str]:
    schema = tool.get("inputSchema")
    if isinstance(schema, dict):
        req = schema.get("required")
        if isinstance(req, list):
            return [str(item) for item in req]
    return []


def _tool_properties(tool: Dict[str, Any]) -> Dict[str, Any]:
    schema = tool.get("inputSchema")
    if isinstance(schema, dict):
        props = schema.get("properties")
        if isinstance(props, dict):
            return props
    return {}


def _build_peer() -> Dict[str, Any]:
    return {
        "user": _env("AITC_SFTP_USER", "SFTP_USER", default="aitc"),
        "password": _env("AITC_SFTP_PASSWORD", "SFTP_PASSWORD", default=""),
        "host": _env("AITC_SFTP_HOST", "SFTP_HOST", default="127.0.0.1"),
        "port": _int(_env("AITC_SFTP_PORT", "SFTP_PORT", default="22"), 22),
        "root_path": _env("AITC_SFTP_ROOT", "SFTP_ROOT", default="/tmp/aitc"),
    }


def _default_value(field: str, schema: Dict[str, Any], force_session: bool = False) -> Any:
    lowered = field.lower()
    if lowered == "peer":
        return _build_peer()
    if lowered == "session":
        return "session-placeholder"
    if "path" in lowered:
        return "."
    if "port" in lowered:
        return 22
    if "timeout" in lowered or "seconds" in lowered or "max" in lowered:
        return 30
    if lowered == "max_bytes":
        return 1024

    if isinstance(schema, dict):
        if "enum" in schema and isinstance(schema["enum"], list) and schema["enum"]:
            return schema["enum"][0]
        schema_type = schema.get("type")
        if schema_type == "integer" or schema_type == "number":
            return 1
        if schema_type == "boolean":
            return False
        if schema_type == "array":
            child = schema.get("items") if isinstance(schema.get("items"), dict) else {}
            return [_default_value(field + "_item", child)]
        if schema_type == "object":
            props = schema.get("properties")
            if isinstance(props, dict):
                child = {}
                for child_name, child_schema in props.items():
                    if isinstance(child_name, str):
                        child[child_name] = _default_value(child_name, child_schema if isinstance(child_schema, dict) else {})
                return child

    if force_session and lowered == "session":
        return "session-placeholder"
    return "value"


def _build_arguments(tool: Dict[str, Any], overrides: Optional[Dict[str, Any]]) -> Dict[str, Any]:
    if not isinstance(tool, dict):
        return dict(overrides or {})

    schema = tool.get("inputSchema")
    if not isinstance(schema, dict):
        schema = {}
    required = schema.get("required")
    if not isinstance(required, list):
        required = []
    properties = schema.get("properties")
    if not isinstance(properties, dict):
        properties = {}

    args: Dict[str, Any] = {}
    if overrides:
        args.update(overrides)

    for field in required:
        if field in args:
            continue
        if not isinstance(field, str):
            continue
        child_schema = properties.get(field)
        if not isinstance(child_schema, dict):
            child_schema = {}
        args[field] = _default_value(field, child_schema)

    return args


def _is_entry_list(value: Any) -> bool:
    return isinstance(value, list) and all(isinstance(item, dict) for item in value)


def _coerce_entries(result: Any) -> List[Dict[str, Any]]:
    if isinstance(result, dict):
        for key in ("entries", "value", "result"):
            candidate = result.get(key)
            if isinstance(candidate, list):
                return [item for item in candidate if isinstance(item, dict)]
        return []
    if isinstance(result, list):
        return [item for item in result if isinstance(item, dict)]
    return []


def _assert_entry_shape(failures: List[str], case_id: str, entry: Any) -> None:
    if not isinstance(entry, dict):
        failures.append(f"{case_id}: list_dir returned non-dict entry {entry!r}")
        return

    name = entry.get("name")
    if not isinstance(name, str) or not name:
        failures.append(f"{case_id}: list_dir entry missing valid string name in {entry!r}")
        return
    if any(part in name for part in ("/", "\\")):
        failures.append(f"{case_id}: list_dir entry name {name!r} is not immediate child path")

    if name in {".", ".."}:
        failures.append(f"{case_id}: list_dir entry contains prohibited name {name!r}")

    if "is_dir" not in entry or not isinstance(entry.get("is_dir"), bool):
        failures.append(f"{case_id}: list_dir entry {name!r} missing bool is_dir")

    if "mod_time" not in entry or not isinstance(entry.get("mod_time"), (int, float)):
        failures.append(f"{case_id}: list_dir entry {name!r} missing numeric mod_time")

    if "byte_size" not in entry or not isinstance(entry.get("byte_size"), (int, float)):
        failures.append(f"{case_id}: list_dir entry {name!r} missing numeric byte_size")


def _assert_stat_shape(failures: List[str], case_id: str, payload: Any, must_be_dir: Optional[bool] = None) -> None:
    if not isinstance(payload, dict):
        failures.append(f"{case_id}: stat returned non-dict payload {payload!r}")
        return

    for field in ("is_dir", "mod_time", "byte_size"):
        if field not in payload:
            failures.append(f"{case_id}: stat payload missing {field}")

    if not isinstance(payload.get("is_dir"), bool):
        failures.append(f"{case_id}: stat payload has invalid is_dir type")
    if must_be_dir is not None and payload.get("is_dir") != must_be_dir:
        failures.append(f"{case_id}: stat.is_dir expected {must_be_dir} but got {payload.get('is_dir')} for payload {payload!r}")
    if not isinstance(payload.get("mod_time"), (int, float)):
        failures.append(f"{case_id}: stat payload has invalid mod_time type")
    if not isinstance(payload.get("byte_size"), (int, float)):
        failures.append(f"{case_id}: stat payload has invalid byte_size type")


def _pick_tool(tools: List[Dict[str, Any]], name: str) -> Optional[Dict[str, Any]]:
    for tool in tools:
        if str(tool.get("name")) == name:
            return tool
    for tool in tools:
        if str(tool.get("name")).lower() == name.lower():
            return tool
    return None


def main() -> int:
    failures: List[str] = []
    proc: Optional[subprocess.Popen[str]] = None
    list_tool: Optional[Dict[str, Any]] = None
    stat_tool: Optional[Dict[str, Any]] = None
    session_tool: Optional[Dict[str, Any]] = None

    list_path = _env("SFTP_LIST_DIR_PATH", "AITC_SFTP_LIST_PATH", "SFTP_PATH", default=".")
    missing_path = _env("SFTP_MISSING_PATH", "AITC_SFTP_MISSING_PATH", default="__aitc_missing_path__")
    file_path = _env("SFTP_METADATA_FILE", "SFTP_FILE_PATH", "AITC_SFTP_FILE_PATH", default="")
    dir_path = _env("SFTP_METADATA_DIR", "SFTP_DIR_PATH", "AITC_SFTP_DIR_PATH", default="")
    symlink_path = _env("SFTP_METADATA_SYMLINK", "AITC_SFTP_SYMLINK_PATH", default="")
    special_path = _env("SFTP_METADATA_SPECIAL", "AITC_SFTP_SPECIAL_PATH", default="")
    permission_path = _env("SFTP_PERMISSION_DENIED_PATH", "AITC_SFTP_PERMISSION_DENIED_PATH", default="")

    try:
        proc, port = _launch_mcp()
    except Exception as exc:
        failures.append(f"00: unable to launch MCP wrapper: {exc}")
        print("FAILURES:")
        for failure in failures:
            print(f"- {failure}")
        return 1

    try:
        try:
            sock = socket.create_connection(("127.0.0.1", port), timeout=10.0)
        except Exception as exc:
            failures.append(f"01: cannot connect to MCP server at 127.0.0.1:{port}: {exc}")
            print("FAILURES:")
            for failure in failures:
                print(f"- {failure}")
            return 1

        try:
            client = RpcClient(sock)
            tools_response = client.call("tools/list", None)
            if _is_error(tools_response):
                failures.append(f"01: tools/list returned protocol error: {_error_text(tools_response)}")
                tools: List[Dict[str, Any]] = []
            else:
                tools = tools_response.get("result", {}).get("tools", [])
                if not isinstance(tools, list):
                    failures.append("01: tools/list did not return a tools list")
                    tools = []
                elif not tools:
                    failures.append("01: tools/list returned zero tools")

            session_tool = _find_tool(
                tools,
                (("connect", "listing"), ("connect", "session"), ("connect",), ("acquire", "session"), ("session",)),
                must_contain={"peer"},
            )
            if session_tool is None:
                session_tool = _find_tool(
                    tools,
                    (("connect", "listing"), ("connect", "session"), ("connect",), ("acquire", "session"), ("session",)),
                )
                if session_tool is None:
                    failures.append("02: could not find a session bootstrap tool in tools/list")

            list_tool = _find_tool(
                tools,
                (("list", "dir"), ("list_dir",), ("list", "directory"), ("list", "entries")),
            )
            if list_tool is None:
                # fallback by exact name if candidate matching missed
                list_tool = _pick_tool(tools, "list_dir")
            if list_tool is None:
                failures.append("03: could not find list_dir tool in tools/list")

            stat_tool = _find_tool(
                tools,
                (("stat",), ("metadata",), ("file", "metadata")),
            )
            if stat_tool is None:
                stat_tool = _pick_tool(tools, "stat")
            if stat_tool is None:
                failures.append("04: could not find stat tool in tools/list")

            session_value: Any = None
            if session_tool is not None:
                session_args = _build_arguments(session_tool, {})
                session_response = client.call("tools/call", {"name": str(session_tool.get("name")), "arguments": session_args})
                if _is_error(session_response):
                    category = _error_category(session_response)
                    failures.append(
                        f"05: session bootstrap tool returned error: {category or _error_text(session_response)}"
                    )
                else:
                    session_value = _extract_session(session_response)
                    if session_value is None:
                        failures.append("05: session bootstrap tool returned no session value")

            if session_value is None:
                failures.append("06: session value is unavailable; metadata checks requiring session may be skipped")

            # list_dir happy path on existing directory/path
            if list_tool is not None:
                path_candidates: List[str] = [list_path]
                if list_path not in {".", ""}:
                    path_candidates.append(".")
                list_response = None
                for candidate in path_candidates:
                    arguments = {"path": candidate}
                    req_fields = {f.lower() for f in _tool_required_fields(list_tool)}
                    if "session" in req_fields and session_value is None and "session" not in arguments:
                        continue
                    if "session" in req_fields:
                        arguments["session"] = session_value
                    list_response = client.call("tools/call", {"name": str(list_tool.get("name")), "arguments": _build_arguments(list_tool, arguments)})
                    if not _is_error(list_response):
                        break

                if list_response is None:
                    failures.append("07: list_dir could not be called with any candidate path")
                elif _is_error(list_response):
                    category = _error_category(list_response)
                    failures.append(f"07: list_dir on existing path returned error: {category or _error_text(list_response)}")
                else:
                    entries = _coerce_entries(_extract_any(list_response))
                    if not _is_entry_list(entries):
                        failures.append(f"07: list_dir result is not an entry list: {entries!r}")
                    else:
                        for entry in entries:
                            _assert_entry_shape(failures, "07", entry)

                        for entry in entries:
                            if not isinstance(entry, dict):
                                continue
                            if entry.get("is_dir") is True and not dir_path:
                                dir_path_candidate = str(entry.get("name", "")).strip()
                                if dir_path_candidate:
                                    dir_path = dir_path_candidate
                                    break

                        for entry in entries:
                            if not isinstance(entry, dict):
                                continue
                            if entry.get("is_dir") is False and not file_path:
                                file_path_candidate = str(entry.get("name", "")).strip()
                                if file_path_candidate:
                                    file_path = file_path_candidate
                                    break

            # path and permissions/error obligations for stat/list operations
            if stat_tool is None:
                failures.append("08: cannot validate metadata error obligations without stat tool")
            else:
                stat_req_fields = {f.lower() for f in _tool_required_fields(stat_tool)}

                def do_stat(case_id: str, target: str, expected_is_dir: Optional[bool] = None) -> Optional[Dict[str, Any]]:
                    args = {"path": target}
                    if "session" in stat_req_fields:
                        if session_value is None:
                            failures.append(f"{case_id}: stat expects session but no session value is available")
                            return None
                        args["session"] = session_value
                    response = client.call("tools/call", {"name": str(stat_tool.get("name")), "arguments": _build_arguments(stat_tool, args)})
                    if _is_error(response):
                        failures.append(
                            f"{case_id}: stat({target!r}) returned unexpected error: {_error_category(response) or _error_text(response)}"
                        )
                        return None
                    result = _extract_any(response)
                    if result is None:
                        failures.append(f"{case_id}: stat({target!r}) returned empty payload")
                        return None
                    payload = result if isinstance(result, dict) else _extract_any(result)
                    _assert_stat_shape(failures, case_id, payload, expected_is_dir)
                    return payload if isinstance(payload, dict) else None

                if dir_path:
                    do_stat("08", dir_path, expected_is_dir=True)
                else:
                    failures.append("08: no directory path available to verify directory stat() contract")

                if file_path:
                    do_stat("09", file_path, expected_is_dir=False)
                else:
                    failures.append("09: no file path available to verify file stat() contract")

                # not found via stat
                not_found_args = {"path": missing_path}
                if "session" in stat_req_fields:
                    if session_value is None:
                        failures.append("10: cannot test not_found path: session missing")
                    else:
                        not_found_args["session"] = session_value
                response_nf = client.call("tools/call", {"name": str(stat_tool.get("name")), "arguments": _build_arguments(stat_tool, not_found_args)})
                if not _is_error(response_nf):
                    failures.append("10: stat for missing path unexpectedly succeeded")
                else:
                    category = _error_category(response_nf)
                    if category != "not_found":
                        failures.append(f"10: missing-path stat expected not_found, got {category or _error_text(response_nf)}")

                # symlink and special path obligations when explicit fixtures are configured
                if symlink_path:
                    symlink_args = {"path": symlink_path}
                    if "session" in stat_req_fields:
                        if session_value is None:
                            failures.append("11: cannot test symlink not_found obligation: session missing")
                        else:
                            symlink_args["session"] = session_value
                    if "session" not in symlink_args and "session" in stat_req_fields:
                        pass
                    else:
                        symlink_response = client.call("tools/call", {"name": str(stat_tool.get("name")), "arguments": _build_arguments(stat_tool, symlink_args)})
                        if not _is_error(symlink_response):
                            failures.append("11: stat on configured symlink path succeeded but should report not_found")
                        else:
                            category = _error_category(symlink_response)
                            if category != "not_found":
                                failures.append(
                                    f"11: stat on configured symlink path expected not_found, got {category or _error_text(symlink_response)}"
                                )

                if special_path:
                    special_args = {"path": special_path}
                    if "session" in stat_req_fields:
                        if session_value is None:
                            failures.append("12: cannot test special-entry not_found obligation: session missing")
                        else:
                            special_args["session"] = session_value
                    if "session" not in special_args and "session" in stat_req_fields:
                        pass
                    else:
                        special_response = client.call("tools/call", {"name": str(stat_tool.get("name")), "arguments": _build_arguments(stat_tool, special_args)})
                        if not _is_error(special_response):
                            failures.append("12: stat on configured special entry succeeded but should report not_found")
                        else:
                            category = _error_category(special_response)
                            if category != "not_found":
                                failures.append(
                                    f"12: stat on configured special entry expected not_found, got {category or _error_text(special_response)}"
                                )

                if permission_path and session_value is not None and "session" in stat_req_fields:
                    deny_args = {"path": permission_path, "session": session_value}
                    permission_resp = client.call("tools/call", {"name": str(stat_tool.get("name")), "arguments": _build_arguments(stat_tool, deny_args)})
                    if not _is_error(permission_resp):
                        failures.append("13: permission-denied path stat succeeded but expected failure")
                    else:
                        category = _error_category(permission_resp)
                        if category != "permission_denied":
                            failures.append(
                                f"13: permission-denied fixture expected permission_denied, got {category or _error_text(permission_resp)}"
                            )

                if list_tool is not None:
                    if list_path:
                        not_found_list = client.call(
                            "tools/call",
                            {
                                "name": str(list_tool.get("name")),
                                "arguments": _build_arguments(list_tool, {"path": missing_path, **({"session": session_value} if "session" in _tool_required_fields(list_tool) and session_value is not None else {})}),
                            },
                        )
                        if not _is_error(not_found_list):
                            failures.append("14: list_dir on missing path unexpectedly succeeded")
                        else:
                            category = _error_category(not_found_list)
                            if category != "not_found":
                                failures.append(
                                    f"14: list_dir on missing path expected not_found, got {category or _error_text(not_found_list)}"
                                )

            if session_tool is not None and "peer" in {f.lower() for f in _tool_required_fields(session_tool)}:
                bad_peer_args = {"peer": {**_build_peer(), "host": "127.0.0.1", "port": 1}}
                bad_session_args = _build_arguments(session_tool, bad_peer_args)
                bad_session_response = client.call("tools/call", {"name": str(session_tool.get("name")), "arguments": bad_session_args})
                if not _is_error(bad_session_response):
                    failures.append("15: session bootstrap with unreachable peer succeeded unexpectedly")
                else:
                    category = _error_category(bad_session_response)
                    if category not in {"io_error", None}:
                        failures.append(f"15: unreachable peer session expected io_error, got {category or _error_text(bad_session_response)}")

        except TimeoutError as exc:
            failures.append(f"99: MCP interaction timed out: {exc}")
        except Exception as exc:
            failures.append(f"99: unexpected MCP interaction error: {exc}")
        finally:
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
    sys.exit(main())
