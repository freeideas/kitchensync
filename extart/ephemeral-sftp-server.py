# /// script
# requires-python = ">=3.11"
# dependencies = ["paramiko>=3.4", "cryptography"]
# ///
"""Ephemeral in-process SFTP server for tests.

Binds 127.0.0.1 on an OS-assigned (ephemeral) port, prints that port as a single
line to stdout, and serves SFTP out of a fresh temporary directory. Everything
written through the server lives only in that temp directory, which is removed
when the process stops -- so uploaded files vanish on shutdown.

A test harness launches this through the bundled uv, e.g.

    aitc/bin/uv.<plat> run --script extart/ephemeral-sftp-server.py

reads the first stdout line to learn the port, connects an SFTP client to
127.0.0.1:<port>, does its work, then terminates the process.

stdout carries exactly one line -- the port number, nothing else -- so it parses
cleanly. The temp root, host-key fingerprint, and auth mode go to stderr.

Authentication (simple by default, overridable):
  (default)            accept any username with any password
  --password PW        accept any username only with password PW
  --authorized-key F   accept only the public key in F (one OpenSSH public-key
                       line) and reject passwords -- e.g. to exercise an
                       Ed25519-only key-auth path
"""

from __future__ import annotations

import argparse
import atexit
import base64
import os
import shutil
import signal
import socket
import sys
import tempfile
import threading
import time
from pathlib import Path

import paramiko

_KEY_CLASSES = {
    "ssh-ed25519": paramiko.Ed25519Key,
    "ssh-rsa": paramiko.RSAKey,
    "ecdsa-sha2-nistp256": paramiko.ECDSAKey,
    "ecdsa-sha2-nistp384": paramiko.ECDSAKey,
    "ecdsa-sha2-nistp521": paramiko.ECDSAKey,
}

_root: str | None = None
_IDLE_SECONDS = 60.0
_activity_lock = threading.Lock()
_last_activity = time.monotonic()
_active_connections = 0


def _eprint(*args: object) -> None:
    print(*args, file=sys.stderr, flush=True)


def _cleanup() -> None:
    if _root and os.path.isdir(_root):
        shutil.rmtree(_root, ignore_errors=True)


def _touch() -> None:
    global _last_activity
    with _activity_lock:
        _last_activity = time.monotonic()


def _connection_opened() -> None:
    global _active_connections, _last_activity
    with _activity_lock:
        _active_connections += 1
        _last_activity = time.monotonic()


def _connection_closed() -> None:
    global _active_connections, _last_activity
    with _activity_lock:
        _active_connections -= 1
        _last_activity = time.monotonic()


def _idle_timed_out() -> bool:
    with _activity_lock:
        return (
            _active_connections == 0
            and time.monotonic() - _last_activity >= _IDLE_SECONDS
        )


def _load_host_key(path: str) -> paramiko.PKey:
    errors = []
    for cls in (paramiko.Ed25519Key, paramiko.ECDSAKey, paramiko.RSAKey):
        try:
            return cls.from_private_key_file(path)
        except Exception as exc:  # noqa: BLE001 -- try the next key type
            errors.append(f"{cls.__name__}: {exc}")
    raise SystemExit(f"could not load host key {path}: " + "; ".join(errors))


def _load_authorized_key(path: str) -> paramiko.PKey:
    parts = Path(path).read_text(encoding="ascii").split()
    if len(parts) < 2:
        raise SystemExit(f"not an OpenSSH public key line: {path}")
    key_type, blob_b64 = parts[0], parts[1]
    cls = _KEY_CLASSES.get(key_type)
    if cls is None:
        raise SystemExit(f"unsupported authorized key type: {key_type}")
    return cls(data=base64.b64decode(blob_b64))


