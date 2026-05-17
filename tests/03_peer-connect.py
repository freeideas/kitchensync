#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = ["paramiko"]
# ///

from __future__ import annotations

import os
import shutil
import socket
import subprocess
import sys
import threading
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

import paramiko

PROJECT_DIR = Path("C:/Users/human/Desktop/prjx/kitchensync")
JAVA = Path("C:/Users/human/Desktop/prjx/kitchensync/tools/compiler/jdk/bin/java.exe")
JAR = Path("C:/Users/human/Desktop/prjx/kitchensync/released/kitchensync.jar")
TEST_ROOT = PROJECT_DIR / "tests" / ".tmp" / "03_peer-connect"


def peer_url(path: Path) -> str:
    return path.resolve().as_uri()


def run_cli(
    *args: str,
    extra_jvm: list[str] | None = None,
    timeout: float = 60,
) -> subprocess.CompletedProcess[str]:
    jvm = extra_jvm or []
    try:
        return subprocess.run(
            [str(JAVA), *jvm, "-jar", str(JAR), *args],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout,
            check=False,
        )
    except subprocess.TimeoutExpired as exc:
        return subprocess.CompletedProcess(
            exc.cmd,
            124,
            exc.stdout or "",
            (exc.stderr or "") + f"\nTimed out after {timeout}s",
        )


def describe(result: subprocess.CompletedProcess[str]) -> str:
    return (
        f"exit={result.returncode}\n"
        f"stdout:\n{result.stdout[-2000:]}\n"
        f"stderr:\n{result.stderr[-2000:]}"
    )


def clean_dir(path: Path) -> None:
    if path.exists():
        shutil.rmtree(path)
    path.mkdir(parents=True, exist_ok=True)


def write_marker(root: Path, name: str, body: str) -> Path:
    root.mkdir(parents=True, exist_ok=True)
    p = root / name
    p.write_text(body, encoding="utf-8")
    return p


def check(condition: bool, failures: list[str], message: str) -> None:
    if not condition:
        failures.append(message)


# ---------------------------------------------------------------------------
# In-process SFTP server (paramiko)
# ---------------------------------------------------------------------------

class _ServerInterface(paramiko.ServerInterface):
    def check_channel_request(self, kind, chanid):
        return (
            paramiko.OPEN_SUCCEEDED
            if kind == "session"
            else paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED
        )

    def check_auth_password(self, username, password):
        return paramiko.AUTH_SUCCESSFUL

    def check_auth_none(self, username):
        return paramiko.AUTH_SUCCESSFUL

    def get_allowed_auths(self, username):
        return "none,password"


def _sftp_class_for(root: Path, deny_mkdir: bool = False) -> type:
    class _SFTP(paramiko.SFTPServerInterface):
        def _rp(self, path: str) -> str:
            rel = self.canonicalize(path).lstrip("/")
            return str(root / rel) if rel else str(root)

        def list_folder(self, path):
            rp = self._rp(path)
            try:
                out = []
                for name in os.listdir(rp):
                    attr = paramiko.SFTPAttributes.from_stat(
                        os.stat(os.path.join(rp, name))
                    )
                    attr.filename = name
                    out.append(attr)
                return out
            except OSError as e:
                return paramiko.SFTPServer.convert_errno(e.errno)

        def stat(self, path):
            try:
                return paramiko.SFTPAttributes.from_stat(os.stat(self._rp(path)))
            except OSError as e:
                return paramiko.SFTPServer.convert_errno(e.errno)

        def lstat(self, path):
            try:
                return paramiko.SFTPAttributes.from_stat(os.lstat(self._rp(path)))
            except OSError as e:
                return paramiko.SFTPServer.convert_errno(e.errno)

        def open(self, path, flags, attr):
            rp = self._rp(path)
            try:
                Path(rp).parent.mkdir(parents=True, exist_ok=True)
                binary = getattr(os, "O_BINARY", 0)
                fd = os.open(rp, flags | binary, 0o666)
            except OSError as e:
                return paramiko.SFTPServer.convert_errno(e.errno)
            if flags & os.O_WRONLY:
                fstr = "ab" if (flags & os.O_APPEND) else "wb"
            elif flags & os.O_RDWR:
                fstr = "a+b" if (flags & os.O_APPEND) else "r+b"
            else:
                fstr = "rb"
            try:
                f = os.fdopen(fd, fstr)
            except OSError as e:
                return paramiko.SFTPServer.convert_errno(e.errno)
            fobj = paramiko.SFTPHandle(flags)
            fobj.filename = rp
            fobj.readfile = f
            fobj.writefile = f
            return fobj

        def mkdir(self, path, attr):
            if deny_mkdir:
                return paramiko.SFTP_PERMISSION_DENIED
            try:
                os.makedirs(self._rp(path), exist_ok=True)
                return paramiko.SFTP_OK
            except OSError as e:
                return paramiko.SFTPServer.convert_errno(e.errno)

        def rmdir(self, path):
            try:
                os.rmdir(self._rp(path))
                return paramiko.SFTP_OK
            except OSError as e:
                return paramiko.SFTPServer.convert_errno(e.errno)

        def remove(self, path):
            try:
                os.unlink(self._rp(path))
                return paramiko.SFTP_OK
            except OSError as e:
                return paramiko.SFTPServer.convert_errno(e.errno)

        def rename(self, oldpath, newpath):
            try:
                os.rename(self._rp(oldpath), self._rp(newpath))
                return paramiko.SFTP_OK
            except OSError as e:
                return paramiko.SFTPServer.convert_errno(e.errno)

        def chattr(self, path, attr):
            try:
                paramiko.SFTPServer.set_file_attr(self._rp(path), attr)
                return paramiko.SFTP_OK
            except OSError as e:
                return paramiko.SFTPServer.convert_errno(e.errno)

    return _SFTP


