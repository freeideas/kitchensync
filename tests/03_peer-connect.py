#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = ["paramiko"]
# ///
"""Peer root auto-creation on connection for file:// and sftp:// peers (03.86, 03.87, 03.93)."""

from __future__ import annotations

import base64, os, re, shutil, socket, subprocess, sys, threading, time
from pathlib import Path

import paramiko

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

TMP = Path(PROJECT).resolve() / "tmp" / "testks" / "03_peer-connect"
TEST_USER = "peerconnect"
TEST_PASSWORD = "peer-connect-password"


def invoke(*peers, timeout=30, env=None):
    return subprocess.run(
        [str(UV), "run", "--script", str(BUILD_PY), "invoke-cli", PROJECT, *peers],
        capture_output=True, text=True, encoding="utf-8", timeout=timeout, env=env,
    )


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

    def list_folder(self, path):
        try:
            root = Path(self._path(path))
            entries = []
            for name in os.listdir(root):
                attrs = paramiko.SFTPAttributes.from_stat(os.lstat(root / name))
                attrs.filename = name
                entries.append(attrs)
            return entries
        except OSError as e:
            return paramiko.SFTPServer.convert_errno(e.errno)

    def open(self, path, flags, attr):
        try:
            fd = os.open(self._path(path), flags, getattr(attr, "st_mode", None) or 0o666)
            mode = "r+b" if flags & os.O_RDWR else ("wb" if flags & os.O_WRONLY else "rb")
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


def _sftp_env(home: Path) -> dict[str, str]:
    env = dict(os.environ)
    env.pop("SSH_AUTH_SOCK", None)
    env.pop("SSH_AGENT_PID", None)
    env["HOME"] = str(home)
    java_opts = env.get("JAVA_TOOL_OPTIONS", "")
    env["JAVA_TOOL_OPTIONS"] = (java_opts + " " if java_opts else "") + f"-Duser.home={home}"
    return env


def _read_text(path: Path) -> str | None:
    if not path.is_file():
        return None
    return path.read_text(encoding="utf-8")


def _scrub_java(source: str) -> str:
    out = list(source)
    i = 0
    state = "code"
    while i < len(source):
        ch = source[i]
        nxt = source[i + 1] if i + 1 < len(source) else ""
        if state == "code":
            if ch == "/" and nxt == "/":
                out[i] = out[i + 1] = " "
                i += 2
                state = "line_comment"
            elif ch == "/" and nxt == "*":
                out[i] = out[i + 1] = " "
                i += 2
                state = "block_comment"
            elif source.startswith('"""', i):
                out[i] = out[i + 1] = out[i + 2] = " "
                i += 3
                state = "text_block"
            elif ch == '"':
                out[i] = " "
                i += 1
                state = "string"
            elif ch == "'":
                out[i] = " "
                i += 1
                state = "char"
            else:
                i += 1
        elif state == "line_comment":
            if ch == "\n":
                state = "code"
            else:
                out[i] = " "
            i += 1
        elif state == "block_comment":
            out[i] = " " if ch != "\n" else "\n"
            if ch == "*" and nxt == "/":
                out[i + 1] = " "
                i += 2
                state = "code"
            else:
                i += 1
        elif state == "text_block":
            if source.startswith('"""', i):
                out[i] = out[i + 1] = out[i + 2] = " "
                i += 3
                state = "code"
            else:
                out[i] = " " if ch != "\n" else "\n"
                i += 1
        elif state == "string":
            out[i] = " " if ch != "\n" else "\n"
            if ch == "\\":
                if i + 1 < len(source):
                    out[i + 1] = " " if source[i + 1] != "\n" else "\n"
                i += 2
            elif ch == '"':
                i += 1
                state = "code"
            else:
                i += 1
        elif state == "char":
            out[i] = " " if ch != "\n" else "\n"
            if ch == "\\":
                if i + 1 < len(source):
                    out[i + 1] = " " if source[i + 1] != "\n" else "\n"
                i += 2
            elif ch == "'":
                i += 1
                state = "code"
            else:
                i += 1
    return "".join(out)


def _matching(text: str, open_index: int, open_ch: str = "{", close_ch: str = "}") -> int | None:
    depth = 0
    for i in range(open_index, len(text)):
        if text[i] == open_ch:
            depth += 1
        elif text[i] == close_ch:
            depth -= 1
            if depth == 0:
                return i
    return None


