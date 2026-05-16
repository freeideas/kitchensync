#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercise the gitignore matcher through its MCP wrapper."""

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

REQUIRED_TOOLS = {
    "compile-matcher",
    "empty-matcher",
    "extend-matcher",
    "filter-entries",
    "match-entry",
}

DEFAULT_OPTIONS: dict[str, Any] = {}


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

        deadline = time.time() + 20
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


def layer(base_path: str, pattern_text: str, source_name: str) -> dict[str, str]:
    return {"base_path": base_path, "pattern_text": pattern_text, "source_name": source_name}


def entry(relative_path: str, kind: str = "regular_file") -> dict[str, str]:
    return {"relative_path": relative_path, "kind": kind}


def expect_success(failures: list[str], label: str, response: dict[str, Any]) -> dict[str, Any]:
    if "error" in response:
        failures.append(f"{label}: expected success, got {response['error']}")
        return {}
    result = response.get("result")
    if not isinstance(result, dict):
        failures.append(f"{label}: expected object result, got {response}")
        return {}
    return result


def result_id(result: dict[str, Any]) -> str | None:
    for key in ("matcher_id", "matcherId", "id"):
        value = result.get(key)
        if isinstance(value, str) and value:
            return value
    return None


def compile_matcher(
    rpc: RpcClient,
    failures: list[str],
    label: str,
    layers: list[dict[str, str]],
    options: dict[str, Any] | None = None,
) -> str | None:
    result = expect_success(
        failures,
        label,
        rpc.call_tool("compile-matcher", {"layers": layers, "options": DEFAULT_OPTIONS if options is None else options}),
    )
    matcher_id = result_id(result)
    if matcher_id is None:
        failures.append(f"{label}: result must include a matcher_id string, got {result}")
    return matcher_id


def empty_matcher(
    rpc: RpcClient,
    failures: list[str],
    label: str,
    options: dict[str, Any] | None = None,
) -> str | None:
    result = expect_success(
        failures,
        label,
        rpc.call_tool("empty-matcher", {"options": DEFAULT_OPTIONS if options is None else options}),
    )
    matcher_id = result_id(result)
    if matcher_id is None:
        failures.append(f"{label}: result must include a matcher_id string, got {result}")
    return matcher_id


def extend_matcher(
    rpc: RpcClient,
    failures: list[str],
    label: str,
    matcher_id: str | None,
    next_layer: dict[str, str],
) -> str | None:
    if matcher_id is None:
        failures.append(f"{label}: cannot extend because matcher id was missing")
        return None
    result = expect_success(
        failures,
        label,
        rpc.call_tool("extend-matcher", {"matcher_id": matcher_id, "layer": next_layer}),
    )
    extended_id = result_id(result)
    if extended_id is None:
        failures.append(f"{label}: result must include a matcher_id string, got {result}")
    return extended_id


def match_result(
    rpc: RpcClient,
    failures: list[str],
    label: str,
    matcher_id: str | None,
    path_entry: dict[str, str],
) -> dict[str, Any]:
    if matcher_id is None:
        failures.append(f"{label}: cannot match because matcher id was missing")
        return {}
    result = expect_success(
        failures,
        label,
        rpc.call_tool("match-entry", {"matcher_id": matcher_id, "entry": path_entry}),
    )
    nested = result.get("match")
    return nested if isinstance(nested, dict) else result


def filter_entries(
    rpc: RpcClient,
    failures: list[str],
    label: str,
    matcher_id: str | None,
    entries: list[dict[str, str]],
) -> list[dict[str, Any]]:
    if matcher_id is None:
        failures.append(f"{label}: cannot filter because matcher id was missing")
        return []
    result = expect_success(
        failures,
        label,
        rpc.call_tool("filter-entries", {"matcher_id": matcher_id, "entries": entries}),
    )
    values = result.get("entries")
    if not isinstance(values, list):
        failures.append(f"{label}: expected result.entries array, got {result}")
        return []
    return [value for value in values if isinstance(value, dict)]


def assert_equal(failures: list[str], label: str, actual: Any, expected: Any) -> None:
    if actual != expected:
        failures.append(f"{label}: expected {expected!r}, got {actual!r}")