def start_sftp_server(
    root: Path, deny_mkdir: bool = False
) -> tuple[int, str, threading.Event]:
    """Start SFTP server at root. Returns (port, known_hosts_line, stop_event)."""
    host_key = paramiko.RSAKey.generate(2048)
    stop = threading.Event()

    srv = socket.socket()
    srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, True)
    srv.bind(("127.0.0.1", 0))
    srv.listen(10)
    port = srv.getsockname()[1]

    sftp_cls = _sftp_class_for(root, deny_mkdir=deny_mkdir)

    def serve() -> None:
        srv.settimeout(1.0)
        transports: list[paramiko.Transport] = []
        while not stop.is_set():
            try:
                conn, _ = srv.accept()
            except socket.timeout:
                continue
            except OSError:
                break
            t = paramiko.Transport(conn)
            t.add_server_key(host_key)
            t.set_subsystem_handler("sftp", paramiko.SFTPServer, sftp_cls)
            t.start_server(server=_ServerInterface())
            transports.append(t)
        for t in transports:
            try:
                t.close()
            except Exception:
                pass
        srv.close()

    threading.Thread(target=serve, daemon=True).start()
    kh_line = f"[127.0.0.1]:{port} {host_key.get_name()} {host_key.get_base64()}"
    return port, kh_line, stop


def make_fake_home(base: Path, *kh_lines: str) -> Path:
    fake_home = base / "home"
    (fake_home / ".ssh").mkdir(parents=True, exist_ok=True)
    (fake_home / ".ssh" / "known_hosts").write_text(
        "\n".join(kh_lines) + "\n", encoding="utf-8"
    )
    return fake_home


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------

def test_file_peer_root_creation(failures: list[str]) -> None:
    """03.86 file://: missing root path and parents are created before connection succeeds"""
    root = TEST_ROOT / "file-create"
    clean_dir(root)
    canon = root / "canon"
    peer = root / "missing" / "parents" / "peer"
    marker = write_marker(canon, "from-canon.txt", "file peer root creation\n")

    result = run_cli(f"+{peer_url(canon)}", peer_url(peer))
    check(
        result.returncode == 0,
        failures,
        "file:// peer with missing root should sync successfully after creating the root\n"
        + describe(result),
    )
    check(peer.is_dir(), failures, f"file:// peer root was not created: {peer}")
    copied = peer / marker.name
    check(
        copied.exists()
        and copied.read_text(encoding="utf-8") == marker.read_text(encoding="utf-8"),
        failures,
        f"file:// peer root not usable after creation; missing copied marker: {copied}",
    )


