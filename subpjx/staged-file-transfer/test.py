#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

from __future__ import annotations

import json
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
from datetime import datetime, timezone
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

PROJECT_DIR = Path("C:/Users/human/Desktop/prjx/kitchensync/subpjx/staged-file-transfer")
JAVA = Path("C:/Users/human/Desktop/prjx/kitchensync/tools/compiler/jdk/bin/java.exe")
MCP_JAR = Path("C:/Users/human/Desktop/prjx/kitchensync/subpjx/staged-file-transfer/released/staged-file-transfer_MCP.jar")

TS1 = "2026-05-15_10-31-00_000001Z"
TS2 = "2026-05-15_11-00-00_000002Z"
TS3 = "2026-05-15_12-00-00_000003Z"
UID1 = "123e4567-e89b-12d3-a456-426614174000"
UID2 = "123e4567-e89b-12d3-a456-426614174111"
UID3 = "ffffffff-eeee-dddd-cccc-bbbbbbbbbbbb"
MOD_TIME = "2026-05-15T10:30:00Z"

failures: list[str] = []


def drain(stream, sink=None):
    for line in stream:
        if sink is not None:
            sink.append(line)


def launch_mcp() -> tuple[subprocess.Popen, int]:
    proc = subprocess.Popen(
        [str(JAVA), "-jar", str(MCP_JAR)],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    if proc.stdout is None or proc.stderr is None:
        proc.terminate()
        raise RuntimeError("MCP server pipes were not created")

    stderr_buf: list[str] = []
    threading.Thread(target=drain, args=(proc.stderr, stderr_buf), daemon=True).start()

    stdout_buf: list[str] = []
    port = None
    deadline = time.time() + 30
    while time.time() < deadline and proc.poll() is None:
        line = proc.stdout.readline()
        stdout_buf.append(line)
        if line.startswith("MCP_PORT="):
            port = int(line.strip().split("=", 1)[1])
            break

    if port is None:
        proc.terminate()
        try:
            proc.wait(timeout=2)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait(timeout=2)
        raise RuntimeError(
            "MCP server did not advertise MCP_PORT\n"
            f"--- stdout ---\n{''.join(stdout_buf)}\n"
            f"--- stderr ---\n{''.join(stderr_buf)}"
        )

    threading.Thread(target=drain, args=(proc.stdout,), daemon=True).start()
    return proc, port


def shutdown_mcp(proc: subprocess.Popen, port: int) -> None:
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=3) as sock:
            rpc(sock, "aitc/shutdown", rpc_id=999)
    except Exception:
        pass
    try:
        proc.wait(timeout=5)
        return
    except subprocess.TimeoutExpired:
        pass
    proc.terminate()
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait()


def rpc(sock: socket.socket, method: str, params=None, rpc_id: int = 1) -> dict:
    msg = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
    if params is not None:
        msg["params"] = params
    sock.sendall((json.dumps(msg, separators=(",", ":")) + "\n").encode("utf-8"))
    data = b""
    deadline = time.time() + 15
    while b"\n" not in data and time.time() < deadline:
        chunk = sock.recv(65536)
        if not chunk:
            break
        data += chunk
    line, _, _ = data.partition(b"\n")
    return json.loads(line.decode("utf-8"))


def call_tool(sock: socket.socket, tool_name: str, arguments: dict, rpc_id: int = 1) -> dict:
    return rpc(sock, "tools/call", {"name": tool_name, "arguments": arguments}, rpc_id)


def parse_result(response: dict) -> dict:
    if "error" in response and response["error"] is not None:
        return {"_rpc_error": response["error"]}
    result = response.get("result", {})
    content = result.get("content", [])
    if content and isinstance(content[0], dict):
        text = content[0].get("text", "{}")
        try:
            return json.loads(text)
        except json.JSONDecodeError:
            return {"_parse_error": text}
    if isinstance(result, dict) and "status" in result:
        return result
    return result