def assert_match(
    failures: list[str],
    label: str,
    actual: dict[str, Any],
    *,
    ignored: bool,
    rule_kind: str,
    negated: bool | None = None,
    pattern: str | None = None,
    source_name: str | None = None,
    line_number: int | None = None,
) -> None:
    assert_equal(failures, f"{label} ignored", actual.get("ignored"), ignored)
    assert_equal(failures, f"{label} rule_kind", actual.get("rule_kind"), rule_kind)
    if negated is not None:
        assert_equal(failures, f"{label} negated", actual.get("negated"), negated)
    if pattern is not None:
        assert_equal(failures, f"{label} pattern", actual.get("pattern"), pattern)
    if source_name is not None:
        assert_equal(failures, f"{label} source_name", actual.get("source_name"), source_name)
    if line_number is not None:
        assert_equal(failures, f"{label} line_number", actual.get("line_number"), line_number)


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
    elif isinstance(data, str):
        parts.append(data)
    return " ".join(parts)


def expect_error_category(failures: list[str], label: str, response: dict[str, Any], category: str) -> None:
    error = response.get("error")
    if not isinstance(error, dict):
        failures.append(f"{label}: expected {category} error, got {response}")
        return
    if category not in error_text(response):
        failures.append(f"{label}: expected {category} in error message or data, got {error}")
    if "result" in response:
        failures.append(f"{label}: error response must not include a partial result, got {response}")


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


def run_concurrent_matches(port: int, matcher_id: str | None, failures: list[str]) -> None:
    if matcher_id is None:
        failures.append("09 concurrent match: matcher id was missing")
        return

    thread_failures: list[str] = []
    lock = threading.Lock()
    cases = [
        (entry("app.log"), True, "pattern"),
        (entry("important.log"), False, "pattern"),
        (entry("src/main.txt"), False, "none"),
        (entry(".kitchensync/state.db"), True, "always_builtin"),
    ]

    def worker(index: int) -> None:
        try:
            local_failures: list[str] = []
            with socket.create_connection(("127.0.0.1", port), timeout=10) as sock:
                rpc = RpcClient(sock)
                for iteration in range(20):
                    path_entry, expected_ignored, expected_rule = cases[(index + iteration) % len(cases)]
                    actual = match_result(rpc, local_failures, f"09 thread {index} iter {iteration}", matcher_id, path_entry)
                    if actual.get("ignored") != expected_ignored or actual.get("rule_kind") != expected_rule:
                        local_failures.append(
                            "09 concurrent match: "
                            f"thread {index} iter {iteration} expected "
                            f"ignored={expected_ignored} rule={expected_rule}, got {actual}"
                        )
            if local_failures:
                with lock:
                    thread_failures.extend(local_failures)
        except Exception as exc:
            with lock:
                thread_failures.append(f"09 concurrent match: thread {index} raised {exc!r}")

    threads = [threading.Thread(target=worker, args=(index,)) for index in range(8)]
    for thread in threads:
        thread.start()
    for thread in threads:
        thread.join(timeout=15)
        if thread.is_alive():
            failures.append("09 concurrent match: worker thread did not finish")

    failures.extend(thread_failures)