def test_sftp_peer_root_creation(failures: list[str]) -> None:
    """03.86 sftp://: missing root path and parents are created on the SFTP peer before connection succeeds"""
    root = TEST_ROOT / "sftp-create"
    clean_dir(root)
    sftp_root = root / "sftp_root"
    sftp_root.mkdir()
    canon = root / "canon"
    marker = write_marker(canon, "sftp-marker.txt", "sftp root creation\n")

    port, kh_line, stop = start_sftp_server(sftp_root)
    try:
        fake_home = make_fake_home(root, kh_line)
        result = run_cli(
            f"+{peer_url(canon)}",
            f"sftp://ks_test:ks_test@127.0.0.1:{port}/sub/deep/peer",
            extra_jvm=[f"-Duser.home={str(fake_home)}"],
        )
        expected = sftp_root / "sub" / "deep" / "peer"
        check(
            result.returncode == 0,
            failures,
            "sftp:// peer with missing root should sync successfully after creating the root\n"
            + describe(result),
        )
        check(
            expected.is_dir(),
            failures,
            f"sftp:// peer root was not created at {expected}",
        )
        copied = expected / marker.name
        check(
            copied.exists()
            and copied.read_text(encoding="utf-8") == marker.read_text(encoding="utf-8"),
            failures,
            f"sftp:// peer root not usable after creation; missing marker: {copied}",
        )
    finally:
        stop.set()


def test_failed_root_creation_uses_fallback(failures: list[str]) -> None:
    """03.87 file://: failed root-path creation marks the URL as failed; next fallback URL is tried"""
    root = TEST_ROOT / "fallback"
    clean_dir(root)
    canon = root / "canon"
    marker = write_marker(canon, "fallback-marker.txt", "fallback after mkdir failure\n")
    blocked_parent = root / "blocked-parent"
    blocked_parent.write_text("not a directory\n", encoding="utf-8")
    bad_root = blocked_parent / "cannot-create"
    fallback_root = root / "fallback-root" / "nested"

    result = run_cli(
        f"+{peer_url(canon)}", f"[{peer_url(bad_root)},{peer_url(fallback_root)}]"
    )
    check(
        result.returncode == 0,
        failures,
        "failed root-path creation should fail only that URL and try the fallback\n"
        + describe(result),
    )
    check(
        fallback_root.is_dir(),
        failures,
        f"fallback file:// root was not created after first URL failed: {fallback_root}",
    )
    copied = fallback_root / marker.name
    check(
        copied.exists(),
        failures,
        f"fallback file:// URL was not used for sync; missing marker: {copied}",
    )


def test_sftp_failed_root_creation_uses_fallback(failures: list[str]) -> None:
    """03.87 sftp://: SFTP mkdir failure marks the URL as failed; next fallback URL is tried"""
    root = TEST_ROOT / "sftp-fallback"
    clean_dir(root)
    sftp_root = root / "sftp_root"
    sftp_root.mkdir()
    canon = root / "canon"
    marker = write_marker(canon, "sftp-fallback-marker.txt", "sftp fallback\n")
    fallback_local = root / "fallback-local" / "nested"

    port, kh_line, stop = start_sftp_server(sftp_root, deny_mkdir=True)
    try:
        fake_home = make_fake_home(root, kh_line)
        result = run_cli(
            f"+{peer_url(canon)}",
            f"[sftp://ks_test:ks_test@127.0.0.1:{port}/bad/root,{peer_url(fallback_local)}]",
            extra_jvm=[f"-Duser.home={str(fake_home)}"],
        )
        check(
            result.returncode == 0,
            failures,
            "sftp:// mkdir failure should fail only that URL and try the fallback\n"
            + describe(result),
        )
        check(
            fallback_local.is_dir(),
            failures,
            f"fallback local root not created after sftp:// URL failed: {fallback_local}",
        )
        copied = fallback_local / marker.name
        check(
            copied.exists(),
            failures,
            f"fallback URL was not used for sync; missing marker: {copied}",
        )
    finally:
        stop.set()


def main() -> int:
    failures: list[str] = []
    TEST_ROOT.mkdir(parents=True, exist_ok=True)

    test_file_peer_root_creation(failures)
    test_sftp_peer_root_creation(failures)
    test_failed_root_creation_uses_fallback(failures)
    test_sftp_failed_root_creation_uses_fallback(failures)
    # 03.93 source-code concurrent join/gather/parallel construct:
    # not reasonably testable through the root public CLI/artifact surface.

    if failures:
        print(f"{len(failures)} check(s) failed:")
        for index, failure in enumerate(failures, 1):
            print(f"\n[{index}] {failure}")
        return 1

    print("03_peer-connect: all checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