def check(condition: bool, msg: str) -> None:
    if not condition:
        failures.append(msg)
        print(f"FAIL: {msg}")
    else:
        print(f"PASS: {msg}")


def conn(port: int) -> socket.socket:
    return socket.create_connection(("127.0.0.1", port), timeout=10)


# ---- copy_file ----

def test_copy_nested_empty_destination(port: int) -> None:
    tmpdir = Path(tempfile.mkdtemp(prefix="sft_"))
    try:
        src = tmpdir / "src"
        dst = tmpdir / "dst"
        src.mkdir(); dst.mkdir()
        (src / "album").mkdir()
        (src / "album" / "a.jpg").write_bytes(bytes([0x01, 0x02, 0x03, 0xFF, 0xFE]))

        with conn(port) as sock:
            resp = call_tool(sock, "copy-file", {
                "source_root": str(src),
                "source_path": "album/a.jpg",
                "destination_root": str(dst),
                "destination_path": "album/a.jpg",
                "winning_mod_time": MOD_TIME,
                "staging_timestamp": TS1,
                "transfer_id": UID1,
                "chunk_size": 4096,
                "channel_capacity": 4,
            })
        r = parse_result(resp)
        check(r.get("status") == "success", f"copy nested empty: status={r.get('status')}, full={r}")
        check(r.get("final_path") == "album/a.jpg",
              f"copy nested empty: final_path={r.get('final_path')!r}")
        expected_tmp = f"album/.kitchensync/TMP/{TS1}/{UID1}/a.jpg"
        check(r.get("temporary_path") == expected_tmp,
              f"copy nested empty: temporary_path={r.get('temporary_path')!r}, want={expected_tmp!r}")
        check(not r.get("backup_path"),
              f"copy nested empty: backup_path should be absent, got {r.get('backup_path')!r}")
        dest = dst / "album" / "a.jpg"
        check(dest.exists(), "copy nested empty: destination file must exist")
        check(dest.read_bytes() == bytes([0x01, 0x02, 0x03, 0xFF, 0xFE]),
              "copy nested empty: binary content must match exactly")
        tmp_uuid_dir = dst / "album" / ".kitchensync" / "TMP" / TS1 / UID1
        check(not tmp_uuid_dir.exists(),
              "copy nested empty: TMP UUID dir cleaned up after success")
    finally:
        shutil.rmtree(str(tmpdir), ignore_errors=True)


def test_copy_root_level_path(port: int) -> None:
    tmpdir = Path(tempfile.mkdtemp(prefix="sft_"))
    try:
        src = tmpdir / "src"
        dst = tmpdir / "dst"
        src.mkdir(); dst.mkdir()
        (src / "a.txt").write_bytes(b"root file")

        with conn(port) as sock:
            resp = call_tool(sock, "copy-file", {
                "source_root": str(src),
                "source_path": "a.txt",
                "destination_root": str(dst),
                "destination_path": "a.txt",
                "winning_mod_time": MOD_TIME,
                "staging_timestamp": TS2,
                "transfer_id": UID2,
                "chunk_size": 1024,
                "channel_capacity": 2,
            })
        r = parse_result(resp)
        check(r.get("status") == "success",
              f"copy root level: status={r.get('status')}, full={r}")
        expected_tmp = f".kitchensync/TMP/{TS2}/{UID2}/a.txt"
        check(r.get("temporary_path") == expected_tmp,
              f"copy root level: temporary_path={r.get('temporary_path')!r}, want={expected_tmp!r}")
        check((dst / "a.txt").read_bytes() == b"root file",
              "copy root level: content preserved")
    finally:
        shutil.rmtree(str(tmpdir), ignore_errors=True)