def _run_method_body() -> str | None:
    code_dir = Path(PROJECT) / "code"
    for path in code_dir.rglob("*.java"):
        try:
            source = path.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        scrubbed = _scrub_java(source)
        match = re.search(
            r"(?:^|[;{}\n]\s*)(?:public|private|protected|static|final|\s)+"
            r"[A-Za-z_$][\w$<>\[\], ?.&]*\s+run\s*\([^;{}]*\)\s*"
            r"(?:throws\s+[A-Za-z_$][\w$.,\s<>]*)?\{",
            scrubbed,
            re.MULTILINE,
        )
        if match:
            open_brace = match.end() - 1
            close_brace = _matching(scrubbed, open_brace)
            if close_brace is not None:
                return scrubbed[open_brace : close_brace + 1]
    return None


def _for_bodies(text: str) -> list[str]:
    bodies = []
    pos = 0
    while True:
        match = re.search(r"\bfor\s*\(", text[pos:])
        if match is None:
            return bodies
        open_paren = pos + match.end() - 1
        close_paren = _matching(text, open_paren, "(", ")")
        if close_paren is None:
            pos = open_paren + 1
            continue
        open_brace = close_paren + 1
        while open_brace < len(text) and text[open_brace].isspace():
            open_brace += 1
        if open_brace >= len(text) or text[open_brace] != "{":
            pos = close_paren + 1
            continue
        close_brace = _matching(text, open_brace)
        if close_brace is not None:
            bodies.append(text[open_brace + 1 : close_brace])
            pos = close_brace + 1
        else:
            pos = open_brace + 1


def _startup_connect_concurrency_failure() -> str | None:
    body = _run_method_body()
    if body is None:
        return "03.93: could not find startup run method in Java source"

    concurrent_patterns = [
        r"\b[A-Za-z_$][\w$]*\.submit\s*\([^;{}]*\bconnectPeer\s*\(",
        r"\bCompletableFuture\.(?:supplyAsync|runAsync)\s*\([^;{}]*\bconnectPeer\s*\(",
        r"\binvokeAll\s*\([^;{}]*\bconnectPeer\s*\(",
        r"\.parallel(?:Stream)?\s*\([^;{}]*\bconnectPeer\s*\(",
    ]
    has_concurrent_start = any(re.search(pattern, body, re.DOTALL) for pattern in concurrent_patterns)
    if not has_concurrent_start:
        return (
            "03.93: startup peer connection attempts are not issued through a "
            "concurrent construct around connectPeer"
        )

    for loop_body in _for_bodies(body):
        inline_await = re.search(
            r"\b(?:submit|supplyAsync|runAsync)\s*\([^;{}]*\bconnectPeer\s*\([^;{}]*\)\s*\)\s*\.\s*(?:get|join)\s*\(",
            loop_body,
            re.DOTALL,
        )
        named_future_await = re.search(
            r"\b(?:Future|CompletableFuture)\b[^;=]*\b([A-Za-z_$][\w$]*)\s*=\s*"
            r"[^;{}]*(?:submit|supplyAsync|runAsync)\s*\([^;{}]*\bconnectPeer\s*\([^;{}]*\)\s*\)"
            r"[^;]*;\s*.*?\b\1\s*\.\s*(?:get|join)\s*\(",
            loop_body,
            re.DOTALL,
        )
        if inline_await or named_future_await:
            return (
                "03.93: startup loop appears to await a peer connection in the same "
                "loop body that starts it"
            )
    return None


