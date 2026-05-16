#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

from __future__ import annotations

import base64
import json
import socket
import subprocess
import sys
import threading
import time
from pathlib import Path
from typing import Any


PROJECT_DIR = Path("/home/ace/Desktop/prjx/kitchensync/subpjx/staged-file-transfer")
JAVA = Path("/home/ace/Desktop/prjx/kitchensync/tools/compiler/jdk/bin/java")
MCP_JAR = PROJECT_DIR / "released/staged-file-transfer_MCP.jar"
MOD_TIME = "2026-05-15T10:30:00Z"


class McpClient:
    def __init__(self) -> None:
        self.proc: subprocess.Popen[str] | None = None
        self.port: int | None = None
        self.sock: socket.socket | None = None
        self.next_id = 1
        self.extra_stdout: list[str] = []
        self.stderr: list[str] = []

    def start(self) -> None:
        self.proc = subprocess.Popen(
            [str(JAVA), "-jar", str(MCP_JAR)],
            cwd=PROJECT_DIR,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            errors="replace",
        )
        if self.proc.stdout is None or self.proc.stderr is None:
            raise RuntimeError("MCP server pipes were not created")

        deadline = time.time() + 30
        while time.time() < deadline and self.proc.poll() is None:
            line = self.proc.stdout.readline()
            if line.startswith("MCP_PORT="):
                self.port = int(line.strip().split("=", 1)[1])
                break
            if line:
                self.extra_stdout.append(line)
        if self.port is None:
            stderr = self.proc.stderr.read()
            raise RuntimeError(
                f"MCP server did not advertise MCP_PORT; exit={self.proc.poll()} stderr={stderr!r}"
            )

        threading.Thread(
            target=self._drain, args=(self.proc.stdout, self.extra_stdout), daemon=True
        ).start()
        threading.Thread(target=self._drain, args=(self.proc.stderr, self.stderr), daemon=True).start()
        self.sock = socket.create_connection(("127.0.0.1", self.port), timeout=10)
        self.sock.settimeout(30)

    @staticmethod
    def _drain(stream: Any, sink: list[str]) -> None:
        for line in stream:
            sink.append(line)

    def rpc(self, method: str, params: dict[str, Any] | None = None) -> dict[str, Any]:
        if self.sock is None:
            raise RuntimeError("MCP socket is not connected")
        rpc_id = self.next_id
        self.next_id += 1
        request: dict[str, Any] = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
        if params is not None:
            request["params"] = params
        self.sock.sendall((json.dumps(request, separators=(",", ":")) + "\n").encode("utf-8"))
        data = b""
        deadline = time.time() + 30
        while b"\n" not in data and time.time() < deadline:
            chunk = self.sock.recv(65536)
            if not chunk:
                break
            data += chunk
        line, _, _ = data.partition(b"\n")
        if not line:
            raise RuntimeError(f"MCP server closed connection for {method}")
        return json.loads(line.decode("utf-8"))

    def call_tool(self, name: str, arguments: dict[str, Any]) -> dict[str, Any]:
        response = self.rpc("tools/call", {"name": name, "arguments": arguments})
        if "error" in response:
            raise RuntimeError(f"{name} failed: {response['error']}")
        result = response.get("result")
        if not isinstance(result, dict):
            raise RuntimeError(f"{name} returned non-object result: {response!r}")
        return result

    def close(self) -> None:
        if self.sock is not None:
            try:
                self.rpc("aitc/shutdown")
            except Exception:
                pass
            try:
                self.sock.close()
            except OSError:
                pass
        if self.proc is None:
            return
        try:
            self.proc.wait(timeout=5)
            return
        except subprocess.TimeoutExpired:
            pass
        self.proc.terminate()
        try:
            self.proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.proc.kill()
            self.proc.wait()


def file_entry(path: str, data: bytes, mod_time: str = "2026-05-15T00:00:00Z") -> dict[str, Any]:
    return {
        "path": path,
        "kind": "file",
        "mod_time": mod_time,
        "data_base64": base64.b64encode(data).decode("ascii"),
    }