def test_copy_over_existing_file(port: int) -> None:
    tmpdir = Path(tempfile.mkdtemp(prefix="sft_"))
    try:
        src = tmpdir / "src"
        dst = tmpdir / "dst"
        src.mkdir(); dst.mkdir()
        (src / "notes").mkdir()
        (src / "notes" / "todo.txt").write_bytes(b"new content")
        (dst / "notes").mkdir()
        (dst / "notes" / "todo.txt").write_bytes(b"old content")

        with conn(port) as sock:
            resp = call_tool(sock, "copy-file", {
                "source_root": str(src),
                "source_path": "notes/todo.txt",
                "destination_root": str(dst),
                "destination_path": "notes/todo.txt",
                "winning_mod_time": MOD_TIME,
                "staging_timestamp": TS2,
                "transfer_id": UID2,
                "chunk_size": 4096,
                "channel_capacity": 4,
            })
        r = parse_result(resp)
        check(r.get("status") == "success",
              f"copy over file: status={r.get('status')}, full={r}")
        expected_bak = f"notes/.kitchensync/BAK/{TS2}/todo.txt"
        check(r.get("backup_path") == expected_bak,
              f"copy over file: backup_path={r.get('backup_path')!r}, want={expected_bak!r}")
        bak = dst / "notes" / ".kitchensync" / "BAK" / TS2 / "todo.txt"
        check(bak.exists(), "copy over file: displaced file must exist at BAK path")
        check(bak.read_bytes() == b"old content",
              "copy over file: original content preserved in BAK")
        check((dst / "notes" / "todo.txt").read_bytes() == b"new content",
              "copy over file: new content at destination")
    finally:
        shutil.rmtree(str(tmpdir), ignore_errors=True)


def test_copy_over_existing_directory(port: int) -> None:
    tmpdir = Path(tempfile.mkdtemp(prefix="sft_"))
    try:
        src = tmpdir / "src"
        dst = tmpdir / "dst"
        src.mkdir(); dst.mkdir()
        (src / "item").write_bytes(b"\xAB\xCD")
        (dst / "item").mkdir()
        (dst / "item" / "child.txt").write_bytes(b"inside dir")

        with conn(port) as sock:
            resp = call_tool(sock, "copy-file", {
                "source_root": str(src),
                "source_path": "item",
                "destination_root": str(dst),
                "destination_path": "item",
                "winning_mod_time": MOD_TIME,
                "staging_timestamp": TS3,
                "transfer_id": UID3,
                "chunk_size": 4096,
                "channel_capacity": 4,
            })
        r = parse_result(resp)
        check(r.get("status") == "success",
              f"copy over dir: status={r.get('status')}, full={r}")
        expected_bak = f".kitchensync/BAK/{TS3}/item"
        check(r.get("backup_path") == expected_bak,
              f"copy over dir: backup_path={r.get('backup_path')!r}, want={expected_bak!r}")
        bak_dir = dst / ".kitchensync" / "BAK" / TS3 / "item"
        check(bak_dir.is_dir(), "copy over dir: displaced dir must exist at BAK path")
        check((bak_dir / "child.txt").exists(),
              "copy over dir: subtree preserved in BAK")
    finally:
        shutil.rmtree(str(tmpdir), ignore_errors=True)


def test_copy_same_filesystem_different_paths(port: int) -> None:
    tmpdir = Path(tempfile.mkdtemp(prefix="sft_"))
    try:
        fs = tmpdir / "fs"
        fs.mkdir()
        (fs / "original.txt").write_bytes(b"shared fs data")

        with conn(port) as sock:
            resp = call_tool(sock, "copy-file", {
                "source_root": str(fs),
                "source_path": "original.txt",
                "destination_root": str(fs),
                "destination_path": "copy.txt",
                "winning_mod_time": MOD_TIME,
                "staging_timestamp": TS1,
                "transfer_id": UID1,
                "chunk_size": 4096,
                "channel_capacity": 4,
            })
        r = parse_result(resp)
        check(r.get("status") == "success",
              f"copy same fs: status={r.get('status')}, full={r}")
        check((fs / "copy.txt").read_bytes() == b"shared fs data",
              "copy same fs: content copied correctly")
        check((fs / "original.txt").exists(),
              "copy same fs: source file unchanged")
    finally:
        shutil.rmtree(str(tmpdir), ignore_errors=True)


