#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = ["paramiko"]
# ///
"""Exercise rename, delete_file, create_dir, delete_dir, and set_mod_time via the sftp-protocol MCP wrapper."""

from __future__ import annotations

import base64, json, os, shutil, socket, subprocess, sys, threading, time
from pathlib import Path

import paramiko

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")
TEST_USER = "mutationtest"
TEST_PASSWORD = "file-ops-mutations-password"


def _drain(stream):
    for _ in stream:
        pass


def _launch(extra_env: dict[str, str] | None = None):
    env = dict(os.environ) if extra_env is None else dict(extra_env)
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

    def remove(self, path):
        try:
            os.remove(self._path(path))
            return paramiko.SFTP_OK
        except OSError as e:
            return paramiko.SFTPServer.convert_errno(e.errno)

    def rename(self, oldpath, newpath):
        try:
            os.rename(self._path(oldpath), self._path(newpath))
            return paramiko.SFTP_OK
        except OSError as e:
            return paramiko.SFTPServer.convert_errno(e.errno)

    def mkdir(self, path, attr):
        try:
            os.mkdir(self._path(path), getattr(attr, "st_mode", None) or 0o777)
            return paramiko.SFTP_OK
        except OSError as e:
            return paramiko.SFTPServer.convert_errno(e.errno)

    def rmdir(self, path):
        try:
            os.rmdir(self._path(path))
            return paramiko.SFTP_OK
        except OSError as e:
            return paramiko.SFTPServer.convert_errno(e.errno)

    def chattr(self, path, attr):
        try:
            target = self._path(path)
            stat_result = os.stat(target)
            atime = getattr(attr, "st_atime", None)
            mtime = getattr(attr, "st_mtime", None)
            os.utime(
                target,
                (
                    stat_result.st_atime if atime is None else atime,
                    stat_result.st_mtime if mtime is None else mtime,
                ),
            )
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
    key = base64.b64encode(host_key.asbytes()).decode("ascii")
    return f"[127.0.0.1]:{port} {host_key.get_name()} {key}\n"


def _remote(path: Path) -> str:
    return path.resolve().as_posix()


_rpc_id = 0
_recv_buf = b""


def _rpc(sock, method, params=None):
    global _rpc_id, _recv_buf
    _rpc_id += 1
    msg = {"jsonrpc": "2.0", "id": _rpc_id, "method": method}
    if params is not None:
        msg["params"] = params
    sock.sendall((json.dumps(msg) + "\n").encode("utf-8"))
    deadline = time.time() + 10
    while b"\n" not in _recv_buf and time.time() < deadline:
        chunk = sock.recv(8192)
        if not chunk:
            break
        _recv_buf += chunk
    line, _, _recv_buf = _recv_buf.partition(b"\n")
    return json.loads(line.decode("utf-8"))


def _call(sock, tool, args):
    return _rpc(sock, "tools/call", {"name": tool, "arguments": args})