class _Server(paramiko.ServerInterface):
    def __init__(
        self,
        user: str | None,
        password: str | None,
        authorized_key: paramiko.PKey | None,
    ) -> None:
        self._user = user
        self._password = password
        self._authorized_key = authorized_key
        # publickey is offered only when an authorized key is configured; password
        # is offered unless we are in publickey-only mode (a key but no password).
        # So: key + no password -> key only (the saved-key / Ed25519 case);
        # password only -> password; both -> either (fallback); neither -> any
        # password.
        self._publickey_enabled = authorized_key is not None
        self._password_enabled = authorized_key is None or password is not None

    def check_channel_request(self, kind: str, chanid: int) -> int:
        if kind == "session":
            return paramiko.OPEN_SUCCEEDED
        return paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED

    def _user_ok(self, username: str) -> bool:
        return self._user is None or username == self._user

    def get_allowed_auths(self, username: str) -> str:
        methods = []
        if self._publickey_enabled:
            methods.append("publickey")
        if self._password_enabled:
            methods.append("password")
        return ",".join(methods) or "none"

    def check_auth_password(self, username: str, password: str) -> int:
        if not self._password_enabled or not self._user_ok(username):
            return paramiko.AUTH_FAILED
        if self._password is None or password == self._password:
            return paramiko.AUTH_SUCCESSFUL
        return paramiko.AUTH_FAILED

    def check_auth_publickey(self, username: str, key: paramiko.PKey) -> int:
        if not self._publickey_enabled or not self._user_ok(username):
            return paramiko.AUTH_FAILED
        if key.asbytes() == self._authorized_key.asbytes():
            return paramiko.AUTH_SUCCESSFUL
        return paramiko.AUTH_FAILED


class _Handle(paramiko.SFTPHandle):
    def stat(self):
        try:
            return paramiko.SFTPAttributes.from_stat(os.fstat(self.readfile.fileno()))
        except OSError as exc:
            return paramiko.SFTPServer.convert_errno(exc.errno)

    def chattr(self, attr):
        return paramiko.SFTP_OK


class _SFTP(paramiko.SFTPServerInterface):
    ROOT: str = ""

    def _real(self, path: str) -> str:
        _touch()
        # canonicalize() returns a normalized absolute posix path, so joining it
        # onto ROOT keeps every access inside the temp directory.
        return self.ROOT + self.canonicalize(path)

    def list_folder(self, path):
        real = self._real(path)
        try:
            out = []
            for name in os.listdir(real):
                attr = paramiko.SFTPAttributes.from_stat(os.stat(os.path.join(real, name)))
                attr.filename = name
                out.append(attr)
            return out
        except OSError as exc:
            return paramiko.SFTPServer.convert_errno(exc.errno)

    def stat(self, path):
        try:
            return paramiko.SFTPAttributes.from_stat(os.stat(self._real(path)))
        except OSError as exc:
            return paramiko.SFTPServer.convert_errno(exc.errno)

    def lstat(self, path):
        try:
            return paramiko.SFTPAttributes.from_stat(os.lstat(self._real(path)))
        except OSError as exc:
            return paramiko.SFTPServer.convert_errno(exc.errno)

    def open(self, path, flags, attr):
        real = self._real(path)
        try:
            fd = os.open(real, flags | getattr(os, "O_BINARY", 0), 0o666)
        except OSError as exc:
            return paramiko.SFTPServer.convert_errno(exc.errno)
        if (flags & os.O_CREAT) and attr is not None:
            attr._flags &= ~attr.FLAG_PERMISSIONS
            paramiko.SFTPServer.set_file_attr(real, attr)
        if flags & os.O_WRONLY:
            mode = "ab" if flags & os.O_APPEND else "wb"
        elif flags & os.O_RDWR:
            mode = "a+b" if flags & os.O_APPEND else "r+b"
        else:
            mode = "rb"
        try:
            handle_file = os.fdopen(fd, mode)
        except OSError as exc:
            return paramiko.SFTPServer.convert_errno(exc.errno)
        handle = _Handle(flags)
        handle.filename = real
        handle.readfile = handle_file
        handle.writefile = handle_file
        return handle

    def remove(self, path):
        try:
            os.remove(self._real(path))
        except OSError as exc:
            return paramiko.SFTPServer.convert_errno(exc.errno)
        return paramiko.SFTP_OK

    def rename(self, oldpath, newpath):
        try:
            os.rename(self._real(oldpath), self._real(newpath))
        except OSError as exc:
            return paramiko.SFTPServer.convert_errno(exc.errno)
        return paramiko.SFTP_OK

    def mkdir(self, path, attr):
        real = self._real(path)
        try:
            os.mkdir(real)
            if attr is not None:
                paramiko.SFTPServer.set_file_attr(real, attr)
        except OSError as exc:
            return paramiko.SFTPServer.convert_errno(exc.errno)
        return paramiko.SFTP_OK

    def rmdir(self, path):
        try:
            os.rmdir(self._real(path))
        except OSError as exc:
            return paramiko.SFTPServer.convert_errno(exc.errno)
        return paramiko.SFTP_OK

    def chattr(self, path, attr):
        try:
            paramiko.SFTPServer.set_file_attr(self._real(path), attr)
        except OSError as exc:
            return paramiko.SFTPServer.convert_errno(exc.errno)
        return paramiko.SFTP_OK