def test_copy_small_chunk_preserves_binary(port: int) -> None:
    tmpdir = Path(tempfile.mkdtemp(prefix="sft_"))
    try:
        src = tmpdir / "src"
        dst = tmpdir / "dst"
        src.mkdir(); dst.mkdir()
        content = bytes(range(256))
        (src / "bin.dat").write_bytes(content)

        with conn(port) as sock:
            resp = call_tool(sock, "copy-file", {
                "source_root": str(src),
                "source_path": "bin.dat",
                "destination_root": str(dst),
                "destination_path": "bin.dat",
                "winning_mod_time": MOD_TIME,
                "staging_timestamp": TS1,
                "transfer_id": UID1,
                "chunk_size": 1,
                "channel_capacity": 1,
            })
        r = parse_result(resp)
        check(r.get("status") == "success",
              f"copy small chunk: status={r.get('status')}, full={r}")
        dest = dst / "bin.dat"
        check(dest.exists() and dest.read_bytes() == content,
              "copy small chunk: all 256 byte values preserved with chunk_size=1")
    finally:
        shutil.rmtree(str(tmpdir), ignore_errors=True)


def test_copy_winning_mod_time(port: int) -> None:
    tmpdir = Path(tempfile.mkdtemp(prefix="sft_"))
    try:
        src = tmpdir / "src"
        dst = tmpdir / "dst"
        src.mkdir(); dst.mkdir()
        (src / "f.txt").write_bytes(b"hi")

        target_mtime = datetime(2026, 1, 1, 0, 0, 0, tzinfo=timezone.utc).timestamp()

        with conn(port) as sock:
            resp = call_tool(sock, "copy-file", {
                "source_root": str(src),
                "source_path": "f.txt",
                "destination_root": str(dst),
                "destination_path": "f.txt",
                "winning_mod_time": "2026-01-01T00:00:00Z",
                "staging_timestamp": TS1,
                "transfer_id": UID1,
                "chunk_size": 4096,
                "channel_capacity": 4,
            })
        r = parse_result(resp)
        check(r.get("status") == "success",
              f"mod time: copy should succeed, got {r}")
        dest = dst / "f.txt"
        if dest.exists():
            actual = dest.stat().st_mtime
            check(abs(actual - target_mtime) < 2.0,
                  f"mod time: expected ~{target_mtime}, got {actual} (diff={abs(actual - target_mtime):.3f}s)")
    finally:
        shutil.rmtree(str(tmpdir), ignore_errors=True)


# ---- displace ----

def test_displace_file(port: int) -> None:
    tmpdir = Path(tempfile.mkdtemp(prefix="sft_"))
    try:
        fs = tmpdir / "fs"
        (fs / "album").mkdir(parents=True)
        (fs / "album" / "photo.jpg").write_bytes(b"\xFF\xD8\xFF")

        with conn(port) as sock:
            resp = call_tool(sock, "displace", {
                "filesystem_root": str(fs),
                "path": "album/photo.jpg",
                "staging_timestamp": TS3,
            })
        r = parse_result(resp)
        check(r.get("status") == "success",
              f"displace file: status={r.get('status')}, full={r}")
        expected_bak = f"album/.kitchensync/BAK/{TS3}/photo.jpg"
        check(r.get("backup_path") == expected_bak,
              f"displace file: backup_path={r.get('backup_path')!r}, want={expected_bak!r}")
        check(not (fs / "album" / "photo.jpg").exists(),
              "displace file: original must be gone")
        bak = fs / "album" / ".kitchensync" / "BAK" / TS3 / "photo.jpg"
        check(bak.exists() and bak.read_bytes() == b"\xFF\xD8\xFF",
              "displace file: content preserved in BAK")
    finally:
        shutil.rmtree(str(tmpdir), ignore_errors=True)