def directory_entry(path: str, mod_time: str = "2026-05-15T00:00:00Z") -> dict[str, Any]:
    return {"path": path, "kind": "directory", "mod_time": mod_time}


def copy_args(
    source_entries: list[dict[str, Any]],
    destination_entries: list[dict[str, Any]],
    **overrides: Any,
) -> dict[str, Any]:
    args: dict[str, Any] = {
        "source_entries": source_entries,
        "destination_entries": destination_entries,
        "source_path": "album/a.bin",
        "destination_path": "album/a.bin",
        "winning_mod_time": MOD_TIME,
        "staging_timestamp": "2026-05-15_10-31-00_000001Z",
        "transfer_id": "123e4567-e89b-12d3-a456-426614174000",
        "chunk_size": 3,
        "channel_capacity": 1,
    }
    args.update(overrides)
    return args


def expect(condition: bool, failures: list[str], message: str) -> None:
    if not condition:
        failures.append(message)


def expect_result(
    output: dict[str, Any],
    failures: list[str],
    context: str,
    status: str,
    error: str | None = None,
) -> dict[str, Any]:
    result = output.get("result")
    expect(isinstance(result, dict), failures, f"{context}: expected object result, got {result!r}")
    if not isinstance(result, dict):
        return {}
    expect(result.get("status") == status, failures, f"{context}: expected status {status}, got {result}")
    if error is not None:
        expect(result.get("error") == error, failures, f"{context}: expected error {error}, got {result}")
    return result


def entry(entries: list[dict[str, Any]], path: str) -> dict[str, Any] | None:
    return next((item for item in entries if item.get("path") == path), None)


def file_bytes(entries: list[dict[str, Any]], path: str) -> bytes | None:
    item = entry(entries, path)
    if item is None or item.get("kind") != "file":
        return None
    return base64.b64decode(str(item.get("data_base64", "")))


def has_path(entries: list[dict[str, Any]], path: str) -> bool:
    return entry(entries, path) is not None


def result_list(result: dict[str, Any], field: str) -> list[Any]:
    value = result.get(field)
    return value if isinstance(value, list) else []


# The MCP wrapper exposes final entry snapshots and OperationResult fields, but
# not per-operation traces, task scheduling, chunk events, or injected backend
# failures for read, write, close, rename, set_mod_time, permission checks, or
# delete_dir. The SPEC's bounded-pipeline overlap/backpressure, no-full-buffer
# streaming, single-rename displacement, pre-rename backend failure cleanup,
# set_mod_time failure, and cleanup-continues-after-delete-failure scenarios are
# therefore not reasonably testable here.


