#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = ["paramiko"]
# ///
"""Streaming file read/write: open-read, read, close-read, open-write, write, close-write."""

from __future__ import annotations

import base64, json, os, shutil, socket, subprocess, sys, threading, time
from pathlib import Path

import paramiko

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

PROJECT_PATH = Path(PROJECT).resolve()
TESTKS = PROJECT_PATH / "tmp" / "testks" / "02_file-ops-streaming-io"
TEST_USER = "streamtest"
TEST_PASSWORD = "streaming-io-password"


def _drain(stream):
    for _ in stream:
        pass


class _LocalSFTP(paramiko.SFTPServerInterface):
    def _path(self, path: str) -> str:
        return str(Path(path))

    def stat(self, path):
        try:
            return paramiko.SFTPAttributes.from_stat(os.stat(self._path(path)))
        except OSError as e:
            return paramiko.SFTPServer.convert_errno(e.errno)

    def lstat(self, path):
        try:
            return paramiko.SFTPAttributes.from_stat(os.lstat(self._path(path)))
        except OSError as e:
            return paramiko.SFTPServer.convert_errno(e.errno)

    def open(self, path, flags, attr):
        try:
            fd = os.open(self._path(path), flags, getattr(attr, "st_mode", None) or 0o666)
            if flags & os.O_APPEND:
                mode = "ab"
            elif flags & (os.O_WRONLY | os.O_RDWR):
                mode = "r+b" if flags & os.O_RDWR and not flags & os.O_TRUNC else "wb"
            else:
                mode = "rb"
            handle = paramiko.SFTPHandle(flags)
            file_obj = os.fdopen(fd, mode)
            handle.readfile = file_obj
            handle.writefile = file_obj
            return handle
        except OSError as e:
            return paramiko.SFTPServer.convert_errno(e.errno)

    def mkdir(self, path, attr):
        try:
            os.mkdir(self._path(path), getattr(attr, "st_mode", None) or 0o777)
            return paramiko.SFTP_OK
        except OSError as e:
            return paramiko.SFTPServer.convert_errno(e.errno)


class _SSHServer(paramiko.ServerInterface):
    def check_channel_request(self, kind, chanid):
        return paramiko.OPEN_SUCCEEDED if kind == "session" else paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED

    def check_auth_password(self, username, password):
        if username == TEST_USER and password == TEST_PASSWORD:
            return paramiko.AUTH_SUCCESSFUL
        return paramiko.AUTH_FAILED

    def get_allowed_auths(self, username):
        return "password"


def _start_sftp_server() -> tuple[int, socket.socket, paramiko.PKey]:
    host_key = paramiko.RSAKey.generate(bits=2048)
    srv_sock = socket.socket()
    srv_sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv_sock.bind(("127.0.0.1", 0))
    srv_sock.listen(20)
    port = srv_sock.getsockname()[1]

    def serve_connection(conn):
        transport = paramiko.Transport(conn)
        transport.add_server_key(host_key)
        transport.set_subsystem_handler("sftp", paramiko.SFTPServer, _LocalSFTP)
        try:
            transport.start_server(server=_SSHServer())
            while transport.is_active():
                time.sleep(0.05)
        except Exception:
            pass
        finally:
            transport.close()

    def accept_loop():
        while True:
            try:
                conn, _ = srv_sock.accept()
            except OSError:
                return
            threading.Thread(target=serve_connection, args=(conn,), daemon=True).start()

    threading.Thread(target=accept_loop, daemon=True).start()
    return port, srv_sock, host_key


def _known_hosts_line(host_key: paramiko.PKey, port: int) -> str:
    return f"[127.0.0.1]:{port} {host_key.get_name()} {base64.b64encode(host_key.asbytes()).decode('ascii')}\n"


def _launch(extra_env: dict[str, str] | None = None):
    env = {**os.environ, **(extra_env or {})}
    proc = subprocess.Popen(
        [str(UV), "run", "--script", str(BUILD_PY), "launch-mcp", PROJECT],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        text=True, encoding="utf-8",
        env=env,
    )
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
        raise RuntimeError("MCP server did not advertise MCP_PORT")
    threading.Thread(target=_drain, args=(proc.stdout,), daemon=True).start()
    threading.Thread(target=_drain, args=(proc.stderr,), daemon=True).start()
    return proc, port