def main() -> int:
    project_root = Path(PROJECT).resolve()
    testdir = project_root / "tmp" / "testks" / "02-file-ops-mutations"

    # Idempotency: reset test directory at start
    if testdir.exists():
        shutil.rmtree(testdir)
    testdir.mkdir(parents=True)

    sftp_port, sftp_sock, host_key = _start_sftp_server()
    home = testdir / "home"
    ssh_dir = home / ".ssh"
    ssh_dir.mkdir(parents=True)
    (ssh_dir / "known_hosts").write_text(_known_hosts_line(host_key, sftp_port), encoding="utf-8")
    (ssh_dir / "known_hosts").chmod(0o600)

    env = dict(os.environ)
    env.pop("SSH_AUTH_SOCK", None)
    env.pop("SSH_AGENT_PID", None)
    env["HOME"] = str(home)
    java_opts = env.get("JAVA_TOOL_OPTIONS", "")
    env["JAVA_TOOL_OPTIONS"] = (java_opts + " " if java_opts else "") + f"-Duser.home={home}"

    proc, port = _launch(env)
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []

            # Acquire a connection handle to localhost
            r = _call(s, "acquire", {"url": f"sftp://{TEST_USER}:{TEST_PASSWORD}@127.0.0.1:{sftp_port}/"})
            if "error" in r:
                print(f"[setup] acquire failed: {r['error']}")
                failures.append("setup: could not acquire SFTP connection handle")
                return 1
            handle = r["result"]["handleId"]
            print(f"[setup] acquired handle {handle!r}")

            def write_file(path, content=b""):
                """Create a file on the remote via open-write / write / close-write."""
                wr = _call(s, "open-write", {"handleId": handle, "path": path})
                if "error" in wr:
                    return False
                write_handle = wr["result"]["writeHandleId"]
                if content:
                    encoded = base64.b64encode(content).decode("ascii")
                    pushed = _call(s, "write", {"writeHandleId": write_handle, "data": encoded})
                    if "error" in pushed:
                        _call(s, "close-write", {"writeHandleId": write_handle})
                        return False
                closed = _call(s, "close-write", {"writeHandleId": write_handle})
                return "error" not in closed

            def read_file(path):
                rd = _call(s, "open-read", {"handleId": handle, "path": path})
                if "error" in rd:
                    return None
                read_handle = rd["result"]["readHandleId"]
                chunks = []
                try:
                    while True:
                        chunk = _call(s, "read", {"readHandleId": read_handle, "maxBytes": 8192})
                        if "error" in chunk:
                            return None
                        result = chunk["result"]
                        if result.get("data"):
                            chunks.append(base64.b64decode(result["data"]))
                        if result.get("eof") is True:
                            return b"".join(chunks)
                finally:
                    _call(s, "close-read", {"readHandleId": read_handle})

            # --- 02.35: rename(src, dst) renames a regular file ---
            src35 = _remote(testdir / "rename-file-src.txt")
            dst35 = _remote(testdir / "rename-file-dst.txt")
            content35 = b"rename preserves this regular file payload\n"
            made35 = write_file(src35, content35)
            r35 = _call(s, "rename", {"handleId": handle, "src": src35, "dst": dst35})
            st35_src = _call(s, "stat", {"handleId": handle, "path": src35})
            st35_dst = _call(s, "stat", {"handleId": handle, "path": dst35})
            read35 = read_file(dst35)
            ok35 = (
                made35
                and "error" not in r35
                and "error" in st35_src
                and "error" not in st35_dst
                and st35_dst["result"].get("isDir") is False
                and st35_dst["result"].get("byteSize") == len(content35)
                and read35 == content35
            )
            print(f"[02.35] rename regular file: {'PASS' if ok35 else 'FAIL'}")
            if not ok35:
                failures.append(f"02.35: made={made35} r={r35} src_stat={st35_src} dst_stat={st35_dst} dst_content={read35!r}")

            # --- 02.36: rename(src, dst) renames a directory ---
            src36 = _remote(testdir / "rename-dir-src")
            dst36 = _remote(testdir / "rename-dir-dst")
            src36_child = _remote(testdir / "rename-dir-src" / "child.txt")
            dst36_child = _remote(testdir / "rename-dir-dst" / "child.txt")
            content36 = b"child file moved with renamed directory\n"
            made36_dir = _call(s, "create-dir", {"handleId": handle, "path": src36})
            made36_child = write_file(src36_child, content36)
            r36 = _call(s, "rename", {"handleId": handle, "src": src36, "dst": dst36})
            st36_src = _call(s, "stat", {"handleId": handle, "path": src36})
            st36_dst = _call(s, "stat", {"handleId": handle, "path": dst36})
            st36_src_child = _call(s, "stat", {"handleId": handle, "path": src36_child})
            st36_dst_child = _call(s, "stat", {"handleId": handle, "path": dst36_child})
            read36_child = read_file(dst36_child)
            ok36 = (
                "error" not in made36_dir
                and made36_child
                and "error" not in r36
                and "error" in st36_src
                and "error" not in st36_dst
                and st36_dst["result"].get("isDir") is True
                and "error" in st36_src_child
                and "error" not in st36_dst_child
                and st36_dst_child["result"].get("isDir") is False
                and st36_dst_child["result"].get("byteSize") == len(content36)
                and read36_child == content36
            )
            print(f"[02.36] rename directory: {'PASS' if ok36 else 'FAIL'}")
            if not ok36:
                failures.append(f"02.36: made_dir={made36_dir} made_child={made36_child} r={r36} src_stat={st36_src} dst_stat={st36_dst} src_child_stat={st36_src_child} dst_child_stat={st36_dst_child} dst_child_content={read36_child!r}")

            # --- 02.37: delete_file(path) removes a regular file ---
            path37 = _remote(testdir / "to-delete.txt")
            made37 = write_file(path37, b"delete this regular file\n")
            r37 = _call(s, "delete-file", {"handleId": handle, "path": path37})
            st37 = _call(s, "stat", {"handleId": handle, "path": path37})
            ok37 = made37 and "error" not in r37 and "error" in st37
            print(f"[02.37] delete_file removes file: {'PASS' if ok37 else 'FAIL'}")
            if not ok37:
                failures.append(f"02.37: made={made37} r={r37} stat={st37}")

            # --- 02.38: create_dir(path) creates directory including missing parents ---
            path38 = _remote(testdir / "deep" / "a" / "b" / "c")
            r38 = _call(s, "create-dir", {"handleId": handle, "path": path38})
            st38 = _call(s, "stat", {"handleId": handle, "path": path38})
            ok38 = (
                "error" not in r38
                and "error" not in st38
                and st38["result"].get("isDir") is True
            )
            print(f"[02.38] create_dir with missing parents: {'PASS' if ok38 else 'FAIL'}")
            if not ok38:
                failures.append(f"02.38: r={r38} stat={st38}")

            # --- 02.39: create_dir(path) succeeds when directory already exists ---
            path39 = _remote(testdir / "already-exists")
            made39 = _call(s, "create-dir", {"handleId": handle, "path": path39})
            st39_before = _call(s, "stat", {"handleId": handle, "path": path39})
            r39 = _call(s, "create-dir", {"handleId": handle, "path": path39})
            st39_after = _call(s, "stat", {"handleId": handle, "path": path39})
            ok39 = (
                "error" not in made39
                and "error" not in st39_before
                and st39_before["result"].get("isDir") is True
                and "error" not in r39
                and "error" not in st39_after
                and st39_after["result"].get("isDir") is True
            )
            print(f"[02.39] create_dir idempotent: {'PASS' if ok39 else 'FAIL'}")
            if not ok39:
                failures.append(f"02.39: made={made39} before={st39_before} r={r39} after={st39_after}")

            # --- 02.40: delete_dir(path) removes an empty directory ---
            path40 = _remote(testdir / "empty-dir")
            made40 = _call(s, "create-dir", {"handleId": handle, "path": path40})
            st40_before = _call(s, "stat", {"handleId": handle, "path": path40})
            r40 = _call(s, "delete-dir", {"handleId": handle, "path": path40})
            st40 = _call(s, "stat", {"handleId": handle, "path": path40})
            ok40 = (
                "error" not in made40
                and "error" not in st40_before
                and st40_before["result"].get("isDir") is True
                and "error" not in r40
                and "error" in st40
            )
            print(f"[02.40] delete_dir removes empty directory: {'PASS' if ok40 else 'FAIL'}")
            if not ok40:
                failures.append(f"02.40: made={made40} before={st40_before} r={r40} stat={st40}")

            # --- 02.41: set_mod_time is observable on a subsequent stat ---
            path41 = _remote(testdir / "mtime-file.txt")
            dir41 = _remote(testdir / "mtime-dir")
            made41_file = write_file(path41)
            made41_dir = _call(s, "create-dir", {"handleId": handle, "path": dir41})
            target_time = 946684800
            r41 = _call(s, "set-mod-time", {
                "handleId": handle,
                "path": path41,
                "modTimeEpochSeconds": target_time,
            })
            r41_dir = _call(s, "set-mod-time", {
                "handleId": handle,
                "path": dir41,
                "modTimeEpochSeconds": target_time,
            })
            st41 = _call(s, "stat", {"handleId": handle, "path": path41})
            st41_dir = _call(s, "stat", {"handleId": handle, "path": dir41})
            ok41 = (
                made41_file
                and "error" not in made41_dir
                and "error" not in r41
                and "error" not in r41_dir
                and "error" not in st41
                and "error" not in st41_dir
                and st41["result"].get("isDir") is False
                and st41["result"].get("modTimeEpochSeconds") == target_time
                and st41_dir["result"].get("isDir") is True
                and st41_dir["result"].get("modTimeEpochSeconds") == target_time
            )
            print(f"[02.41] set_mod_time observable on stat: {'PASS' if ok41 else 'FAIL'}")
            if not ok41:
                failures.append(f"02.41: made_file={made41_file} made_dir={made41_dir} file_set={r41} dir_set={r41_dir} file_stat={st41} dir_stat={st41_dir}")

            _call(s, "release", {"handleId": handle})

            if failures:
                print("\nFAILURES:")
                for f in failures:
                    print(f"  - {f}")
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
        sftp_sock.close()


if __name__ == "__main__":
    sys.exit(main())