def check_copy_behaviors(client: McpClient, failures: list[str]) -> None:
    data = b"\x00\xffhello\r\nbinary\x80\x81\x00"
    output = client.call_tool("copy-file", copy_args([file_entry("album/a.bin", data)], []))
    result = expect_result(output, failures, "copy binary into empty nested destination", "success")
    destination = output["destination_entries"]
    copied = entry(destination, "album/a.bin")
    expect(file_bytes(destination, "album/a.bin") == data, failures, "copy nested: binary bytes changed")
    expect(copied is not None and copied.get("mod_time") == MOD_TIME, failures, "copy nested: mod time wrong")
    expect(not has_path(destination, "album/.kitchensync/TMP/2026-05-15_10-31-00_000001Z/123e4567-e89b-12d3-a456-426614174000/a.bin"), failures, "copy nested: TMP file remained")
    expect(result.get("final_path") == "album/a.bin", failures, f"copy nested: wrong final_path, got {result}")
    expect(
        result.get("temporary_path")
        == "album/.kitchensync/TMP/2026-05-15_10-31-00_000001Z/123e4567-e89b-12d3-a456-426614174000/a.bin",
        failures,
        f"copy nested: wrong temporary_path, got {result}",
    )
    expect(
        "album/.kitchensync/TMP/2026-05-15_10-31-00_000001Z/123e4567-e89b-12d3-a456-426614174000"
        in result_list(result, "created_paths"),
        failures,
        f"copy nested: TMP parent was not reported as created, got {result}",
    )
    expect(not result.get("backup_path"), failures, f"copy nested: backup_path should be absent, got {result}")

    output = client.call_tool(
        "copy-file",
        copy_args(
            [file_entry("root.bin", b"root")],
            [],
            source_path="root.bin",
            destination_path="root.bin",
            staging_timestamp="2026-05-15_10-32-00_000001Z",
            transfer_id="123e4567-e89b-12d3-a456-426614174001",
        ),
    )
    result = expect_result(output, failures, "copy root-level destination", "success")
    expect(file_bytes(output["destination_entries"], "root.bin") == b"root", failures, "copy root-level: bytes wrong")
    expect(
        result.get("temporary_path")
        == ".kitchensync/TMP/2026-05-15_10-32-00_000001Z/123e4567-e89b-12d3-a456-426614174001/root.bin",
        failures,
        f"copy root-level: wrong temporary_path, got {result}",
    )

    output = client.call_tool(
        "copy-file",
        copy_args(
            [file_entry("same-source.bin", b"same filesystem")],
            [],
            same_filesystem=True,
            source_path="same-source.bin",
            destination_path="same-destination.bin",
            staging_timestamp="2026-05-15_10-33-00_000001Z",
            transfer_id="123e4567-e89b-12d3-a456-426614174002",
        ),
    )
    expect_result(output, failures, "copy on same filesystem to a different path", "success")
    expect(file_bytes(output["destination_entries"], "same-destination.bin") == b"same filesystem", failures, "same filesystem copy: bytes wrong")

    output = client.call_tool(
        "copy-file",
        copy_args(
            [file_entry("notes/todo.txt", b"new")],
            [file_entry("notes/todo.txt", b"old")],
            source_path="notes/todo.txt",
            destination_path="notes/todo.txt",
            staging_timestamp="2026-05-15_11-00-00_000002Z",
            transfer_id="123e4567-e89b-12d3-a456-426614174111",
            chunk_size=1,
            channel_capacity=2,
        ),
    )
    result = expect_result(output, failures, "copy over existing file", "success")
    destination = output["destination_entries"]
    expect(file_bytes(destination, "notes/todo.txt") == b"new", failures, "copy over file: final bytes wrong")
    expect(
        file_bytes(destination, "notes/.kitchensync/BAK/2026-05-15_11-00-00_000002Z/todo.txt") == b"old",
        failures,
        "copy over file: old destination was not moved to BAK",
    )
    expect(
        result.get("backup_path") == "notes/.kitchensync/BAK/2026-05-15_11-00-00_000002Z/todo.txt",
        failures,
        f"copy over file: wrong backup_path, got {result}",
    )

    output = client.call_tool(
        "copy-file",
        copy_args(
            [file_entry("replace-dir", b"file replacing directory")],
            [file_entry("replace-dir/child.txt", b"inside old directory")],
            source_path="replace-dir",
            destination_path="replace-dir",
            staging_timestamp="2026-05-15_11-01-00_000002Z",
            transfer_id="123e4567-e89b-12d3-a456-426614174112",
        ),
    )
    expect_result(output, failures, "copy over existing directory", "success")
    destination = output["destination_entries"]
    expect(file_bytes(destination, "replace-dir") == b"file replacing directory", failures, "copy over directory: final file not installed")
    expect(
        file_bytes(destination, ".kitchensync/BAK/2026-05-15_11-01-00_000002Z/replace-dir/child.txt")
        == b"inside old directory",
        failures,
        "copy over directory: existing subtree was not moved to BAK",
    )


