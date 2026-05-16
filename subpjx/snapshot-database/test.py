#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercise the snapshot database through its MCP wrapper."""

from __future__ import annotations

import json
import os
import re
import shutil
import socket
import sqlite3
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path
from typing import Any


BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

REQUIRED_TOOLS = {
    "close-database",
    "confirm-copy-completed",
    "generate-timestamps",
    "has-rows",
    "lookup",
    "mark-absent",
    "mark-displaced",
    "open-database",
    "path-id",
    "purge",
    "record-copy-pending",
    "record-present",
    "root-parent-id",
}

TIME_RE = re.compile(r"^\d{4}-\d{2}-\d{2}_\d{2}-\d{2}-\d{2}_\d{6}Z$")

MASK64 = (1 << 64) - 1
XXH_PRIME_1 = 11400714785074694791
XXH_PRIME_2 = 14029467366897019727
XXH_PRIME_3 = 1609587929392839161
XXH_PRIME_4 = 9650029242287828579
XXH_PRIME_5 = 2870177450012600261
BASE62 = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"


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


def rotl(value: int, bits: int) -> int:
    return ((value << bits) | (value >> (64 - bits))) & MASK64


def xxh_round(acc: int, lane: int) -> int:
    acc = (acc + lane * XXH_PRIME_2) & MASK64
    acc = rotl(acc, 31)
    return (acc * XXH_PRIME_1) & MASK64


def xxh64(data: bytes) -> int:
    length = len(data)
    index = 0
    if length >= 32:
        v1 = (XXH_PRIME_1 + XXH_PRIME_2) & MASK64
        v2 = XXH_PRIME_2
        v3 = 0
        v4 = (-XXH_PRIME_1) & MASK64
        limit = length - 32
        while index <= limit:
            v1 = xxh_round(v1, int.from_bytes(data[index:index + 8], "little"))
            v2 = xxh_round(v2, int.from_bytes(data[index + 8:index + 16], "little"))
            v3 = xxh_round(v3, int.from_bytes(data[index + 16:index + 24], "little"))
            v4 = xxh_round(v4, int.from_bytes(data[index + 24:index + 32], "little"))
            index += 32
        h = (rotl(v1, 1) + rotl(v2, 7) + rotl(v3, 12) + rotl(v4, 18)) & MASK64
        for v in (v1, v2, v3, v4):
            h ^= xxh_round(0, v)
            h = (h * XXH_PRIME_1 + XXH_PRIME_4) & MASK64
    else:
        h = XXH_PRIME_5

    h = (h + length) & MASK64
    while index + 8 <= length:
        k1 = xxh_round(0, int.from_bytes(data[index:index + 8], "little"))
        h ^= k1
        h = (rotl(h, 27) * XXH_PRIME_1 + XXH_PRIME_4) & MASK64
        index += 8
    if index + 4 <= length:
        h ^= (int.from_bytes(data[index:index + 4], "little") * XXH_PRIME_1) & MASK64
        h = (rotl(h, 23) * XXH_PRIME_2 + XXH_PRIME_3) & MASK64
        index += 4
    while index < length:
        h ^= (data[index] * XXH_PRIME_5) & MASK64
        h = (rotl(h, 11) * XXH_PRIME_1) & MASK64
        index += 1

    h ^= h >> 33
    h = (h * XXH_PRIME_2) & MASK64
    h ^= h >> 29
    h = (h * XXH_PRIME_3) & MASK64
    h ^= h >> 32
    return h & MASK64


def path_id_expected(relative_path: str) -> str:
    value = xxh64(relative_path.encode("utf-8"))
    encoded = ""
    if value == 0:
        encoded = "0"
    while value:
        value, digit = divmod(value, 62)
        encoded = BASE62[digit] + encoded
    return encoded.rjust(11, "0")


def success(failures: list[str], label: str, response: dict[str, Any]) -> dict[str, Any]:
    if "error" in response:
        failures.append(f"{label}: expected success, got {response['error']}")
        return {}
    result = response.get("result")
    if not isinstance(result, dict):
        failures.append(f"{label}: expected object result, got {response}")
        return {}
    return result


def error_category(response: dict[str, Any]) -> str:
    error = response.get("error")
    if not isinstance(error, dict):
        return ""
    data = error.get("data")
    if isinstance(data, dict) and isinstance(data.get("category"), str):
        return data["category"]
    text = " ".join(str(error.get(key, "")) for key in ("message", "data"))
    for category in (
        "invalid_path",
        "invalid_timestamp",
        "invalid_metadata",
        "not_found",
        "database_error",
    ):
        if category in text:
            return category
    return ""


def expect_error(failures: list[str], label: str, response: dict[str, Any], category: str) -> None:
    error = response.get("error")
    if not isinstance(error, dict):
        failures.append(f"{label}: expected {category} error, got {response}")
        return
    actual = error_category(response)
    if actual != category:
        failures.append(f"{label}: expected error category {category}, got {response}")
    if "result" in response:
        failures.append(f"{label}: error response must not contain a partial result, got {response}")