def test_displace_directory(port: int) -> None:
    tmpdir = Path(tempfile.mkdtemp(prefix="sft_"))
    try:
        fs = tmpdir / "fs"
        (fs / "album" / "raw" / "sub").mkdir(parents=True)
        (fs / "album" / "raw" / "img001.dng").write_bytes(b"\xAB\xCD")
        (fs / "album" / "raw" / "sub" / "img002.dng").write_bytes(b"\xEF\x01")

        with conn(port) as sock:
            resp = call_tool(sock, "displace", {
                "filesystem_root": str(fs),
                "path": "album/raw",
                "staging_timestamp": TS3,
            })
        r = parse_result(resp)
        check(r.get("status") == "success",
              f"displace dir: status={r.get('status')}, full={r}")
        expected_bak = f"album/.kitchensync/BAK/{TS3}/raw"
        check(r.get("backup_path") == expected_bak,
              f"displace dir: backup_path={r.get('backup_path')!r}, want={expected_bak!r}")
        check(not (fs / "album" / "raw").exists(),
              "displace dir: original directory must be gone")
        bak = fs / "album" / ".kitchensync" / "BAK" / TS3 / "raw"
        check(bak.is_dir(), "displace dir: BAK directory must exist")
        check((bak / "img001.dng").read_bytes() == b"\xAB\xCD",
              "displace dir: top-level file in subtree preserved")
        check((bak / "sub" / "img002.dng").read_bytes() == b"\xEF\x01",
              "displace dir: nested file in subtree preserved")
    finally:
        shutil.rmtree(str(tmpdir), ignore_errors=True)


def test_displace_nonexistent(port: int) -> None:
    tmpdir = Path(tempfile.mkdtemp(prefix="sft_"))
    try:
        fs = tmpdir / "fs"
        fs.mkdir()

        with conn(port) as sock:
            resp = call_tool(sock, "displace", {
                "filesystem_root": str(fs),
                "path": "does/not/exist.txt",
                "staging_timestamp": TS1,
            })
        r = parse_result(resp)
        check(r.get("status") == "success",
              f"displace nonexistent: status={r.get('status')}, full={r}")
        check(not r.get("backup_path"),
              f"displace nonexistent: backup_path should be absent, got {r.get('backup_path')!r}")
    finally:
        shutil.rmtree(str(tmpdir), ignore_errors=True)


# ---- cleanup_expired ----

def test_cleanup_expired(port: int) -> None:
    tmpdir = Path(tempfile.mkdtemp(prefix="sft_"))
    try:
        fs = tmpdir / "fs"
        bak = fs / "album" / ".kitchensync" / "BAK"
        tmp = fs / "album" / ".kitchensync" / "TMP"

        expired_bak = "2026-04-30_23-59-59_000000Z"
        retained_bak = "2026-05-01_00-00-00_000000Z"
        expired_tmp = "2026-05-13_23-59-59_000000Z"

        (bak / expired_bak).mkdir(parents=True)
        (bak / expired_bak / "old.txt").write_bytes(b"old")
        (bak / retained_bak).mkdir(parents=True)
        (bak / "not-a-timestamp").mkdir(parents=True)
        (tmp / expired_tmp / "uuid1").mkdir(parents=True)
        (tmp / expired_tmp / "uuid1" / "staged.dat").write_bytes(b"tmp")

        with conn(port) as sock:
            resp = call_tool(sock, "cleanup-expired", {
                "filesystem_root": str(fs),
                "directory_path": "album",
                "bak_cutoff_exclusive": "2026-05-01_00-00-00_000000Z",
                "tmp_cutoff_exclusive": "2026-05-14_00-00-00_000000Z",
            })
        r = parse_result(resp)
        check(r.get("status") in ("success", "partial_success"),
              f"cleanup: status={r.get('status')}, full={r}")
        removed = r.get("removed_paths", [])
        check(any(expired_bak in p for p in removed),
              f"cleanup: expired BAK '{expired_bak}' must appear in removed_paths={removed}")
        check(any(expired_tmp in p for p in removed),
              f"cleanup: expired TMP '{expired_tmp}' must appear in removed_paths={removed}")
        check(not (bak / expired_bak).exists(),
              "cleanup: expired BAK dir deleted from filesystem")
        check(not (tmp / expired_tmp).exists(),
              "cleanup: expired TMP dir deleted from filesystem")
        check((bak / retained_bak).exists(),
              "cleanup: retained BAK dir must still exist")
        check((bak / "not-a-timestamp").exists(),
              "cleanup: non-timestamp directory must be ignored and preserved")
    finally:
        shutil.rmtree(str(tmpdir), ignore_errors=True)