def _rpc(sock, method, params=None, rpc_id=1):
    msg = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
    if params is not None:
        msg["params"] = params
    sock.sendall((json.dumps(msg) + "\n").encode("utf-8"))
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
    return json.loads(line.decode("utf-8"))


def main() -> int:
    # Idempotency: reset test state before each run
    if TESTKS.exists():
        shutil.rmtree(TESTKS)
    TESTKS.mkdir(parents=True)

    # Pre-create a file with known content for the read tests
    read_file = TESTKS / "read_source.txt"
    read_content = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"
    read_file.write_bytes(read_content)

    sftp_port, sftp_sock, host_key = _start_sftp_server()
    home = TESTKS / "home"
    ssh_dir = home / ".ssh"
    ssh_dir.mkdir(parents=True)
    (ssh_dir / "known_hosts").write_text(_known_hosts_line(host_key, sftp_port), encoding="utf-8")
    (ssh_dir / "known_hosts").chmod(0o600)

    java_opts = os.environ.get("JAVA_TOOL_OPTIONS", "")
    java_opts = f"{java_opts} -Duser.home={home}" if java_opts else f"-Duser.home={home}"
    proc, port = _launch({"JAVA_TOOL_OPTIONS": java_opts})
    failures = []
    conn_handle = ""

    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as sock:
            _id = [1]

            def call(tool, args):
                r = _rpc(sock, "tools/call", {"name": tool, "arguments": args}, rpc_id=_id[0])
                _id[0] += 1
                return r

            resp = call("acquire", {"url": f"sftp://{TEST_USER}:{TEST_PASSWORD}@127.0.0.1:{sftp_port}/"})
            err = resp.get("error")
            conn_handle = (resp.get("result") or {}).get("handleId", "")
            print(f"[setup] acquire handle: {conn_handle!r}, error: {err}")
            if err or not conn_handle:
                failures.append(f"setup: could not acquire SFTP connection: {err}")
                print("\nFAILURES:")
                for f in failures:
                    print(f"  - {f}")
                return 1

            # 02.26 — open-read returns a read handle
            resp = call("open-read", {"handleId": conn_handle, "path": read_file.as_posix()})
            err = resp.get("error")
            read_handle = (resp.get("result") or {}).get("readHandleId", "")
            print(f"[02.26] open-read handle: {read_handle!r}, error: {err}")
            if err or not read_handle:
                failures.append(f"02.26: open-read did not return a read handle (error={err})")

            # 02.27 — read returns the next chunk of bytes
            resp = call("read", {"readHandleId": read_handle, "maxBytes": 8})
            err = resp.get("error")
            result = resp.get("result") or {}
            chunk_b64 = result.get("data", "")
            eof_27 = result.get("eof", False)
            print(f"[02.27] read bytes (b64): {chunk_b64!r}, eof: {eof_27}, error: {err}")
            if err:
                failures.append(f"02.27: read returned error: {err}")
            else:
                first_chunk = base64.b64decode(chunk_b64) if chunk_b64 else b""
                if first_chunk != read_content[:8]:
                    failures.append(f"02.27: expected first 8 bytes {read_content[:8]!r}, got {first_chunk!r}")

            # 02.28 — read reports EOF after the last chunk
            eof_seen = eof_27
            all_read = base64.b64decode(chunk_b64) if chunk_b64 else b""
            for _ in range(10):
                if eof_seen:
                    break
                resp = call("read", {"readHandleId": read_handle, "maxBytes": 1024})
                if resp.get("error"):
                    failures.append(f"02.28: read returned error before EOF: {resp['error']}")
                    break
                result = resp.get("result") or {}
                eof_seen = result.get("eof", False)
                if result.get("data"):
                    all_read += base64.b64decode(result["data"])
            print(f"[02.28] EOF reached: {eof_seen}")
            if not eof_seen:
                failures.append("02.28: read did not report EOF after last chunk")
            if all_read != read_content:
                failures.append(f"02.28: streaming read content mismatch: expected {read_content!r}, got {all_read!r}")

            # 02.29 — close-read closes the read handle
            resp = call("close-read", {"readHandleId": read_handle})
            err = resp.get("error")
            print(f"[02.29] close-read error: {err}")
            if err:
                failures.append(f"02.29: close-read returned error: {err}")
            else:
                resp = call("read", {"readHandleId": read_handle, "maxBytes": 1})
                reuse_err = resp.get("error")
                print(f"[02.29] read after close error: {reuse_err}")
                if not reuse_err:
                    failures.append("02.29: read handle remained usable after close-read")

            # 02.30 + 02.31 — open-write returns a write handle; creates file when absent
            write_file = TESTKS / "write_target.txt"
            # write_file does not exist (TESTKS was freshly created above, no write_target.txt)
            resp = call("open-write", {"handleId": conn_handle, "path": write_file.as_posix()})
            err = resp.get("error")
            write_handle = (resp.get("result") or {}).get("writeHandleId", "")
            print(f"[02.30] open-write handle: {write_handle!r}, error: {err}")
            if err or not write_handle:
                failures.append(f"02.30: open-write did not return a write handle (error={err})")
            print(f"[02.31] file exists after open-write on absent path: {write_file.exists()}")
            if err:
                failures.append(f"02.31: open-write errored on absent-file path: {err}")
            elif not write_file.exists():
                failures.append("02.31: open-write did not create the absent target file")

            # 02.32 — open-write creates missing parent directories
            deep_file = TESTKS / "new_dir" / "sub_dir" / "deep.txt"
            # new_dir/sub_dir/ don't exist (TESTKS was freshly created)
            resp32 = call("open-write", {"handleId": conn_handle, "path": deep_file.as_posix()})
            err32 = resp32.get("error")
            deep_handle = (resp32.get("result") or {}).get("writeHandleId", "")
            print(f"[02.32] open-write with missing parents, error: {err32}")
            if err32 or not deep_handle:
                failures.append(f"02.32: open-write failed to create missing parent dirs: {err32}")
            elif not deep_file.parent.is_dir():
                failures.append("02.32: open-write did not create missing parent directories")
            if deep_handle:
                close32 = call("close-write", {"writeHandleId": deep_handle})
                if close32.get("error"):
                    failures.append(f"02.32: close-write for deep file returned error: {close32['error']}")
                elif not deep_file.exists():
                    failures.append("02.32: deep file was not created after open-write/close-write")

            # 02.33 — write appends chunks to the write handle
            chunk1, chunk2 = b"First chunk. ", b"Second chunk."
            resp = call("write", {"writeHandleId": write_handle, "data": base64.b64encode(chunk1).decode("ascii")})
            err = resp.get("error")
            print(f"[02.33] write chunk1 error: {err}")
            if err:
                failures.append(f"02.33: write returned error on chunk1: {err}")
            resp = call("write", {"writeHandleId": write_handle, "data": base64.b64encode(chunk2).decode("ascii")})
            err = resp.get("error")
            if err:
                failures.append(f"02.33: write returned error on chunk2: {err}")

            # 02.34 — close-write flushes; bytes are observable in the file afterwards
            resp = call("close-write", {"writeHandleId": write_handle})
            err = resp.get("error")
            print(f"[02.34] close-write error: {err}")
            if err:
                failures.append(f"02.34: close-write returned error: {err}")
            elif not write_file.exists():
                failures.append("02.34: file does not exist after close-write")
            else:
                actual = write_file.read_bytes()
                expected = chunk1 + chunk2
                print(f"[02.34] file bytes: {actual!r}")
                if actual != expected:
                    failures.append(f"02.34: file content mismatch: expected {expected!r}, got {actual!r}")
                resp = call("write", {"writeHandleId": write_handle, "data": base64.b64encode(b"x").decode("ascii")})
                reuse_err = resp.get("error")
                print(f"[02.34] write after close error: {reuse_err}")
                if not reuse_err:
                    failures.append("02.34: write handle remained usable after close-write")

            call("release", {"handleId": conn_handle})
            conn_handle = ""

            if failures:
                print("\nFAILURES:")
                for f in failures:
                    print(f"  - {f}")
                return 1
            print("\nAll assertions passed.")
            return 0
    finally:
        if conn_handle:
            try:
                with socket.create_connection(("127.0.0.1", port), timeout=10) as cleanup_sock:
                    _rpc(cleanup_sock, "tools/call", {"name": "release", "arguments": {"handleId": conn_handle}}, rpc_id=9999)
            except Exception:
                pass
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
        sftp_sock.close()


if __name__ == "__main__":
    sys.exit(main())