def check_displace_and_cleanup(client: McpClient, failures: list[str]) -> None:
    output = client.call_tool(
        "displace",
        {
            "entries": [file_entry("docs/readme.txt", b"readme")],
            "path": "docs/readme.txt",
            "staging_timestamp": "2026-05-15_11-59-59_000003Z",
        },
    )
    result = expect_result(output, failures, "displace file", "success")
    entries = output["entries"]
    expect(not has_path(entries, "docs/readme.txt"), failures, "displace file: source still exists")
    expect(
        file_bytes(entries, "docs/.kitchensync/BAK/2026-05-15_11-59-59_000003Z/readme.txt") == b"readme",
        failures,
        "displace file: file was not moved to BAK path",
    )
    expect(
        result.get("backup_path") == "docs/.kitchensync/BAK/2026-05-15_11-59-59_000003Z/readme.txt",
        failures,
        f"displace file: wrong backup_path, got {result}",
    )

    output = client.call_tool(
        "displace",
        {
            "entries": [file_entry("album/raw/frame.dat", b"frame")],
            "path": "album/raw",
            "staging_timestamp": "2026-05-15_12-00-00_000003Z",
        },
    )
    result = expect_result(output, failures, "displace directory", "success")
    entries = output["entries"]
    expect(not has_path(entries, "album/raw"), failures, "displace directory: source still exists")
    expect(
        file_bytes(entries, "album/.kitchensync/BAK/2026-05-15_12-00-00_000003Z/raw/frame.dat") == b"frame",
        failures,
        "displace directory: subtree was not preserved at BAK path",
    )
    expect(
        result.get("backup_path") == "album/.kitchensync/BAK/2026-05-15_12-00-00_000003Z/raw",
        failures,
        f"displace directory: wrong backup_path, got {result}",
    )

    output = client.call_tool(
        "displace",
        {
            "entries": [],
            "path": "missing.txt",
            "staging_timestamp": "2026-05-15_12-00-01_000003Z",
        },
    )
    result = expect_result(output, failures, "displace missing path", "success")
    expect(not result.get("backup_path"), failures, f"displace missing path: backup_path should be absent, got {result}")

    output = client.call_tool(
        "cleanup-expired",
        {
            "entries": [
                file_entry(".kitchensync/BAK/2026-04-30_23-59-59_000000Z/root-old.txt", b"old"),
                file_entry(".kitchensync/BAK/2026-05-01_00-00-00_000000Z/root-keep.txt", b"keep"),
            ],
            "directory_path": "",
            "bak_cutoff_exclusive": "2026-05-01_00-00-00_000000Z",
            "tmp_cutoff_exclusive": "2026-05-14_00-00-00_000000Z",
        },
    )
    result = expect_result(output, failures, "cleanup root metadata directory", "success")
    entries = output["entries"]
    removed_paths = result_list(result, "removed_paths")
    expect(
        ".kitchensync/BAK/2026-04-30_23-59-59_000000Z" in removed_paths,
        failures,
        f"cleanup root: expired BAK timestamp directory was not reported removed, got {result}",
    )
    expect(not has_path(entries, ".kitchensync/BAK/2026-04-30_23-59-59_000000Z/root-old.txt"), failures, "cleanup root: expired BAK file was retained")
    expect(has_path(entries, ".kitchensync/BAK/2026-05-01_00-00-00_000000Z/root-keep.txt"), failures, "cleanup root: cutoff-equal BAK timestamp directory was deleted")

    cleanup_entries = [
        file_entry("album/.kitchensync/BAK/2026-04-30_23-59-59_000000Z/old.txt", b"old"),
        file_entry("album/.kitchensync/BAK/2026-05-01_00-00-00_000000Z/keep.txt", b"keep"),
        file_entry("album/.kitchensync/BAK/not-a-timestamp/keep.txt", b"keep"),
        file_entry("album/.kitchensync/TMP/2026-05-13_23-59-59_000000Z/tmp.txt", b"tmp"),
    ]
    output = client.call_tool(
        "cleanup-expired",
        {
            "entries": cleanup_entries,
            "directory_path": "album",
            "bak_cutoff_exclusive": "2026-05-01_00-00-00_000000Z",
            "tmp_cutoff_exclusive": "2026-05-14_00-00-00_000000Z",
        },
    )
    result = expect_result(output, failures, "cleanup expired BAK and TMP directories", "success")
    entries = output["entries"]
    removed_paths = result_list(result, "removed_paths")
    expect(
        "album/.kitchensync/BAK/2026-04-30_23-59-59_000000Z" in removed_paths,
        failures,
        f"cleanup: expired BAK timestamp directory was not reported removed, got {result}",
    )
    expect(
        "album/.kitchensync/TMP/2026-05-13_23-59-59_000000Z" in removed_paths,
        failures,
        f"cleanup: expired TMP timestamp directory was not reported removed, got {result}",
    )
    expect(
        "album/.kitchensync/BAK/2026-05-01_00-00-00_000000Z" not in removed_paths,
        failures,
        f"cleanup: cutoff-equal BAK timestamp directory was reported removed, got {result}",
    )
    expect(not has_path(entries, "album/.kitchensync/BAK/2026-04-30_23-59-59_000000Z/old.txt"), failures, "cleanup: expired BAK file was retained")
    expect(has_path(entries, "album/.kitchensync/BAK/2026-05-01_00-00-00_000000Z/keep.txt"), failures, "cleanup: cutoff-equal BAK timestamp directory was deleted")
    expect(has_path(entries, "album/.kitchensync/BAK/not-a-timestamp/keep.txt"), failures, "cleanup: non-timestamp BAK directory was deleted")
    expect(not has_path(entries, "album/.kitchensync/TMP/2026-05-13_23-59-59_000000Z/tmp.txt"), failures, "cleanup: expired TMP file was retained")

    output = client.call_tool(
        "cleanup-expired",
        {
            "entries": [
                file_entry(
                    "mtime/.kitchensync/BAK/2026-04-30_23-59-59_000000Z/old-by-name.txt",
                    b"old by name",
                    mod_time="2026-05-20T00:00:00Z",
                ),
                file_entry(
                    "mtime/.kitchensync/BAK/2026-05-02_00-00-00_000000Z/new-by-name.txt",
                    b"new by name",
                    mod_time="2026-04-01T00:00:00Z",
                ),
            ],
            "directory_path": "mtime",
            "bak_cutoff_exclusive": "2026-05-01_00-00-00_000000Z",
            "tmp_cutoff_exclusive": "2026-05-01_00-00-00_000000Z",
        },
    )
    expect_result(output, failures, "cleanup uses timestamp names", "success")
    entries = output["entries"]
    expect(
        not has_path(entries, "mtime/.kitchensync/BAK/2026-04-30_23-59-59_000000Z/old-by-name.txt"),
        failures,
        "cleanup timestamp names: expired name with newer mod time was retained",
    )
    expect(
        has_path(entries, "mtime/.kitchensync/BAK/2026-05-02_00-00-00_000000Z/new-by-name.txt"),
        failures,
        "cleanup timestamp names: unexpired name with older mod time was deleted",
    )

    output = client.call_tool(
        "cleanup-expired",
        {
            "entries": [file_entry("plain/file.txt", b"plain")],
            "directory_path": "plain",
            "bak_cutoff_exclusive": "2026-05-01_00-00-00_000000Z",
            "tmp_cutoff_exclusive": "2026-05-14_00-00-00_000000Z",
        },
    )
    result = expect_result(output, failures, "cleanup missing metadata directories", "success")
    expect(has_path(output["entries"], "plain/file.txt"), failures, "cleanup missing metadata: non-metadata file was removed")
    expect(not result_list(result, "removed_paths"), failures, f"cleanup missing metadata: paths were removed, got {result}")