def test_cleanup_root_directory(port: int) -> None:
    tmpdir = Path(tempfile.mkdtemp(prefix="sft_"))
    try:
        fs = tmpdir / "fs"
        expired_bak = "2026-04-15_00-00-00_000000Z"
        (fs / ".kitchensync" / "BAK" / expired_bak).mkdir(parents=True)

        with conn(port) as sock:
            resp = call_tool(sock, "cleanup-expired", {
                "filesystem_root": str(fs),
                "directory_path": "",
                "bak_cutoff_exclusive": "2026-05-01_00-00-00_000000Z",
                "tmp_cutoff_exclusive": "2026-05-01_00-00-00_000000Z",
            })
        r = parse_result(resp)
        check(r.get("status") in ("success", "partial_success"),
              f"cleanup root: status={r.get('status')}, full={r}")
        check(not (fs / ".kitchensync" / "BAK" / expired_bak).exists(),
              "cleanup root: expired BAK at root level must be deleted")
    finally:
        shutil.rmtree(str(tmpdir), ignore_errors=True)


def test_cleanup_missing_kitchensync(port: int) -> None:
    tmpdir = Path(tempfile.mkdtemp(prefix="sft_"))
    try:
        fs = tmpdir / "fs"
        (fs / "mydir").mkdir(parents=True)

        with conn(port) as sock:
            resp = call_tool(sock, "cleanup-expired", {
                "filesystem_root": str(fs),
                "directory_path": "mydir",
                "bak_cutoff_exclusive": "2026-05-01_00-00-00_000000Z",
                "tmp_cutoff_exclusive": "2026-05-01_00-00-00_000000Z",
            })
        r = parse_result(resp)
        check(r.get("status") == "success",
              f"cleanup no .kitchensync: status={r.get('status')}, full={r}")
    finally:
        shutil.rmtree(str(tmpdir), ignore_errors=True)


# ---- error behavior ----

def test_invalid_path_errors(port: int) -> None:
    tmpdir = Path(tempfile.mkdtemp(prefix="sft_"))
    try:
        fs = tmpdir / "fs"
        fs.mkdir()
        (fs / "a.txt").write_bytes(b"x")

        cases = [
            ("/leading/slash", "leading slash"),
            ("trailing/slash/", "trailing slash"),
            ("has\\backslash", "backslash"),
            ("empty//segment", "empty segment"),
            ("dot/./segment", "dot segment"),
            ("dotdot/../up", "dotdot segment"),
        ]

        with conn(port) as sock:
            for i, (bad_path, label) in enumerate(cases, start=1):
                resp = call_tool(sock, "copy-file", {
                    "source_root": str(fs),
                    "source_path": "a.txt",
                    "destination_root": str(fs),
                    "destination_path": bad_path,
                    "winning_mod_time": MOD_TIME,
                    "staging_timestamp": TS1,
                    "transfer_id": UID1,
                    "chunk_size": 4096,
                    "channel_capacity": 4,
                }, rpc_id=i)
                r = parse_result(resp)
                check(r.get("error") == "invalid_path",
                      f"invalid path ({label}): expected error=invalid_path, got {r}")

            resp = call_tool(sock, "displace", {
                "filesystem_root": str(fs),
                "path": "/absolute",
                "staging_timestamp": TS1,
            }, rpc_id=len(cases) + 1)
            r = parse_result(resp)
            check(r.get("error") == "invalid_path",
                  f"displace invalid path: expected error=invalid_path, got {r}")
    finally:
        shutil.rmtree(str(tmpdir), ignore_errors=True)