def main() -> int:
    if TMP.exists():
        shutil.rmtree(TMP)
    TMP.mkdir(parents=True)

    failures = []
    sftp_sock = None

    try:
        sftp_port, sftp_sock, host_key = _start_sftp_server()
        home = TMP / "home"
        ssh_dir = home / ".ssh"
        ssh_dir.mkdir(parents=True)
        (ssh_dir / "known_hosts").write_text(_known_hosts_line(host_key, sftp_port), encoding="utf-8", newline="\n")
        (ssh_dir / "known_hosts").chmod(0o600)
        sftp_env = _sftp_env(home)

        # --- 03.86a: file:// root path (including missing parents) created on connect ---
        src_a = TMP / "src-a"
        src_a.mkdir()
        (src_a / "hello.txt").write_text("hello")
        new_a = TMP / "new-a" / "deep" / "sub"
        # new_a and its parents do not exist — must be created before URL is considered connected

        proc = invoke("+" + src_a.as_uri(), new_a.as_uri())
        print(f"[03.86a] file:// root+parents created (exit {proc.returncode})")
        if not new_a.is_dir():
            failures.append(
                f"03.86a: file:// root {new_a} not created\n"
                f"  stdout: {proc.stdout!r}\n  stderr: {proc.stderr!r}"
            )
        if _read_text(new_a / "hello.txt") != "hello":
            failures.append(
                f"03.86a: file:// root was not connected and synced after creation\n"
                f"  stdout: {proc.stdout!r}\n  stderr: {proc.stderr!r}"
            )
        if proc.returncode != 0:
            failures.append(
                f"03.86a: exit {proc.returncode}\n"
                f"  stdout: {proc.stdout!r}\n  stderr: {proc.stderr!r}"
            )

        # --- 03.86b: sftp:// root path (including missing parents) created on connect ---
        src_b = TMP / "src-b"
        src_b.mkdir()
        (src_b / "hello.txt").write_text("hello")
        sftp_new = TMP / "sftp-new" / "deep" / "sub"
        # sftp_new and its parents do not exist — must be created via sftp before URL is connected

        sftp_url = f"sftp://{TEST_USER}:{TEST_PASSWORD}@127.0.0.1:{sftp_port}{sftp_new}"
        proc = invoke("+" + src_b.as_uri(), sftp_url, env=sftp_env)
        print(f"[03.86b] sftp:// root+parents created (exit {proc.returncode})")
        if not sftp_new.is_dir():
            failures.append(
                f"03.86b: sftp:// root {sftp_new} not created\n"
                f"  stdout: {proc.stdout!r}\n  stderr: {proc.stderr!r}"
            )
        if _read_text(sftp_new / "hello.txt") != "hello":
            failures.append(
                f"03.86b: sftp:// root was not connected and synced after creation\n"
                f"  stdout: {proc.stdout!r}\n  stderr: {proc.stderr!r}"
            )
        if proc.returncode != 0:
            failures.append(
                f"03.86b: exit {proc.returncode}\n"
                f"  stdout: {proc.stdout!r}\n  stderr: {proc.stderr!r}"
            )

        # --- 03.87: root creation failure → fallback URL tried ---
        src_c = TMP / "src-c"
        src_c.mkdir()
        (src_c / "hello.txt").write_text("hello")
        obstruction = TMP / "obstruction"
        obstruction.write_text("not a dir")     # regular file; mkdir through it must fail
        bad_peer = obstruction / "sub"          # uncreatable: parent is a file
        good_peer = TMP / "fallback-good"       # does not exist; will be created per 03.86

        bracket = f"[{bad_peer.as_uri()},{good_peer.as_uri()}]"
        proc = invoke("+" + src_c.as_uri(), bracket)
        print(f"[03.87] creation failure → fallback tried (exit {proc.returncode})")
        if not good_peer.is_dir():
            failures.append(
                f"03.87: fallback path {good_peer} not created after primary creation failed\n"
                f"  stdout: {proc.stdout!r}\n  stderr: {proc.stderr!r}"
            )
        if _read_text(good_peer / "hello.txt") != "hello":
            failures.append(
                f"03.87: fallback URL was not connected and synced after primary creation failed\n"
                f"  stdout: {proc.stdout!r}\n  stderr: {proc.stderr!r}"
            )
        if proc.returncode != 0:
            failures.append(
                f"03.87: exit {proc.returncode} with fallback available\n"
                f"  stdout: {proc.stdout!r}\n  stderr: {proc.stderr!r}"
            )

        # --- 03.93: startup peer connection attempts issued concurrently ---
        concurrency_failure = _startup_connect_concurrency_failure()
        print(f"[03.93] startup peer connect concurrent source structure: {concurrency_failure is None}")
        if concurrency_failure is not None:
            failures.append(concurrency_failure)

    finally:
        if sftp_sock is not None:
            sftp_sock.close()
        shutil.rmtree(TMP, ignore_errors=True)

    if failures:
        print("\nFAILURES:")
        for f in failures:
            print(f"  - {f}")
        return 1
    print("\nAll assertions passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