def check_failure_and_invalid_inputs(client: McpClient, failures: list[str]) -> None:
    invalid_cases = [
        ("empty source path", {"source_path": ""}, "invalid_path"),
        ("empty destination path", {"destination_path": ""}, "invalid_path"),
        ("parent-segment source path", {"source_path": "../source.txt"}, "invalid_path"),
        ("parent-segment destination path", {"destination_path": "../dest.txt"}, "invalid_path"),
        ("absolute destination path", {"destination_path": "/dest.txt"}, "invalid_path"),
        ("trailing-slash destination path", {"destination_path": "dest.txt/"}, "invalid_path"),
        ("empty-segment destination path", {"destination_path": "dir//dest.txt"}, "invalid_path"),
        ("dot-segment destination path", {"destination_path": "dir/./dest.txt"}, "invalid_path"),
        ("backslash destination path", {"destination_path": "dir\\dest.txt"}, "invalid_path"),
        ("nul destination path", {"destination_path": "dest\u0000.txt"}, "invalid_path"),
        ("invalid timestamp", {"staging_timestamp": "2026-99-99_10-31-00_000001Z"}, "invalid_timestamp"),
        ("invalid transfer id", {"transfer_id": "not-a-uuid"}, "invalid_transfer_id"),
        ("invalid chunk size", {"chunk_size": 0}, "invalid_settings"),
        ("invalid channel capacity", {"channel_capacity": 0}, "invalid_settings"),
    ]
    for label, overrides, expected_error in invalid_cases:
        args = {
            "source_path": "source.txt",
            "destination_path": "dest.txt",
            **overrides,
        }
        output = client.call_tool(
            "copy-file",
            copy_args(
                [file_entry("source.txt", b"new")],
                [file_entry("dest.txt", b"old")],
                **args,
            ),
        )
        expect_result(output, failures, label, "failed", expected_error)
        expect(file_bytes(output["destination_entries"], "dest.txt") == b"old", failures, f"{label}: destination was mutated")

    output = client.call_tool(
        "copy-file",
        copy_args(
            [file_entry("dest.txt", b"old")],
            [],
            same_filesystem=True,
            source_path="dest.txt",
            destination_path="dest.txt",
            staging_timestamp="2026-05-15_10-31-00_100001Z",
            transfer_id="123e4567-e89b-12d3-a456-426614174099",
        ),
    )
    expect_result(output, failures, "same filesystem same path copy", "failed", "same_source_and_destination")
    expect(file_bytes(output["destination_entries"], "dest.txt") == b"old", failures, "same path copy: destination was mutated")

    output = client.call_tool(
        "copy-file",
        copy_args(
            [],
            [file_entry("dest.txt", b"old")],
            source_path="missing.txt",
            destination_path="dest.txt",
            staging_timestamp="2026-05-15_10-31-00_200001Z",
            transfer_id="123e4567-e89b-12d3-a456-426614174098",
        ),
    )
    expect_result(output, failures, "missing source before final rename", "failed", "not_found")
    expect(file_bytes(output["destination_entries"], "dest.txt") == b"old", failures, "missing source: original destination changed")
    expect(not has_path(output["destination_entries"], ".kitchensync/TMP/2026-05-15_10-31-00_200001Z"), failures, "missing source: TMP metadata remained")

    output = client.call_tool(
        "displace",
        {
            "entries": [file_entry("dest.txt", b"old")],
            "path": "../dest.txt",
            "staging_timestamp": "2026-05-15_10-31-00_210001Z",
        },
    )
    expect_result(output, failures, "displace invalid path", "failed", "invalid_path")
    expect(file_bytes(output["entries"], "dest.txt") == b"old", failures, "displace invalid path: entries were mutated")

    output = client.call_tool(
        "displace",
        {
            "entries": [file_entry("dest.txt", b"old")],
            "path": "",
            "staging_timestamp": "2026-05-15_10-31-00_210002Z",
        },
    )
    expect_result(output, failures, "displace empty path", "failed", "invalid_path")
    expect(file_bytes(output["entries"], "dest.txt") == b"old", failures, "displace empty path: entries were mutated")

    output = client.call_tool(
        "displace",
        {
            "entries": [file_entry("dest.txt", b"old")],
            "path": "dest.txt",
            "staging_timestamp": "2026-05-15_10-31-00_12345Z",
        },
    )
    expect_result(output, failures, "displace invalid timestamp", "failed", "invalid_timestamp")
    expect(file_bytes(output["entries"], "dest.txt") == b"old", failures, "displace invalid timestamp: entries were mutated")

    output = client.call_tool(
        "cleanup-expired",
        {
            "entries": [file_entry("album/.kitchensync/BAK/2026-04-30_23-59-59_000000Z/old.txt", b"old")],
            "directory_path": "../album",
            "bak_cutoff_exclusive": "2026-05-01_00-00-00_000000Z",
            "tmp_cutoff_exclusive": "2026-05-14_00-00-00_000000Z",
        },
    )
    expect_result(output, failures, "cleanup invalid directory path", "failed", "invalid_path")
    expect(has_path(output["entries"], "album/.kitchensync/BAK/2026-04-30_23-59-59_000000Z/old.txt"), failures, "cleanup invalid directory path: entries were mutated")

    output = client.call_tool(
        "cleanup-expired",
        {
            "entries": [file_entry("album/.kitchensync/BAK/2026-04-30_23-59-59_000000Z/old.txt", b"old")],
            "directory_path": "album",
            "bak_cutoff_exclusive": "2026-05-01_00-00-00_000000",
            "tmp_cutoff_exclusive": "2026-05-14_00-00-00_000000Z",
        },
    )
    expect_result(output, failures, "cleanup invalid BAK cutoff timestamp", "failed", "invalid_timestamp")
    expect(has_path(output["entries"], "album/.kitchensync/BAK/2026-04-30_23-59-59_000000Z/old.txt"), failures, "cleanup invalid timestamp: entries were mutated")

    output = client.call_tool(
        "cleanup-expired",
        {
            "entries": [file_entry("album/.kitchensync/TMP/2026-05-13_23-59-59_000000Z/tmp.txt", b"tmp")],
            "directory_path": "album",
            "bak_cutoff_exclusive": "2026-05-01_00-00-00_000000Z",
            "tmp_cutoff_exclusive": "2026-05-14_99-00_000000Z",
        },
    )
    expect_result(output, failures, "cleanup invalid TMP cutoff timestamp", "failed", "invalid_timestamp")
    expect(has_path(output["entries"], "album/.kitchensync/TMP/2026-05-13_23-59-59_000000Z/tmp.txt"), failures, "cleanup invalid TMP timestamp: entries were mutated")

    output = client.call_tool(
        "copy-file",
        copy_args(
            [file_entry("blocked.txt", b"blocked new")],
            [
                file_entry("blocked.txt", b"blocked old"),
                file_entry(".kitchensync/BAK/2026-05-15_10-31-00_300001Z", b"not a directory"),
            ],
            source_path="blocked.txt",
            destination_path="blocked.txt",
            staging_timestamp="2026-05-15_10-31-00_300001Z",
            transfer_id="123e4567-e89b-12d3-a456-426614174097",
        ),
    )
    expect_result(output, failures, "displacement failure prevents final rename", "failed", "displacement_failed")
    expect(file_bytes(output["destination_entries"], "blocked.txt") == b"blocked old", failures, "displacement failure: original changed")
    expect(not has_path(output["destination_entries"], ".kitchensync/TMP/2026-05-15_10-31-00_300001Z"), failures, "displacement failure: TMP directory remained")


def main() -> int:
    failures: list[str] = []
    client = McpClient()
    try:
        client.start()
        check_copy_behaviors(client, failures)
        check_displace_and_cleanup(client, failures)
        check_failure_and_invalid_inputs(client, failures)

        time.sleep(0.2)
        expect(not client.extra_stdout, failures, f"public operations wrote to stdout after MCP_PORT: {client.extra_stdout!r}")
        expect(not client.stderr, failures, f"public operations wrote to stderr: {client.stderr!r}")
    except Exception as exc:
        failures.append(f"test harness failed: {exc!r}")
    finally:
        client.close()

    if failures:
        print("FAIL")
        for index, failure in enumerate(failures, 1):
            print(f"{index}. {failure}")
        return 1
    print("PASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