def test_invalid_timestamp_errors(port: int) -> None:
    tmpdir = Path(tempfile.mkdtemp(prefix="sft_"))
    try:
        fs = tmpdir / "fs"
        fs.mkdir()
        (fs / "a.txt").write_bytes(b"x")

        cases = [
            ("2026-05-15T10:31:00Z", "ISO 8601 format"),
            ("2026-05-15_10-31-00", "missing microseconds and Z"),
            ("not-a-timestamp", "gibberish"),
            ("2026-13-15_10-31-00_000001Z", "month=13"),
        ]

        with conn(port) as sock:
            for i, (bad_ts, label) in enumerate(cases, start=1):
                resp = call_tool(sock, "copy-file", {
                    "source_root": str(fs),
                    "source_path": "a.txt",
                    "destination_root": str(fs),
                    "destination_path": "b.txt",
                    "winning_mod_time": MOD_TIME,
                    "staging_timestamp": bad_ts,
                    "transfer_id": UID1,
                    "chunk_size": 4096,
                    "channel_capacity": 4,
                }, rpc_id=i)
                r = parse_result(resp)
                check(r.get("error") == "invalid_timestamp",
                      f"invalid timestamp ({label}): expected error=invalid_timestamp, got {r}")
    finally:
        shutil.rmtree(str(tmpdir), ignore_errors=True)


def test_invalid_transfer_id_errors(port: int) -> None:
    tmpdir = Path(tempfile.mkdtemp(prefix="sft_"))
    try:
        fs = tmpdir / "fs"
        fs.mkdir()
        (fs / "a.txt").write_bytes(b"x")

        cases = [
            ("not-a-uuid", "random string"),
            ("123456789", "numeric only"),
            ("gggggggg-hhhh-iiii-jjjj-kkkkkkkkkkkk", "invalid hex chars"),
        ]

        with conn(port) as sock:
            for i, (bad_id, label) in enumerate(cases, start=1):
                resp = call_tool(sock, "copy-file", {
                    "source_root": str(fs),
                    "source_path": "a.txt",
                    "destination_root": str(fs),
                    "destination_path": "b.txt",
                    "winning_mod_time": MOD_TIME,
                    "staging_timestamp": TS1,
                    "transfer_id": bad_id,
                    "chunk_size": 4096,
                    "channel_capacity": 4,
                }, rpc_id=i)
                r = parse_result(resp)
                check(r.get("error") == "invalid_transfer_id",
                      f"invalid transfer_id ({label}): expected error=invalid_transfer_id, got {r}")
    finally:
        shutil.rmtree(str(tmpdir), ignore_errors=True)


def test_invalid_settings_errors(port: int) -> None:
    tmpdir = Path(tempfile.mkdtemp(prefix="sft_"))
    try:
        fs = tmpdir / "fs"
        fs.mkdir()
        (fs / "a.txt").write_bytes(b"x")

        cases = [
            (0, 4, "chunk_size=0"),
            (-1, 4, "chunk_size=-1"),
            (4096, 0, "channel_capacity=0"),
            (4096, -1, "channel_capacity=-1"),
        ]

        with conn(port) as sock:
            for i, (chunk, cap, label) in enumerate(cases, start=1):
                resp = call_tool(sock, "copy-file", {
                    "source_root": str(fs),
                    "source_path": "a.txt",
                    "destination_root": str(fs),
                    "destination_path": "b.txt",
                    "winning_mod_time": MOD_TIME,
                    "staging_timestamp": TS1,
                    "transfer_id": UID1,
                    "chunk_size": chunk,
                    "channel_capacity": cap,
                }, rpc_id=i)
                r = parse_result(resp)
                check(r.get("error") == "invalid_settings",
                      f"invalid settings ({label}): expected error=invalid_settings, got {r}")
    finally:
        shutil.rmtree(str(tmpdir), ignore_errors=True)