def main() -> int:
    proc: subprocess.Popen[str] | None = None
    port = 0
    try:
        proc, port, stdout_lines, stderr_lines = launch_mcp()
        failures: list[str] = []
        with socket.create_connection(("127.0.0.1", port), timeout=10) as sock:
            rpc = RpcClient(sock)

            time.sleep(0.1)
            stdout_before = len(stdout_lines)
            stderr_before = len(stderr_lines)

            tools = expect_success(failures, "01 tools/list", rpc.request("tools/list"))
            require_tools(failures, tools)
            print(f"[01] tools/list exposed {len(tools.get('tools', []))} tool(s)")

            pattern_text = "\n".join(
                [
                    "# comment",
                    "",
                    r"\#literal",
                    r"\!literal",
                    r"space\ ",
                    "plain-space   ",
                    "*.log",
                    "!important.log",
                    "src/?ain.[ch]",
                    "[ab]rack.txt",
                    "[!0-9]ode.txt",
                    "/root-only.txt",
                    "docs/*.md",
                    "build/",
                    "**/temp.txt",
                    "assets/**",
                    "a/**/b.txt",
                    "foo***bar",
                    "range-[a-z].txt",
                    ".gitignore",
                    "*.LOG",
                    "ordered.txt",
                    "!ordered.txt",
                    "ordered.*",
                    "mid#hash",
                ]
            )
            patterns = compile_matcher(rpc, failures, "02 compile pattern matcher", [layer("", pattern_text, "root")])
            pattern_cases = [
                ("02 escaped #", entry("#literal"), True, "pattern", r"\#literal", 3),
                ("02 escaped !", entry("!literal"), True, "pattern", r"\!literal", 4),
                ("02 escaped trailing space", entry("space "), True, "pattern", r"space\ ", 5),
                ("02 unescaped trailing spaces", entry("plain-space"), True, "pattern", "plain-space   ", 6),
                ("02 star", entry("app.log"), True, "pattern", "*.log", 7),
                ("02 negation", entry("important.log"), False, "pattern", "!important.log", 8),
                ("02 question and bracket", entry("src/main.c"), True, "pattern", "src/?ain.[ch]", 9),
                ("02 bracket set", entry("brack.txt"), True, "pattern", "[ab]rack.txt", 10),
                ("02 bracket negation", entry("code.txt"), True, "pattern", "[!0-9]ode.txt", 11),
                ("02 leading slash anchors", entry("root-only.txt"), True, "pattern", "/root-only.txt", 12),
                ("02 interior slash", entry("docs/readme.md"), True, "pattern", "docs/*.md", 13),
                ("02 directory pattern", entry("src/build", "directory"), True, "pattern", "build/", 14),
                ("02 start double-star root", entry("temp.txt"), True, "pattern", "**/temp.txt", 15),
                ("02 start double-star nested", entry("x/y/temp.txt"), True, "pattern", "**/temp.txt", 15),
                ("02 end double-star", entry("assets/icons/logo.png"), True, "pattern", "assets/**", 16),
                ("02 middle double-star shallow", entry("a/b.txt"), True, "pattern", "a/**/b.txt", 17),
                ("02 middle double-star deep", entry("a/x/y/b.txt"), True, "pattern", "a/**/b.txt", 17),
                ("02 consecutive stars", entry("foobazbar"), True, "pattern", "foo***bar", 18),
                ("02 bracket range", entry("range-m.txt"), True, "pattern", "range-[a-z].txt", 19),
                ("02 ignore file name has no special bypass", entry(".gitignore"), True, "pattern", ".gitignore", 20),
                ("02 case-sensitive match", entry("APP.LOG"), True, "pattern", "*.LOG", 21),
                ("02 interior # is literal", entry("mid#hash"), True, "pattern", "mid#hash", 25),
            ]
            for label, path_entry, ignored, rule_kind, pattern, line_number in pattern_cases:
                assert_match(
                    failures,
                    label,
                    match_result(rpc, failures, label, patterns, path_entry),
                    ignored=ignored,
                    rule_kind=rule_kind,
                    pattern=pattern,
                    source_name="root",
                    line_number=line_number,
                )
            assert_match(
                failures,
                "02 comments are ignored",
                match_result(rpc, failures, "02 comment miss", patterns, entry("# comment")),
                ignored=False,
                rule_kind="none",
            )
            assert_match(
                failures,
                "02 anchored pattern excludes nested path",
                match_result(rpc, failures, "02 anchored miss", patterns, entry("sub/root-only.txt")),
                ignored=False,
                rule_kind="none",
            )
            assert_match(
                failures,
                "02 interior slash excludes deeper path",
                match_result(rpc, failures, "02 interior slash miss", patterns, entry("docs/nested/readme.md")),
                ignored=False,
                rule_kind="none",
            )
            assert_match(
                failures,
                "02 directory-only does not match file",
                match_result(rpc, failures, "02 directory-only file", patterns, entry("build", "regular_file")),
                ignored=False,
                rule_kind="none",
            )
            assert_match(
                failures,
                "02 basename pattern matches nested path",
                match_result(rpc, failures, "02 nested basename", patterns, entry("logs/app.log")),
                ignored=True,
                rule_kind="pattern",
                pattern="*.log",
                source_name="root",
                line_number=7,
            )
            assert_match(
                failures,
                "02 uppercase suffix pattern matches uppercase suffix",
                match_result(rpc, failures, "02 case-sensitive uppercase match", patterns, entry("app.LOG")),
                ignored=True,
                rule_kind="pattern",
                pattern="*.LOG",
                source_name="root",
                line_number=21,
            )
            assert_match(
                failures,
                "02 matching is case-sensitive",
                match_result(rpc, failures, "02 case-sensitive miss", patterns, entry("app.Log")),
                ignored=False,
                rule_kind="none",
            )
            assert_match(
                failures,
                "02 later matching pattern overrides earlier negation",
                match_result(rpc, failures, "02 later override", patterns, entry("ordered.txt")),
                ignored=True,
                rule_kind="pattern",
                pattern="ordered.*",
                source_name="root",
                line_number=24,
            )
            print("[02] pattern parsing and wildcard semantics exercised")

            filter_matcher = compile_matcher(
                rpc,
                failures,
                "03 compile filter matcher",
                [layer("", "*.log\nbuild/\n!important.log\n", "filter-root")],
            )
            filter_input = [
                entry("src", "directory"),
                entry("app.log"),
                entry("important.log"),
                entry("src/build", "directory"),
                entry("src/build/out.bin"),
                entry("src/main.txt"),
            ]
            kept = filter_entries(rpc, failures, "03 filter entries", filter_matcher, filter_input)
            assert_equal(
                failures,
                "03 filter returns non-ignored input entries in order",
                kept,
                [entry("src", "directory"), entry("important.log"), entry("src/main.txt")],
            )
            assert_match(
                failures,
                "03 ignored directory applies to descendants",
                match_result(rpc, failures, "03 descendant", filter_matcher, entry("src/build/out.bin")),
                ignored=True,
                rule_kind="pattern",
            )
            print("[03] filter removes ignored entries and directory exclusions cover descendants")

            layered = compile_matcher(
                rpc,
                failures,
                "04 compile layered matcher",
                [
                    layer("", "*.tmp\n", "root-layer"),
                    layer("docs", "!keep.tmp\nmanual/*.bak\n/root-only.md\n", "docs-layer"),
                ],
            )
            layer_expectations = [
                ("04 root tmp", entry("scratch.tmp"), True, "*.tmp", "root-layer", 1, False),
                ("04 parent applies below", entry("docs/draft.tmp"), True, "*.tmp", "root-layer", 1, False),
                ("04 deeper negation", entry("docs/keep.tmp"), False, "!keep.tmp", "docs-layer", 1, True),
                ("04 deeper slash pattern", entry("docs/manual/old.bak"), True, "manual/*.bak", "docs-layer", 2, False),
                ("04 leading slash anchors to layer base", entry("docs/root-only.md"), True, "/root-only.md", "docs-layer", 3, False),
                ("04 deeper layer scoped", entry("elsewhere/keep.tmp"), True, "*.tmp", "root-layer", 1, False),
            ]
            for label, path_entry, ignored, pattern, source, line_number, negated in layer_expectations:
                assert_match(
                    failures,
                    label,
                    match_result(rpc, failures, label, layered, path_entry),
                    ignored=ignored,
                    rule_kind="pattern",
                    negated=negated,
                    pattern=pattern,
                    source_name=source,
                    line_number=line_number,
                )
            assert_match(
                failures,
                "04 anchored layer pattern excludes nested path",
                match_result(rpc, failures, "04 layer anchored miss", layered, entry("docs/sub/root-only.md")),
                ignored=False,
                rule_kind="none",
            )
            assert_match(
                failures,
                "04 deeper layer does not apply outside base path",
                match_result(rpc, failures, "04 layer scoped miss", layered, entry("elsewhere/manual/old.bak")),
                ignored=False,
                rule_kind="none",
            )
            same_base_order = compile_matcher(
                rpc,
                failures,
                "04 compile same-base layer order matcher",
                [
                    layer("", "*.tmp\n", "same-base-first"),
                    layer("", "!keep.tmp\n", "same-base-second"),
                ],
            )
            assert_match(
                failures,
                "04 supplied layer order is honored",
                match_result(rpc, failures, "04 same-base layer order", same_base_order, entry("keep.tmp")),
                ignored=False,
                rule_kind="pattern",
                negated=True,
                pattern="!keep.tmp",
                source_name="same-base-second",
                line_number=1,
            )
            print("[04] hierarchical layers, base paths, and supplied layer order exercised")

            builtins = empty_matcher(rpc, failures, "05 empty matcher")
            assert_match(
                failures,
                "05 default .git",
                match_result(rpc, failures, "05 default .git", builtins, entry(".git", "directory")),
                ignored=True,
                rule_kind="default_builtin",
            )
            assert_match(
                failures,
                "05 default .git descendant",
                match_result(rpc, failures, "05 default .git descendant", builtins, entry(".git/config")),
                ignored=True,
                rule_kind="default_builtin",
            )
            assert_match(
                failures,
                "05 default nested .git",
                match_result(rpc, failures, "05 nested .git", builtins, entry("src/.git", "directory")),
                ignored=True,
                rule_kind="default_builtin",
            )
            assert_match(
                failures,
                "05 default nested .git descendant",
                match_result(rpc, failures, "05 nested .git descendant", builtins, entry("src/.git/config")),
                ignored=True,
                rule_kind="default_builtin",
            )
            assert_match(
                failures,
                "05 default .git does not match regular file",
                match_result(rpc, failures, "05 .git regular file", builtins, entry(".git")),
                ignored=False,
                rule_kind="none",
            )
            assert_match(
                failures,
                "05 default .kitchensync",
                match_result(rpc, failures, "05 default .kitchensync", builtins, entry(".kitchensync", "directory")),
                ignored=True,
                rule_kind="always_builtin",
            )
            assert_match(
                failures,
                "05 default .kitchensync descendant",
                match_result(rpc, failures, "05 .kitchensync descendant", builtins, entry(".kitchensync/snapshot.db")),
                ignored=True,
                rule_kind="always_builtin",
            )
            assert_match(
                failures,
                "05 default nested .kitchensync",
                match_result(rpc, failures, "05 nested .kitchensync", builtins, entry("src/.kitchensync", "directory")),
                ignored=True,
                rule_kind="always_builtin",
            )
            assert_match(
                failures,
                "05 default nested .kitchensync descendant",
                match_result(rpc, failures, "05 nested .kitchensync descendant", builtins, entry("src/.kitchensync/state.db")),
                ignored=True,
                rule_kind="always_builtin",
            )
            assert_match(
                failures,
                "05 default .kitchensync does not match regular file",
                match_result(rpc, failures, "05 .kitchensync regular file", builtins, entry(".kitchensync")),
                ignored=False,
                rule_kind="none",
            )
            assert_match(
                failures,
                "05 default symlink",
                match_result(rpc, failures, "05 default symlink", builtins, entry("link", "symlink")),
                ignored=True,
                rule_kind="always_builtin",
            )
            assert_match(
                failures,
                "05 default special entry",
                match_result(rpc, failures, "05 default special", builtins, entry("sock", "special")),
                ignored=True,
                rule_kind="always_builtin",
            )

            reinclude_builtins = compile_matcher(
                rpc,
                failures,
                "05 compile builtin negations",
                [layer("", "!.git/\n!.kitchensync/\n!link\n!sock\n", "builtin-negations")],
            )
            assert_match(
                failures,
                "05 .git can be re-included",
                match_result(rpc, failures, "05 .git reinclude", reinclude_builtins, entry(".git", "directory")),
                ignored=False,
                rule_kind="pattern",
                negated=True,
                pattern="!.git/",
            )
            assert_match(
                failures,
                "05 .git descendant re-included",
                match_result(rpc, failures, "05 .git descendant", reinclude_builtins, entry(".git/config")),
                ignored=False,
                rule_kind="pattern",
                negated=True,
                pattern="!.git/",
            )
            assert_match(
                failures,
                "05 .kitchensync cannot be re-included",
                match_result(rpc, failures, "05 .kitchensync cannot reinclude", reinclude_builtins, entry(".kitchensync", "directory")),
                ignored=True,
                rule_kind="always_builtin",
            )
            assert_match(
                failures,
                "05 .kitchensync descendant cannot be re-included",
                match_result(rpc, failures, "05 .kitchensync descendant cannot reinclude", reinclude_builtins, entry(".kitchensync/snapshot.db")),
                ignored=True,
                rule_kind="always_builtin",
            )
            assert_match(
                failures,
                "05 symlink cannot be re-included",
                match_result(rpc, failures, "05 symlink", reinclude_builtins, entry("link", "symlink")),
                ignored=True,
                rule_kind="always_builtin",
            )
            assert_match(
                failures,
                "05 special entry cannot be re-included",
                match_result(rpc, failures, "05 special", reinclude_builtins, entry("sock", "special")),
                ignored=True,
                rule_kind="always_builtin",
            )
            custom_options = {
                "always_excluded_directory_names": [".kitchensync", "guard"],
                "default_excluded_directory_names": [".git", "cache"],
                "ignore_symlinks": False,
                "ignore_special_entries": False,
            }
            custom = compile_matcher(
                rpc,
                failures,
                "05 compile custom options",
                [layer("", "!guard/\n!cache/\nlink\nsock\n", "custom-options")],
                custom_options,
            )
            assert_match(
                failures,
                "05 custom always-excluded directory",
                match_result(rpc, failures, "05 custom always", custom, entry("guard", "directory")),
                ignored=True,
                rule_kind="always_builtin",
            )
            assert_match(
                failures,
                "05 custom always-excluded descendant",
                match_result(rpc, failures, "05 custom always descendant", custom, entry("guard/file.txt")),
                ignored=True,
                rule_kind="always_builtin",
            )
            assert_match(
                failures,
                "05 custom default-excluded directory can be re-included",
                match_result(rpc, failures, "05 custom default", custom, entry("cache", "directory")),
                ignored=False,
                rule_kind="pattern",
                negated=True,
                pattern="!cache/",
            )
            assert_match(
                failures,
                "05 custom default-excluded descendant can be re-included",
                match_result(rpc, failures, "05 custom default descendant", custom, entry("cache/file.txt")),
                ignored=False,
                rule_kind="pattern",
                negated=True,
                pattern="!cache/",
            )
            assert_match(
                failures,
                "05 symlink option allows normal pattern matching",
                match_result(rpc, failures, "05 custom symlink pattern", custom, entry("link", "symlink")),
                ignored=True,
                rule_kind="pattern",
                pattern="link",
                source_name="custom-options",
                line_number=3,
            )
            assert_match(
                failures,
                "05 symlink option can allow unmatched symlink entries",
                match_result(rpc, failures, "05 custom symlink allowed", custom, entry("free-link", "symlink")),
                ignored=False,
                rule_kind="none",
            )
            assert_match(
                failures,
                "05 special option allows normal pattern matching",
                match_result(rpc, failures, "05 custom special pattern", custom, entry("sock", "special")),
                ignored=True,
                rule_kind="pattern",
                pattern="sock",
                source_name="custom-options",
                line_number=4,
            )
            assert_match(
                failures,
                "05 special option can allow unmatched special entries",
                match_result(rpc, failures, "05 custom special allowed", custom, entry("free-sock", "special")),
                ignored=False,
                rule_kind="none",
            )
            print("[05] built-in exclusions, custom options, and overridable .git behavior exercised")

            reinclude_directory = compile_matcher(
                rpc,
                failures,
                "06 compile directory reinclude matcher",
                [layer("", "build/\n!build/\n", "directory-reinclude")],
            )
            assert_match(
                failures,
                "06 directory re-inclusion applies to descendants",
                match_result(rpc, failures, "06 directory descendant reinclude", reinclude_directory, entry("build/keep.txt")),
                ignored=False,
                rule_kind="pattern",
                negated=True,
                pattern="!build/",
                source_name="directory-reinclude",
                line_number=2,
            )
            blocked_descendant = compile_matcher(
                rpc,
                failures,
                "06 compile blocked descendant matcher",
                [layer("", "build/\n!build/keep.txt\n", "blocked-descendant")],
            )
            assert_match(
                failures,
                "06 descendant negation cannot bypass ignored parent directory",
                match_result(rpc, failures, "06 blocked descendant", blocked_descendant, entry("build/keep.txt")),
                ignored=True,
                rule_kind="pattern",
            )
            base = compile_matcher(rpc, failures, "06 compile base matcher", [layer("", "*.tmp\n", "base")])
            extended = extend_matcher(rpc, failures, "06 extend matcher", base, layer("docs", "!keep.tmp\n", "extension"))
            assert_match(
                failures,
                "06 original matcher remains unchanged",
                match_result(rpc, failures, "06 original immutable", base, entry("docs/keep.tmp")),
                ignored=True,
                rule_kind="pattern",
                pattern="*.tmp",
                source_name="base",
            )
            assert_match(
                failures,
                "06 extended matcher includes new layer",
                match_result(rpc, failures, "06 extended", extended, entry("docs/keep.tmp")),
                ignored=False,
                rule_kind="pattern",
                negated=True,
                pattern="!keep.tmp",
                source_name="extension",
            )
            print("[06] extend returns a new matcher without mutating the original")

            invalid_base_paths = ["/bad", "bad/", "bad//path", "bad/./path", "bad/../path", r"bad\path", "bad\x00path"]
            for invalid_path in invalid_base_paths:
                expect_error_category(
                    failures,
                    f"07 invalid base path {invalid_path!r}",
                    rpc.call_tool("compile-matcher", {"layers": [layer(invalid_path, "*.tmp\n", "bad")], "options": DEFAULT_OPTIONS}),
                    "invalid_path",
                )
            invalid_entry_paths = ["", "/bad", "bad/", "bad//path", "bad/./path", "bad/../path", r"bad\path", "bad\x00path"]
            for invalid_path in invalid_entry_paths:
                expect_error_category(
                    failures,
                    f"07 invalid entry path {invalid_path!r}",
                    rpc.call_tool("match-entry", {"matcher_id": patterns or "", "entry": entry(invalid_path)}),
                    "invalid_path",
                )
            invalid_option_names = ["", "bad/name", r"bad\name", ".", "..", "bad\0name"]
            for invalid_name in invalid_option_names:
                expect_error_category(
                    failures,
                    f"07 invalid always option name {invalid_name!r}",
                    rpc.call_tool(
                        "compile-matcher",
                        {
                            "layers": [],
                            "options": {"always_excluded_directory_names": [invalid_name]},
                        },
                    ),
                    "invalid_options",
                )
                expect_error_category(
                    failures,
                    f"07 invalid default option name {invalid_name!r}",
                    rpc.call_tool(
                        "compile-matcher",
                        {
                            "layers": [],
                            "options": {"default_excluded_directory_names": [invalid_name]},
                        },
                    ),
                    "invalid_options",
                )
            expect_error_category(
                failures,
                "07 NUL pattern text",
                rpc.call_tool("compile-matcher", {"layers": [layer("", "bad\0pattern\n", "bad")], "options": DEFAULT_OPTIONS}),
                "invalid_pattern_text",
            )
            # Text that cannot be represented by the host language API is not
            # reasonably testable through this Python/JSON public surface.
            print("[07] invalid inputs report specified error categories")

            malformed = compile_matcher(rpc, failures, "08 malformed bracket compile", [layer("", "[abc\n", "malformed")])
            assert_match(
                failures,
                "08 malformed bracket literal",
                match_result(rpc, failures, "08 malformed bracket literal", malformed, entry("[abc")),
                ignored=True,
                rule_kind="pattern",
                pattern="[abc",
                source_name="malformed",
                line_number=1,
            )
            print("[08] malformed bracket expressions are literal patterns")

            run_concurrent_matches(port, filter_matcher, failures)
            print("[09] concurrent match calls completed")

            time.sleep(0.1)
            extra_stdout = [line for line in stdout_lines[stdout_before:] if not line.startswith("MCP_PORT=")]
            extra_stderr = stderr_lines[stderr_before:]
            if extra_stdout:
                failures.append(f"10: public operations must not write stdout, got {extra_stdout!r}")
            if extra_stderr:
                failures.append(f"10: public operations must not write stderr, got {extra_stderr!r}")
            print("[10] public operations produced no stdout/stderr")

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