def require_tools(failures: list[str], response: dict[str, Any]) -> dict[str, dict[str, Any]]:
    result = success(failures, "01 tools/list", response)
    tools = result.get("tools")
    if not isinstance(tools, list):
        failures.append(f"01 tools/list: expected tools array, got {result}")
        return {}

    by_name: dict[str, dict[str, Any]] = {}
    for tool in tools:
        if not isinstance(tool, dict):
            failures.append(f"01 tools/list: tool must be an object, got {tool!r}")
            continue
        name = tool.get("name")
        if not isinstance(name, str):
            failures.append(f"01 tools/list: tool name must be a string, got {tool!r}")
            continue
        by_name[name] = tool

    missing = sorted(REQUIRED_TOOLS - set(by_name))
    if missing:
        failures.append(f"01 tools/list: missing tools: {', '.join(missing)}")
    return by_name


def metadata(kind: str, mod_time: str, byte_size: int) -> dict[str, Any]:
    return {"kind": kind, "mod_time": mod_time, "byte_size": byte_size}


def open_database(rpc: RpcClient, failures: list[str], db_file: Path) -> str:
    result = success(failures, "02 open-database", rpc.call_tool("open-database", {"db_file": str(db_file)}))
    database_id = result.get("database_id")
    if not isinstance(database_id, str) or not database_id:
        failures.append(f"02 open-database: expected non-empty database_id, got {result}")
        return "__missing_database_id__"
    return database_id


def lookup(rpc: RpcClient, failures: list[str], database_id: str, path: str, label: str) -> dict[str, Any] | None:
    result = success(
        failures,
        label,
        rpc.call_tool("lookup", {"database_id": database_id, "relative_path": path}),
    )
    row = result.get("row")
    if row is not None and not isinstance(row, dict):
        failures.append(f"{label}: expected row object or null, got {result}")
        return None
    return row


def check_row(
    failures: list[str],
    label: str,
    row: dict[str, Any] | None,
    expected: dict[str, Any],
) -> None:
    if row is None:
        failures.append(f"{label}: expected row, got absent")
        return
    for key, value in expected.items():
        if row.get(key) != value:
            failures.append(f"{label}: expected {key}={value!r}, got row {row}")
    if row.get("id") != path_id_expected(str(row.get("relative_path", ""))):
        failures.append(f"{label}: id does not match relative_path hash: {row}")
    deleted_time = row.get("deleted_time")
    if deleted_time is not None and not isinstance(deleted_time, str):
        failures.append(f"{label}: deleted_time must be string or null, got {row}")


def inspect_schema(failures: list[str], db_file: Path) -> None:
    try:
        con = sqlite3.connect(str(db_file))
        try:
            tables = sorted(
                row[0]
                for row in con.execute(
                    "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'"
                )
            )
            if tables != ["snapshot"]:
                failures.append(f"02 schema: expected exactly one snapshot table, got {tables}")

            views = sorted(row[0] for row in con.execute("SELECT name FROM sqlite_master WHERE type='view'"))
            if views:
                failures.append(f"02 schema: expected no snapshot views or aliases, got {views}")

            columns = [
                (row[1], row[2].upper(), bool(row[3]), bool(row[5]))
                for row in con.execute("PRAGMA table_info(snapshot)")
            ]
            expected_columns = [
                ("id", "TEXT", False, True),
                ("parent_id", "TEXT", True, False),
                ("basename", "TEXT", True, False),
                ("mod_time", "TEXT", True, False),
                ("byte_size", "INTEGER", True, False),
                ("last_seen", "TEXT", False, False),
                ("deleted_time", "TEXT", False, False),
            ]
            if columns != expected_columns:
                failures.append(f"02 schema: unexpected snapshot columns {columns}")

            indexes = sorted(
                row[1]
                for row in con.execute("PRAGMA index_list(snapshot)")
                if not str(row[1]).startswith("sqlite_autoindex_")
            )
            expected_indexes = [
                "snapshot_deleted_time_idx",
                "snapshot_last_seen_idx",
                "snapshot_parent_id_idx",
            ]
            if indexes != expected_indexes:
                failures.append(f"02 schema: expected indexes {expected_indexes}, got {indexes}")
            expected_index_columns = {
                "snapshot_parent_id_idx": ["parent_id"],
                "snapshot_last_seen_idx": ["last_seen"],
                "snapshot_deleted_time_idx": ["deleted_time"],
            }
            for index_name, expected_columns_for_index in expected_index_columns.items():
                actual_columns = [row[2] for row in con.execute(f"PRAGMA index_info({index_name})")]
                if actual_columns != expected_columns_for_index:
                    failures.append(
                        f"02 schema: expected {index_name} on {expected_columns_for_index}, got {actual_columns}"
                    )

            journal_mode = con.execute("PRAGMA journal_mode").fetchone()[0]
            if str(journal_mode).lower() in {"wal", "memory", "off"}:
                failures.append(f"02 schema: expected rollback journal mode, got {journal_mode}")

            # Not reasonably testable here: SQLite foreign_keys is
            # connection-local, and this schema has no foreign-key constraints
            # to exercise through the public API.
        finally:
            con.close()
    except sqlite3.Error as exc:
        failures.append(f"02 schema: sqlite inspection failed: {exc}")