def test_same_source_and_destination(port: int) -> None:
    tmpdir = Path(tempfile.mkdtemp(prefix="sft_"))
    try:
        fs = tmpdir / "fs"
        fs.mkdir()
        (fs / "a.txt").write_bytes(b"x")

        with conn(port) as sock:
            resp = call_tool(sock, "copy-file", {
                "source_root": str(fs),
                "source_path": "a.txt",
                "destination_root": str(fs),
                "destination_path": "a.txt",
                "winning_mod_time": MOD_TIME,
                "staging_timestamp": TS1,
                "transfer_id": UID1,
                "chunk_size": 4096,
                "channel_capacity": 4,
            })
        r = parse_result(resp)
        check(r.get("error") == "same_source_and_destination",
              f"same src+dst: expected error=same_source_and_destination, got {r}")
    finally:
        shutil.rmtree(str(tmpdir), ignore_errors=True)


def test_source_not_found(port: int) -> None:
    tmpdir = Path(tempfile.mkdtemp(prefix="sft_"))
    try:
        src = tmpdir / "src"
        dst = tmpdir / "dst"
        src.mkdir(); dst.mkdir()

        with conn(port) as sock:
            resp = call_tool(sock, "copy-file", {
                "source_root": str(src),
                "source_path": "missing.txt",
                "destination_root": str(dst),
                "destination_path": "out.txt",
                "winning_mod_time": MOD_TIME,
                "staging_timestamp": TS1,
                "transfer_id": UID1,
                "chunk_size": 4096,
                "channel_capacity": 4,
            })
        r = parse_result(resp)
        check(r.get("error") == "not_found",
              f"source not found: expected error=not_found, got {r}")
    finally:
        shutil.rmtree(str(tmpdir), ignore_errors=True)


# not reasonably testable: concurrent reader/writer overlap -- internal pipeline concurrency
#   not observable through tools/call
# not reasonably testable: failed read/write before rename leaves original in place --
#   requires fault injection into filesystem backend
# not reasonably testable: displacement failure during copy prevents final rename --
#   requires fault injection
# not reasonably testable: set_mod_time failure -> partial_success with file in place --
#   requires fault injection
# not reasonably testable: cleanup partial failure continues and returns partial_success --
#   requires fault injection
# not reasonably testable: library stdout/stderr suppression -- indistinguishable from
#   MCP wrapper's own stdout (MCP_PORT line) through tools/call surface


def main() -> None:
    proc, port = launch_mcp()
    try:
        test_copy_nested_empty_destination(port)
        test_copy_root_level_path(port)
        test_copy_over_existing_file(port)
        test_copy_over_existing_directory(port)
        test_copy_same_filesystem_different_paths(port)
        test_copy_small_chunk_preserves_binary(port)
        test_copy_winning_mod_time(port)
        test_displace_file(port)
        test_displace_directory(port)
        test_displace_nonexistent(port)
        test_cleanup_expired(port)
        test_cleanup_root_directory(port)
        test_cleanup_missing_kitchensync(port)
        test_invalid_path_errors(port)
        test_invalid_timestamp_errors(port)
        test_invalid_transfer_id_errors(port)
        test_invalid_settings_errors(port)
        test_same_source_and_destination(port)
        test_source_not_found(port)
    finally:
        shutdown_mcp(proc, port)

    if failures:
        print(f"\n{len(failures)} check(s) failed:")
        for f in failures:
            print(f"  - {f}")
        sys.exit(1)
    else:
        print("\nAll checks passed.")
        sys.exit(0)


if __name__ == "__main__":
    main()