def _serve(client: socket.socket, host_key: paramiko.PKey, server: _Server) -> None:
    transport = paramiko.Transport(client)
    try:
        transport.add_server_key(host_key)
        transport.set_subsystem_handler("sftp", paramiko.SFTPServer, _SFTP)
        transport.start_server(server=server)
        channel = transport.accept(timeout=30)
        if channel is None:
            return
        while transport.is_active():
            time.sleep(0.2)
    except Exception as exc:  # noqa: BLE001 -- one bad client must not kill the server
        _eprint(f"connection error: {exc}")
    finally:
        try:
            transport.close()
        except Exception:  # noqa: BLE001
            pass
        _connection_closed()


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Ephemeral in-process SFTP server for tests.")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--user", default=None,
                        help="require this username (default: accept any username)")
    parser.add_argument("--password", default=None,
                        help="accept this password (default: accept any password, "
                             "unless --authorized-key makes it key-only)")
    parser.add_argument("--authorized-key", default=None,
                        help="accept this OpenSSH public key; key-only unless "
                             "--password is also given")
    parser.add_argument("--host-key", default=None,
                        help="use this private host key file (Ed25519/ECDSA/RSA) "
                             "instead of a freshly generated one")
    args = parser.parse_args(argv)

    global _root
    listener: socket.socket | None = None
    try:
        _root = tempfile.mkdtemp(prefix="ephemeral-sftp-")
        _SFTP.ROOT = _root
        atexit.register(_cleanup)

        def _handle_signal(_signum: int, _frame: object) -> None:
            _cleanup()
            os._exit(0)

        signal.signal(signal.SIGTERM, _handle_signal)
        signal.signal(signal.SIGINT, _handle_signal)

        # Parent-death watchdog. Tests launch this server through the bundled `uv`,
        # which runs it as a child process. A test that tears down by killing the
        # `uv` wrapper (not this process) would otherwise orphan the server, leaving
        # it looping forever in accept() with its temp directory on disk -- the
        # source of leaked server processes across a test run. Watch for the parent
        # going away (the process is re-parented, so getppid() changes) and exit, so
        # no teardown style can leave a server behind.
        def _parent_death_watchdog(original_ppid: int) -> None:
            while True:
                time.sleep(0.5)
                if os.getppid() != original_ppid:
                    _cleanup()
                    os._exit(0)

        threading.Thread(
            target=_parent_death_watchdog,
            args=(os.getppid(),),
            daemon=True,
        ).start()

        authorized = _load_authorized_key(args.authorized_key) if args.authorized_key else None
        host_key = _load_host_key(args.host_key) if args.host_key else paramiko.RSAKey.generate(2048)

        listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        listener.bind((args.host, 0))
        listener.listen(100)
        listener.settimeout(1.0)
        port = listener.getsockname()[1]

        server = _Server(args.user, args.password, authorized)

        # The single machine-readable line. Everything else is human-facing stderr.
        print(port, flush=True)
        _touch()
        _eprint(f"sftp root: {_root}")
        # The host public key in OpenSSH format, so a test can build a known_hosts
        # entry: "[127.0.0.1]:<port> <this line>".
        _eprint(f"host key: {host_key.get_name()} {host_key.get_base64()}")
        _eprint(f"user: {args.user if args.user is not None else '(any)'}")
        _eprint(f"auth: {server.get_allowed_auths(args.user or '')}"
                + (f" (password '{args.password}')" if args.password is not None else "")
                + (" (publickey)" if authorized is not None else ""))

        while True:
            if _idle_timed_out():
                _eprint("idle timeout: no SFTP use for 60 seconds")
                return 0
            try:
                client, _addr = listener.accept()
            except socket.timeout:
                continue
            _connection_opened()
            threading.Thread(
                target=_serve,
                args=(client, host_key, server),
                daemon=True,
            ).start()
    except KeyboardInterrupt:
        return 0
    finally:
        if listener is not None:
            listener.close()
        _cleanup()


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