def table_rows(db_file: Path) -> list[tuple[Any, ...]]:
    con = sqlite3.connect(str(db_file))
    try:
        return list(con.execute("SELECT id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time FROM snapshot"))
    finally:
        con.close()


def assert_path_ids(rpc: RpcClient, failures: list[str]) -> None:
    examples = {
        "/": "JyBskcNRrBK",
        "docs": "H41WPg3SlMv",
        "docs/readme.txt": "K5EzsWuLZ04",
        "pad-13": "0sShSI1uSxK",
    }
    root = success(failures, "03 root-parent-id", rpc.call_tool("root-parent-id", {})).get("id")
    if root != examples["/"]:
        failures.append(f"03 root-parent-id: expected {examples['/']}, got {root!r}")

    for path, expected in examples.items():
        if path == "/":
            continue
        result = success(failures, f"03 path-id {path}", rpc.call_tool("path-id", {"relative_path": path}))
        if result.get("id") != expected:
            failures.append(f"03 path-id {path}: expected {expected}, got {result}")
        if result.get("id") != path_id_expected(path):
            failures.append(f"03 path-id {path}: result does not match test xxHash64 implementation")
    utf8_path = "caf\u00e9.txt"
    utf8_result = success(
        failures,
        "03 path-id UTF-8 bytes",
        rpc.call_tool("path-id", {"relative_path": utf8_path}),
    )
    if utf8_result.get("id") != path_id_expected(utf8_path):
        failures.append(f"03 path-id UTF-8 bytes: result does not match UTF-8 xxHash64/base62")
    print("[03] path IDs match spec examples, xxHash64/base62, and zero padding")


def main() -> int:
    failures: list[str] = []
    tmp = Path(tempfile.mkdtemp(prefix="snapshot-db-test-"))
    proc: subprocess.Popen[str] | None = None
    port = 0
    try:
        db_file = tmp / "snapshot.db"
        if db_file.exists():
            db_file.unlink()

        proc, port, stdout_lines, stderr_lines = launch_mcp()
        with socket.create_connection(("127.0.0.1", port), timeout=10) as sock:
            rpc = RpcClient(sock)

            tools = require_tools(failures, rpc.request("tools/list"))
            print(f"[01] tools/list returned {len(tools)} tool(s)")

            database_id = open_database(rpc, failures, db_file)
            if not db_file.exists():
                failures.append("02 open-database: missing database file was not created")
            inspect_schema(failures, db_file)
            has_rows = success(
                failures,
                "02 has-rows empty database",
                rpc.call_tool("has-rows", {"database_id": database_id}),
            ).get("has_rows")
            if has_rows is not False:
                failures.append(f"02 has-rows: expected false for new database, got {has_rows!r}")
            print("[02] opening a missing database creates the schema")

            assert_path_ids(rpc, failures)

            invalid_cases = [
                ("04 empty path", "path-id", {"relative_path": ""}, "invalid_path"),
                ("04 leading slash", "path-id", {"relative_path": "/docs"}, "invalid_path"),
                ("04 root path", "path-id", {"relative_path": "/"}, "invalid_path"),
                ("04 trailing slash", "path-id", {"relative_path": "docs/"}, "invalid_path"),
                ("04 empty segment", "path-id", {"relative_path": "docs//readme.txt"}, "invalid_path"),
                ("04 NUL path", "path-id", {"relative_path": "bad\u0000name"}, "invalid_path"),
                (
                    "04 invalid path write",
                    "record-present",
                    {
                        "database_id": database_id,
                        "relative_path": "/bad-write.txt",
                        "metadata": metadata("file", "2026-05-15_10-00-00_000000Z", 1),
                        "seen_at": "2026-05-15_10-00-01_000000Z",
                    },
                    "invalid_path",
                ),
                (
                    "04 invalid timestamp",
                    "record-present",
                    {
                        "database_id": database_id,
                        "relative_path": "bad-time.txt",
                        "metadata": metadata("file", "2026-05-15T10:00:00Z", 1),
                        "seen_at": "2026-05-15_10-00-00_000000Z",
                    },
                    "invalid_timestamp",
                ),
                (
                    "04 invalid seen_at timestamp",
                    "record-present",
                    {
                        "database_id": database_id,
                        "relative_path": "bad-seen-time.txt",
                        "metadata": metadata("file", "2026-05-15_10-00-00_000000Z", 1),
                        "seen_at": "2026-05-15T10:00:01Z",
                    },
                    "invalid_timestamp",
                ),
                (
                    "04 invalid copy-pending timestamp",
                    "record-copy-pending",
                    {
                        "database_id": database_id,
                        "relative_path": "bad-copy-time.txt",
                        "metadata": metadata("file", "2026-05-15T10:00:00Z", 1),
                    },
                    "invalid_timestamp",
                ),
                (
                    "04 invalid purge cutoff timestamp",
                    "purge",
                    {"database_id": database_id, "cutoff_time": "2026-05-15T10:00:00Z"},
                    "invalid_timestamp",
                ),
                (
                    "04 invalid calendar timestamp",
                    "record-present",
                    {
                        "database_id": database_id,
                        "relative_path": "bad-calendar.txt",
                        "metadata": metadata("file", "2026-02-30_10-00-00_000000Z", 1),
                        "seen_at": "2026-05-15_10-00-00_000000Z",
                    },
                    "invalid_timestamp",
                ),
                (
                    "04 invalid file metadata",
                    "record-present",
                    {
                        "database_id": database_id,
                        "relative_path": "bad-file.txt",
                        "metadata": metadata("file", "2026-05-15_10-00-00_000000Z", -1),
                        "seen_at": "2026-05-15_10-00-01_000000Z",
                    },
                    "invalid_metadata",
                ),
                (
                    "04 invalid directory metadata",
                    "record-present",
                    {
                        "database_id": database_id,
                        "relative_path": "bad-dir",
                        "metadata": metadata("directory", "2026-05-15_10-00-00_000000Z", 0),
                        "seen_at": "2026-05-15_10-00-01_000000Z",
                    },
                    "invalid_metadata",
                ),
                (
                    "04 missing metadata field",
                    "record-present",
                    {
                        "database_id": database_id,
                        "relative_path": "missing-metadata.txt",
                        "metadata": {"kind": "file", "mod_time": "2026-05-15_10-00-00_000000Z"},
                        "seen_at": "2026-05-15_10-00-01_000000Z",
                    },
                    "invalid_metadata",
                ),
            ]
            for label, tool, args, category in invalid_cases:
                expect_error(failures, label, rpc.call_tool(tool, args), category)
            missing_parent_db = tmp / "missing-parent" / "snapshot.db"
            expect_error(
                failures,
                "04 database open missing parent",
                rpc.call_tool("open-database", {"db_file": str(missing_parent_db)}),
                "database_error",
            )
            if missing_parent_db.parent.exists():
                failures.append("04 database open missing parent: open-database created the parent directory")
            has_rows = success(
                failures,
                "04 has-rows after invalid writes",
                rpc.call_tool("has-rows", {"database_id": database_id}),
            ).get("has_rows")
            if has_rows is not False:
                failures.append(f"04 invalid inputs: expected no rows written, got has_rows={has_rows!r}")
            print("[04] invalid paths, timestamps, and metadata are rejected without rows")

            expect_success = success
            expect_success(
                failures,
                "05 record-present file",
                rpc.call_tool(
                    "record-present",
                    {
                        "database_id": database_id,
                        "relative_path": "docs/readme.txt",
                        "metadata": metadata("file", "2026-05-15_10-00-00_000000Z", 12),
                        "seen_at": "2026-05-15_10-00-05_000000Z",
                    },
                ),
            )
            row = lookup(rpc, failures, database_id, "docs/readme.txt", "05 lookup inserted file")
            check_row(
                failures,
                "05 inserted file",
                row,
                {
                    "id": "K5EzsWuLZ04",
                    "parent_id": "H41WPg3SlMv",
                    "relative_path": "docs/readme.txt",
                    "basename": "readme.txt",
                    "kind": "file",
                    "mod_time": "2026-05-15_10-00-00_000000Z",
                    "byte_size": 12,
                    "last_seen": "2026-05-15_10-00-05_000000Z",
                    "deleted_time": None,
                },
            )
            has_rows = success(
                failures,
                "05 has-rows populated database",
                rpc.call_tool("has-rows", {"database_id": database_id}),
            ).get("has_rows")
            if has_rows is not True:
                failures.append(f"05 has-rows: expected true after insert, got {has_rows!r}")
            expect_success(
                failures,
                "05 mark-absent readme",
                rpc.call_tool("mark-absent", {"database_id": database_id, "relative_path": "docs/readme.txt"}),
            )
            expect_success(
                failures,
                "05 record-present clears tombstone",
                rpc.call_tool(
                    "record-present",
                    {
                        "database_id": database_id,
                        "relative_path": "docs/readme.txt",
                        "metadata": metadata("file", "2026-05-15_10-02-00_000000Z", 99),
                        "seen_at": "2026-05-15_10-02-05_000000Z",
                    },
                ),
            )
            row = lookup(rpc, failures, database_id, "docs/readme.txt", "05 lookup updated file")
            check_row(
                failures,
                "05 updated file",
                row,
                {
                    "mod_time": "2026-05-15_10-02-00_000000Z",
                    "byte_size": 99,
                    "last_seen": "2026-05-15_10-02-05_000000Z",
                    "deleted_time": None,
                },
            )
            expect_success(
                failures,
                "05 record-present directory",
                rpc.call_tool(
                    "record-present",
                    {
                        "database_id": database_id,
                        "relative_path": "docs",
                        "metadata": metadata("directory", "2026-05-15_10-03-00_000000Z", -1),
                        "seen_at": "2026-05-15_10-03-05_000000Z",
                    },
                ),
            )
            row = lookup(rpc, failures, database_id, "docs", "05 lookup directory")
            check_row(
                failures,
                "05 directory row",
                row,
                {
                    "id": "H41WPg3SlMv",
                    "parent_id": "JyBskcNRrBK",
                    "relative_path": "docs",
                    "basename": "docs",
                    "kind": "directory",
                    "mod_time": "2026-05-15_10-03-00_000000Z",
                    "byte_size": -1,
                    "last_seen": "2026-05-15_10-03-05_000000Z",
                    "deleted_time": None,
                },
            )
            expect_success(
                failures,
                "05 record-present zero-byte file",
                rpc.call_tool(
                    "record-present",
                    {
                        "database_id": database_id,
                        "relative_path": "empty.txt",
                        "metadata": metadata("file", "2026-05-15_10-04-00_000000Z", 0),
                        "seen_at": "2026-05-15_10-04-05_000000Z",
                    },
                ),
            )
            row = lookup(rpc, failures, database_id, "empty.txt", "05 lookup zero-byte file")
            check_row(
                failures,
                "05 zero-byte file",
                row,
                {
                    "kind": "file",
                    "mod_time": "2026-05-15_10-04-00_000000Z",
                    "byte_size": 0,
                    "last_seen": "2026-05-15_10-04-05_000000Z",
                    "deleted_time": None,
                },
            )
            print("[05] record-present inserts, updates, clears tombstones, and stores directories")

            expect_success(
                failures,
                "06 record-present pending source",
                rpc.call_tool(
                    "record-present",
                    {
                        "database_id": database_id,
                        "relative_path": "pending.bin",
                        "metadata": metadata("file", "2026-05-15_11-00-00_000000Z", 5),
                        "seen_at": "2026-05-15_11-00-01_000000Z",
                    },
                ),
            )
            expect_success(
                failures,
                "06 mark pending source absent",
                rpc.call_tool("mark-absent", {"database_id": database_id, "relative_path": "pending.bin"}),
            )
            expect_success(
                failures,
                "06 record-copy-pending updates metadata",
                rpc.call_tool(
                    "record-copy-pending",
                    {
                        "database_id": database_id,
                        "relative_path": "pending.bin",
                        "metadata": metadata("file", "2026-05-15_11-02-00_000000Z", 7),
                    },
                ),
            )
            row = lookup(rpc, failures, database_id, "pending.bin", "06 lookup pending update")
            check_row(
                failures,
                "06 pending update",
                row,
                {
                    "mod_time": "2026-05-15_11-02-00_000000Z",
                    "byte_size": 7,
                    "last_seen": "2026-05-15_11-00-01_000000Z",
                    "deleted_time": None,
                },
            )
            expect_success(
                failures,
                "06 record-copy-pending insert",
                rpc.call_tool(
                    "record-copy-pending",
                    {
                        "database_id": database_id,
                        "relative_path": "new-pending.bin",
                        "metadata": metadata("file", "2026-05-15_11-03-00_000000Z", 8),
                    },
                ),
            )
            row = lookup(rpc, failures, database_id, "new-pending.bin", "06 lookup pending insert")
            check_row(
                failures,
                "06 pending insert",
                row,
                {
                    "parent_id": "JyBskcNRrBK",
                    "relative_path": "new-pending.bin",
                    "basename": "new-pending.bin",
                    "kind": "file",
                    "mod_time": "2026-05-15_11-03-00_000000Z",
                    "byte_size": 8,
                    "last_seen": None,
                    "deleted_time": None,
                },
            )
            print("[06] record-copy-pending preserves last_seen and clears tombstones")

            expect_success(
                failures,
                "07 confirm-copy-completed",
                rpc.call_tool(
                    "confirm-copy-completed",
                    {
                        "database_id": database_id,
                        "relative_path": "pending.bin",
                        "seen_at": "2026-05-15_11-05-00_000000Z",
                    },
                ),
            )
            row = lookup(rpc, failures, database_id, "pending.bin", "07 lookup completed copy")
            check_row(
                failures,
                "07 completed copy",
                row,
                {
                    "mod_time": "2026-05-15_11-02-00_000000Z",
                    "byte_size": 7,
                    "last_seen": "2026-05-15_11-05-00_000000Z",
                    "deleted_time": None,
                },
            )
            before_missing_confirm = lookup(
                rpc,
                failures,
                database_id,
                "pending.bin",
                "07 lookup before missing confirm",
            )
            expect_error(
                failures,
                "07 confirm-copy-completed missing",
                rpc.call_tool(
                    "confirm-copy-completed",
                    {
                        "database_id": database_id,
                        "relative_path": "missing.bin",
                        "seen_at": "2026-05-15_11-05-00_000000Z",
                    },
                ),
                "not_found",
            )
            after_missing_confirm = lookup(
                rpc,
                failures,
                database_id,
                "pending.bin",
                "07 lookup after missing confirm",
            )
            if before_missing_confirm != after_missing_confirm:
                failures.append(
                    "07 confirm-copy-completed missing: failed transaction changed an existing row "
                    f"from {before_missing_confirm} to {after_missing_confirm}"
                )

            expect_success(
                failures,
                "07 record row before tombstoned completion",
                rpc.call_tool(
                    "record-present",
                    {
                        "database_id": database_id,
                        "relative_path": "tombstoned-complete.bin",
                        "metadata": metadata("file", "2026-05-15_11-06-00_000000Z", 6),
                        "seen_at": "2026-05-15_11-06-00_000000Z",
                    },
                ),
            )
            expect_success(
                failures,
                "07 tombstone row before completion",
                rpc.call_tool(
                    "mark-absent",
                    {"database_id": database_id, "relative_path": "tombstoned-complete.bin"},
                ),
            )
            expect_success(
                failures,
                "07 confirm-copy-completed only last_seen",
                rpc.call_tool(
                    "confirm-copy-completed",
                    {
                        "database_id": database_id,
                        "relative_path": "tombstoned-complete.bin",
                        "seen_at": "2026-05-15_11-07-00_000000Z",
                    },
                ),
            )
            row = lookup(
                rpc,
                failures,
                database_id,
                "tombstoned-complete.bin",
                "07 lookup tombstoned completion",
            )
            check_row(
                failures,
                "07 completed copy preserves non-last_seen fields",
                row,
                {
                    "mod_time": "2026-05-15_11-06-00_000000Z",
                    "byte_size": 6,
                    "last_seen": "2026-05-15_11-07-00_000000Z",
                    "deleted_time": "2026-05-15_11-06-00_000000Z",
                },
            )
            print("[07] confirm-copy-completed updates last_seen and reports not_found")

            expect_success(
                failures,
                "08 mark-absent",
                rpc.call_tool("mark-absent", {"database_id": database_id, "relative_path": "pending.bin"}),
            )
            first_absent = lookup(rpc, failures, database_id, "pending.bin", "08 lookup absent")
            expect_success(
                failures,
                "08 mark-absent idempotent",
                rpc.call_tool("mark-absent", {"database_id": database_id, "relative_path": "pending.bin"}),
            )
            second_absent = lookup(rpc, failures, database_id, "pending.bin", "08 lookup absent again")
            check_row(
                failures,
                "08 absent tombstone",
                second_absent,
                {
                    "last_seen": "2026-05-15_11-05-00_000000Z",
                    "deleted_time": "2026-05-15_11-05-00_000000Z",
                },
            )
            if first_absent and second_absent and first_absent.get("deleted_time") != second_absent.get("deleted_time"):
                failures.append(f"08 mark-absent: tombstone estimate changed from {first_absent} to {second_absent}")
            expect_success(
                failures,
                "08 mark-absent missing",
                rpc.call_tool("mark-absent", {"database_id": database_id, "relative_path": "not-there.txt"}),
            )
            print("[08] mark-absent sets and preserves tombstones")

            other_db = success(
                failures,
                "09 open second database",
                rpc.call_tool("open-database", {"db_file": str(tmp / "other-snapshot.db")}),
            )
            other_database_id = other_db.get("database_id")
            if not isinstance(other_database_id, str) or not other_database_id:
                failures.append(f"09 open second database: expected non-empty database_id, got {other_db}")
                other_database_id = "__missing_other_database_id__"
            for path, kind, seen, size in [
                ("album", "directory", "2026-05-15_09-20-00_000000Z", -1),
                ("album/raw/a.jpg", "file", "2026-05-15_09-20-01_000000Z", 4),
            ]:
                expect_success(
                    failures,
                    f"09 record second database {path}",
                    rpc.call_tool(
                        "record-present",
                        {
                            "database_id": other_database_id,
                            "relative_path": path,
                            "metadata": metadata(kind, seen, size),
                            "seen_at": seen,
                        },
                    ),
                )

            for path, kind, seen, size in [
                ("album", "directory", "2026-05-15_09-00-00_000000Z", -1),
                ("album/raw", "directory", "2026-05-15_09-00-01_000000Z", -1),
                ("album/raw/a.jpg", "file", "2026-05-15_09-00-02_000000Z", 1),
                ("album/old-tombstone.jpg", "file", "2026-05-15_08-30-00_000000Z", 1),
                ("old.txt", "file", "2026-05-15_08-00-00_000000Z", 2),
                ("album-not-child/a.jpg", "file", "2026-05-15_09-00-03_000000Z", 3),
            ]:
                expect_success(
                    failures,
                    f"09 record {path}",
                    rpc.call_tool(
                        "record-present",
                        {
                            "database_id": database_id,
                            "relative_path": path,
                            "metadata": metadata(kind, seen, size),
                            "seen_at": seen,
                        },
                    ),
                )
            expect_success(
                failures,
                "09 mark preexisting descendant tombstone",
                rpc.call_tool("mark-absent", {"database_id": database_id, "relative_path": "album/old-tombstone.jpg"}),
            )
            expect_success(
                failures,
                "09 mark-displaced",
                rpc.call_tool("mark-displaced", {"database_id": database_id, "relative_path": "album"}),
            )
            for path in ("album", "album/raw", "album/raw/a.jpg"):
                row = lookup(rpc, failures, database_id, path, f"09 lookup {path}")
                check_row(failures, f"09 displaced {path}", row, {"deleted_time": "2026-05-15_09-00-00_000000Z"})
            for path in ("old.txt", "album-not-child/a.jpg"):
                row = lookup(rpc, failures, database_id, path, f"09 lookup unrelated {path}")
                check_row(failures, f"09 unrelated {path}", row, {"deleted_time": None})
            for path in ("album", "album/raw/a.jpg"):
                row = lookup(rpc, failures, other_database_id, path, f"09 lookup second database {path}")
                check_row(failures, f"09 second database {path}", row, {"deleted_time": None})
            row = lookup(rpc, failures, database_id, "album/old-tombstone.jpg", "09 lookup preexisting tombstone")
            check_row(
                failures,
                "09 preexisting descendant tombstone",
                row,
                {"deleted_time": "2026-05-15_08-30-00_000000Z"},
            )

            for path, kind, seen, size in [
                ("already-deleted", "directory", "2026-05-15_09-10-00_000000Z", -1),
                ("already-deleted/live-child.txt", "file", "2026-05-15_09-10-01_000000Z", 1),
            ]:
                expect_success(
                    failures,
                    f"09 record already tombstoned target {path}",
                    rpc.call_tool(
                        "record-present",
                        {
                            "database_id": database_id,
                            "relative_path": path,
                            "metadata": metadata(kind, seen, size),
                            "seen_at": seen,
                        },
                    ),
                )
            expect_success(
                failures,
                "09 tombstone displaced target before cascade",
                rpc.call_tool("mark-absent", {"database_id": database_id, "relative_path": "already-deleted"}),
            )
            expect_success(
                failures,
                "09 change tombstoned target last_seen before cascade",
                rpc.call_tool(
                    "confirm-copy-completed",
                    {
                        "database_id": database_id,
                        "relative_path": "already-deleted",
                        "seen_at": "2026-05-15_09-15-00_000000Z",
                    },
                ),
            )
            expect_success(
                failures,
                "09 mark-displaced already tombstoned target",
                rpc.call_tool("mark-displaced", {"database_id": database_id, "relative_path": "already-deleted"}),
            )
            for path in ("already-deleted", "already-deleted/live-child.txt"):
                row = lookup(rpc, failures, database_id, path, f"09 lookup already tombstoned cascade {path}")
                check_row(
                    failures,
                    f"09 already tombstoned cascade {path}",
                    row,
                    {"deleted_time": "2026-05-15_09-10-00_000000Z"},
                )
            before_missing_displace = lookup(
                rpc,
                failures,
                database_id,
                "old.txt",
                "09 lookup before missing displace",
            )
            expect_success(
                failures,
                "09 mark-displaced missing",
                rpc.call_tool("mark-displaced", {"database_id": database_id, "relative_path": "missing-displace"}),
            )
            after_missing_displace = lookup(
                rpc,
                failures,
                database_id,
                "old.txt",
                "09 lookup after missing displace",
            )
            if before_missing_displace != after_missing_displace:
                failures.append(
                    "09 mark-displaced missing: changed an existing row "
                    f"from {before_missing_displace} to {after_missing_displace}"
                )
            print("[09] mark-displaced cascades only to descendants")

            for path, seen in [
                ("old-live.txt", "2026-05-15_07-00-00_000000Z"),
                ("new-live.txt", "2026-05-15_12-00-00_000000Z"),
                ("old-deleted.txt", "2026-05-15_07-01-00_000000Z"),
                ("new-deleted.txt", "2026-05-15_12-01-00_000000Z"),
                ("cutoff-live.txt", "2026-05-15_10-00-00_000000Z"),
                ("cutoff-deleted.txt", "2026-05-15_10-00-00_000000Z"),
            ]:
                expect_success(
                    failures,
                    f"10 record {path}",
                    rpc.call_tool(
                        "record-present",
                        {
                            "database_id": database_id,
                            "relative_path": path,
                            "metadata": metadata("file", seen, 1),
                            "seen_at": seen,
                        },
                    ),
                )
            for path in ("old-deleted.txt", "new-deleted.txt", "cutoff-deleted.txt"):
                expect_success(
                    failures,
                    f"10 mark absent {path}",
                    rpc.call_tool("mark-absent", {"database_id": database_id, "relative_path": path}),
                )
            expect_success(
                failures,
                "10 absent last_seen row",
                rpc.call_tool(
                    "record-copy-pending",
                    {
                        "database_id": database_id,
                        "relative_path": "copy-without-last-seen.txt",
                        "metadata": metadata("file", "2026-05-15_12-02-00_000000Z", 1),
                    },
                ),
            )
            purge = expect_success(
                failures,
                "10 purge",
                rpc.call_tool("purge", {"database_id": database_id, "cutoff_time": "2026-05-15_10-00-00_000000Z"}),
            )
            if purge.get("deleted_count") != 12:
                failures.append(f"10 purge: expected deleted_count=12, got {purge}")
            expected_absent = [
                "new-pending.bin",
                "album",
                "album/raw",
                "album/raw/a.jpg",
                "album/old-tombstone.jpg",
                "already-deleted",
                "already-deleted/live-child.txt",
                "old.txt",
                "album-not-child/a.jpg",
                "old-live.txt",
                "old-deleted.txt",
                "copy-without-last-seen.txt",
            ]
            for path in expected_absent:
                if lookup(rpc, failures, database_id, path, f"10 lookup purged {path}") is not None:
                    failures.append(f"10 purge: expected {path} to be absent")
            for path in ("new-live.txt", "new-deleted.txt", "cutoff-live.txt", "cutoff-deleted.txt"):
                if lookup(rpc, failures, database_id, path, f"10 lookup preserved {path}") is None:
                    failures.append(f"10 purge: expected {path} to be preserved")
            print("[10] purge removes old tombstones, old live rows, and absent last_seen rows")

            times = success(
                failures,
                "11 generate-timestamps",
                rpc.call_tool(
                    "generate-timestamps",
                    {
                        "wall_clock_times": [
                            "2026-05-15_13-00-00_000000Z",
                            "2026-05-15_13-00-00_000000Z",
                            "2026-05-15_13-00-00_000000Z",
                            "2026-05-15_13-00-00_000002Z",
                        ]
                    },
                ),
            ).get("timestamps")
            expected_times = [
                "2026-05-15_13-00-00_000000Z",
                "2026-05-15_13-00-00_000001Z",
                "2026-05-15_13-00-00_000002Z",
                "2026-05-15_13-00-00_000003Z",
            ]
            if times != expected_times:
                failures.append(f"11 generate-timestamps: expected {expected_times}, got {times}")
            if not isinstance(times, list) or any(not isinstance(t, str) or not TIME_RE.fullmatch(t) for t in times):
                failures.append(f"11 generate-timestamps: timestamps must use SnapshotTime text, got {times}")
            print("[11] timestamp generator is strictly increasing when wall clock repeats")

            # Not reasonably testable here: forcing SQLite to fail after a
            # transaction begins would require sabotaging the database or disk.
            # Public validation and not_found failures are observable without
            # breaking the environment, so they must leave committed rows intact.
            before = lookup(rpc, failures, database_id, "docs/readme.txt", "12 lookup before failed validation")
            expect_error(
                failures,
                "12 invalid metadata write",
                rpc.call_tool(
                    "record-present",
                    {
                        "database_id": database_id,
                        "relative_path": "docs/readme.txt",
                        "metadata": metadata("file", "2026-05-15_14-00-00_000000Z", -9),
                        "seen_at": "2026-05-15_14-00-01_000000Z",
                    },
                ),
                "invalid_metadata",
            )
            after = lookup(rpc, failures, database_id, "docs/readme.txt", "12 lookup after failed validation")
            if before != after:
                failures.append(f"12 failed validation: row changed from {before} to {after}")
            print("[12] failed writes leave previous committed rows observable")

            expect_success(
                failures,
                "13 orphan child",
                rpc.call_tool(
                    "record-present",
                    {
                        "database_id": database_id,
                        "relative_path": "ghost/child.txt",
                        "metadata": metadata("file", "2026-05-15_06-00-00_000000Z", 1),
                        "seen_at": "2026-05-15_06-00-00_000000Z",
                    },
                ),
            )
            row = lookup(rpc, failures, database_id, "ghost/child.txt", "13 lookup orphan child")
            check_row(
                failures,
                "13 parent id from path",
                row,
                {"parent_id": path_id_expected("ghost"), "basename": "child.txt"},
            )
            purge = expect_success(
                failures,
                "13 purge orphan child",
                rpc.call_tool("purge", {"database_id": database_id, "cutoff_time": "2026-05-15_10-00-00_000000Z"}),
            )
            if purge.get("deleted_count") != 1:
                failures.append(f"13 purge orphan child: expected deleted_count=1, got {purge}")
            if lookup(rpc, failures, database_id, "ghost/child.txt", "13 lookup purged orphan child") is not None:
                failures.append("13 purge orphan child: expected orphan child to be absent")
            expect_error(
                failures,
                "13 root lookup invalid",
                rpc.call_tool("lookup", {"database_id": database_id, "relative_path": "/"}),
                "invalid_path",
            )
            print("[13] parent IDs come from paths, orphan rows purge, and the root is never a row")

            expect_success(
                failures,
                "14 close-database",
                rpc.call_tool("close-database", {"database_id": database_id}),
            )
            expect_success(
                failures,
                "14 close-database idempotent",
                rpc.call_tool("close-database", {"database_id": database_id}),
            )
            expect_success(
                failures,
                "14 close second database",
                rpc.call_tool("close-database", {"database_id": other_database_id}),
            )
            rows = table_rows(db_file)
            if any(row[0] == "JyBskcNRrBK" for row in rows):
                failures.append("14 root row: root sentinel ID was inserted into snapshot")
            if Path(str(db_file) + "-wal").exists():
                failures.append("14 rollback journal: WAL sidecar file exists")
            print("[14] close is idempotent and no root/WAL state is present")

            time.sleep(0.2)
            extra_stdout = [line for line in stdout_lines if not line.startswith("MCP_PORT=")]
            if extra_stdout:
                failures.append(f"15 public operations must not write stdout, got {extra_stdout!r}")
            if stderr_lines:
                failures.append(f"15 public operations must not write stderr, got {stderr_lines!r}")
            print("[15] public operations produced no stdout or stderr")

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
        shutil.rmtree(tmp, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
