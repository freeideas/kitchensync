#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = ["paramiko==3.5.1"]
# ///
"""Logging: exact C/X lines, stdout/stderr routing, verbosity, pool events, and list errors."""

from __future__ import annotations

import errno
import logging
import os
import posixpath
import re
import shutil
import socket
import subprocess
import sys
import threading
from pathlib import Path

import paramiko
from paramiko import SFTPAttributes, SFTPHandle, SFTPServer

logging.getLogger("paramiko").setLevel(logging.CRITICAL + 1)

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

TMP = (Path(PROJECT) / "tmp" / "testks" / "03_logging").resolve()
TEST_HOME = TMP / "home"

USER = "logtest"
PASSWORD = "logpass"
HOST = "127.0.0.1"
POOL_RE = re.compile(r"endpoint=([^ ]+) connections=(\d+)/(\d+)")


def _invoke(
    args: list[str],
    timeout: int = 60,
    *,
    test_home: bool = False,
) -> subprocess.CompletedProcess[str]:
    env = os.environ.copy()
    env.pop("SSH_AUTH_SOCK", None)
    env.pop("JAVA_TOOL_OPTIONS", None)
    if test_home:
        env["JAVA_TOOL_OPTIONS"] = f"-Duser.home={TEST_HOME}"
    return subprocess.run(
        [str(UV), "run", "--script", str(BUILD_PY), "invoke-cli", PROJECT] + args,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        timeout=timeout,
        env=env,
    )


def _write(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")


def _line_count(stdout: str, expected: str) -> int:
    return sum(1 for line in stdout.splitlines() if line == expected)


def _sftp_url(port: int, path: Path, **query: int) -> str:
    rel = "/" + path.relative_to(TMP).as_posix()
    url = f"sftp://{USER}:{PASSWORD}@{HOST}:{port}{rel}"
    if query:
        url += "?" + "&".join(f"{key}={value}" for key, value in query.items())
    return url


class PasswordServer(paramiko.ServerInterface):
    def check_auth_password(self, username: str, password: str) -> int:
        if username == USER and password == PASSWORD:
            return paramiko.AUTH_SUCCESSFUL
        return paramiko.AUTH_FAILED

    def get_allowed_auths(self, username: str) -> str:
        return "password"

    def check_channel_request(self, kind: str, chanid: int) -> int:
        if kind == "session":
            return paramiko.OPEN_SUCCEEDED
        return paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED


class RootedSftp(paramiko.SFTPServerInterface):
    def __init__(self, server, root: str, fail_list_relatives: set[str]):
        super().__init__(server)
        self.root = Path(root).resolve()
        self.fail_list_relatives = fail_list_relatives

    def _relative(self, path: str) -> str:
        return posixpath.normpath("/" + path.lstrip("/")).lstrip("/")

    def _local(self, path: str) -> Path:
        local = (self.root / self._relative(path)).resolve()
        if local == self.root or self.root in local.parents:
            return local
        raise OSError(errno.EACCES, "path outside test root")

    @staticmethod
    def _attr(path: Path) -> SFTPAttributes:
        attrs = SFTPAttributes.from_stat(path.stat())
        attrs.filename = path.name
        return attrs

    def list_folder(self, path: str):
        if self._relative(path) in self.fail_list_relatives:
            return SFTPServer.convert_errno(errno.EACCES)
        try:
            return [self._attr(child) for child in self._local(path).iterdir()]
        except OSError as ex:
            return SFTPServer.convert_errno(ex.errno)

    def stat(self, path: str):
        try:
            return SFTPAttributes.from_stat(self._local(path).stat())
        except OSError as ex:
            return SFTPServer.convert_errno(ex.errno)

    def lstat(self, path: str):
        return self.stat(path)

    def open(self, path: str, flags: int, attr: SFTPAttributes):
        try:
            local = self._local(path)
            if flags & os.O_CREAT:
                local.parent.mkdir(parents=True, exist_ok=True)
            fd = os.open(local, flags, getattr(attr, "st_mode", None) or 0o666)
            mode = "r+b" if flags & os.O_RDWR else "wb" if flags & os.O_WRONLY else "rb"
            file_obj = os.fdopen(fd, mode)
            handle = SFTPHandle(flags)
            if "r" in mode or "+" in mode:
                handle.readfile = file_obj
            if "w" in mode or "+" in mode:
                handle.writefile = file_obj
            return handle
        except OSError as ex:
            return SFTPServer.convert_errno(ex.errno)

    def remove(self, path: str):
        try:
            self._local(path).unlink()
            return paramiko.SFTP_OK
        except OSError as ex:
            return SFTPServer.convert_errno(ex.errno)

    def rename(self, oldpath: str, newpath: str):
        try:
            os.replace(self._local(oldpath), self._local(newpath))
            return paramiko.SFTP_OK
        except OSError as ex:
            return SFTPServer.convert_errno(ex.errno)

    def posix_rename(self, oldpath: str, newpath: str):
        return self.rename(oldpath, newpath)

    def mkdir(self, path: str, attr: SFTPAttributes):
        try:
            self._local(path).mkdir(mode=getattr(attr, "st_mode", None) or 0o777)
            return paramiko.SFTP_OK
        except OSError as ex:
            return SFTPServer.convert_errno(ex.errno)

    def rmdir(self, path: str):
        try:
            self._local(path).rmdir()
            return paramiko.SFTP_OK
        except OSError as ex:
            return SFTPServer.convert_errno(ex.errno)

    def chattr(self, path: str, attr: SFTPAttributes):
        try:
            local = self._local(path)
            if attr.st_atime is not None or attr.st_mtime is not None:
                current = local.stat()
                atime = attr.st_atime if attr.st_atime is not None else current.st_atime
                mtime = attr.st_mtime if attr.st_mtime is not None else current.st_mtime
                os.utime(local, (atime, mtime))
            return paramiko.SFTP_OK
        except OSError as ex:
            return SFTPServer.convert_errno(ex.errno)


class LocalSftpServer:
    def __init__(self, root: Path, fail_list_relatives: set[str]):
        self.root = root
        self.fail_list_relatives = fail_list_relatives
        self.key = paramiko.RSAKey.generate(2048)
        self.sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self.sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self.sock.bind((HOST, 0))
        self.sock.listen(32)
        self.port = self.sock.getsockname()[1]
        self.transports: list[paramiko.Transport] = []
        self.stop = threading.Event()
        self.thread = threading.Thread(target=self._serve, daemon=True)
        self.thread.start()

    def _serve(self) -> None:
        self.sock.settimeout(0.2)
        while not self.stop.is_set():
            try:
                client, _ = self.sock.accept()
            except socket.timeout:
                continue
            except OSError:
                break
            transport = paramiko.Transport(client)
            transport.add_server_key(self.key)
            transport.set_subsystem_handler(
                "sftp",
                SFTPServer,
                RootedSftp,
                str(self.root),
                self.fail_list_relatives,
            )
            try:
                transport.start_server(server=PasswordServer())
                self.transports.append(transport)
            except Exception:
                transport.close()

    def write_known_hosts(self) -> None:
        ssh = TEST_HOME / ".ssh"
        ssh.mkdir(parents=True, exist_ok=True)
        (ssh / "known_hosts").write_text(
            f"[{HOST}]:{self.port} {self.key.get_name()} {self.key.get_base64()}\n",
            encoding="utf-8",
            newline="\n",
        )

    def close(self) -> None:
        self.stop.set()
        try:
            self.sock.close()
        except OSError:
            pass
        for transport in self.transports:
            transport.close()


def _pool_lines(stdout: str) -> list[tuple[str, int, int]]:
    lines: list[tuple[str, int, int]] = []
    for line in stdout.splitlines():
        match = POOL_RE.fullmatch(line)
        if match:
            lines.append((match.group(1), int(match.group(2)), int(match.group(3))))
    return lines


def main() -> int:
    if TMP.exists():
        shutil.rmtree(TMP)
    TMP.mkdir(parents=True)
    TEST_HOME.mkdir(parents=True)

    failures: list[str] = []
    stderr_runs: list[tuple[str, subprocess.CompletedProcess[str]]] = []
    fail_list_relatives: set[str] = set()
    sftp_server: LocalSftpServer | None = None

    try:
        # 03.78: one exact C line per copy decision, even with two receivers.
        cp1 = TMP / "copy" / "peer1"
        cp2 = TMP / "copy" / "peer2"
        cp3 = TMP / "copy" / "peer3"
        cp1.mkdir(parents=True)
        cp2.mkdir(parents=True)
        cp3.mkdir(parents=True)
        _write(cp1 / "hello.txt", "hello")
        copy_run = _invoke(["-vl", "info", "+" + cp1.as_uri(), cp2.as_uri(), cp3.as_uri()])
        stderr_runs.append(("copy/info", copy_run))
        copy_count = _line_count(copy_run.stdout, "C hello.txt")
        print(f"[03.78] exact 'C hello.txt' lines at info: {copy_count}")
        if copy_run.returncode != 0:
            failures.append(
                f"03.78 setup: copy sync failed (exit {copy_run.returncode})\n"
                f"  stdout: {copy_run.stdout!r}\n  stderr: {copy_run.stderr!r}"
            )
        elif copy_count != 1:
            failures.append(
                f"03.78: expected exactly one 'C hello.txt' line, got {copy_count}\n"
                f"  stdout: {copy_run.stdout!r}"
            )

        # 03.79: one exact X line per displacement decision, even with two affected peers.
        dp1 = TMP / "disp" / "peer1"
        dp2 = TMP / "disp" / "peer2"
        dp3 = TMP / "disp" / "peer3"
        dp1.mkdir(parents=True)
        dp2.mkdir(parents=True)
        dp3.mkdir(parents=True)
        _write(dp2 / "extra.txt", "subordinate 2")
        _write(dp3 / "extra.txt", "subordinate 3")
        disp_run = _invoke(["-vl", "info", "+" + dp1.as_uri(), "-" + dp2.as_uri(), "-" + dp3.as_uri()])
        stderr_runs.append(("displacement/info", disp_run))
        disp_count = _line_count(disp_run.stdout, "X extra.txt")
        print(f"[03.79] exact 'X extra.txt' lines at info: {disp_count}")
        if disp_run.returncode != 0:
            failures.append(
                f"03.79 setup: displacement sync failed (exit {disp_run.returncode})\n"
                f"  stdout: {disp_run.stdout!r}\n  stderr: {disp_run.stderr!r}"
            )
        elif disp_count != 1:
            failures.append(
                f"03.79: expected exactly one 'X extra.txt' line, got {disp_count}\n"
                f"  stdout: {disp_run.stdout!r}"
            )

        # 03.81: C and X progress lines are suppressed at error verbosity.
        ep1 = TMP / "error-vl" / "peer1"
        ep2 = TMP / "error-vl" / "peer2"
        ep1.mkdir(parents=True)
        ep2.mkdir(parents=True)
        _write(ep1 / "copy.txt", "copy")
        _write(ep2 / "extra.txt", "extra")
        error_vl_run = _invoke(["-vl", "error", "+" + ep1.as_uri(), "-" + ep2.as_uri()])
        stderr_runs.append(("copy-displace/error", error_vl_run))
        c_or_x = [line for line in error_vl_run.stdout.splitlines() if line.startswith(("C ", "X "))]
        print(f"[03.81] C/X lines at -vl error: {c_or_x!r}")
        if error_vl_run.returncode != 0:
            failures.append(
                f"03.81 setup: error-verbosity sync failed (exit {error_vl_run.returncode})\n"
                f"  stdout: {error_vl_run.stdout!r}\n  stderr: {error_vl_run.stderr!r}"
            )
        elif c_or_x:
            failures.append(
                f"03.81: C/X progress lines appeared at -vl error: {c_or_x!r}\n"
                f"  stdout: {error_vl_run.stdout!r}"
            )

        sftp_server = LocalSftpServer(TMP, fail_list_relatives)
        sftp_server.write_known_hosts()

        # 03.82 / 03.84: trace shows acquire and release pool events with endpoint key format.
        ts = TMP / "trace" / "sftp"
        tf = TMP / "trace" / "file"
        ts.mkdir(parents=True)
        tf.mkdir(parents=True)
        _write(ts / "trace.txt", "trace")
        trace_run = _invoke(
            ["-vl", "trace", "+" + _sftp_url(sftp_server.port, ts, mc=2), tf.as_uri()],
            test_home=True,
        )
        trace_pool = _pool_lines(trace_run.stdout)
        expected_endpoint = f"{USER}@{HOST}"
        has_acquire = any(endpoint == expected_endpoint and count > 0 and max_count == 2 for endpoint, count, max_count in trace_pool)
        has_release = any(endpoint == expected_endpoint and count == 0 and max_count == 2 for endpoint, count, max_count in trace_pool)
        formatted_pool_lines = [line for line in trace_run.stdout.splitlines() if "endpoint=" in line or "connections=" in line]
        all_pool_lines_formatted = all(POOL_RE.fullmatch(line) for line in formatted_pool_lines)
        keyed_by_user_host = all(endpoint == expected_endpoint for endpoint, _, _ in trace_pool)
        print(f"[03.82] trace pool acquire={has_acquire} release={has_release}")
        print(f"[03.84] pool lines formatted={all_pool_lines_formatted} keyed={keyed_by_user_host}")
        if trace_run.returncode != 0:
            failures.append(
                f"03.82 setup: trace SFTP sync failed (exit {trace_run.returncode})\n"
                f"  stdout: {trace_run.stdout!r}\n  stderr: {trace_run.stderr!r}"
            )
        else:
            if not has_acquire or not has_release:
                failures.append(
                    "03.82: trace output did not include both acquire and release pool events\n"
                    f"  stdout: {trace_run.stdout!r}"
                )
            if not formatted_pool_lines or not all_pool_lines_formatted or not keyed_by_user_host:
                failures.append(
                    "03.84: pool events did not use "
                    "'endpoint=<user@host> connections=<n>/<max>' keyed by user+host\n"
                    f"  stdout: {trace_run.stdout!r}"
                )

        # 03.83: pool events are absent below trace, even when SFTP work occurs.
        for verbosity in ("error", "info", "debug"):
            src = TMP / "pool-hidden" / verbosity / "sftp"
            dst = TMP / "pool-hidden" / verbosity / "file"
            src.mkdir(parents=True)
            dst.mkdir(parents=True)
            _write(src / f"{verbosity}.txt", verbosity)
            hidden_run = _invoke(
                ["-vl", verbosity, "+" + _sftp_url(sftp_server.port, src, mc=2), dst.as_uri()],
                test_home=True,
            )
            hidden_pool_candidates = [
                line for line in hidden_run.stdout.splitlines()
                if "endpoint=" in line or "connections=" in line
            ]
            print(f"[03.83] pool lines at -vl {verbosity}: {hidden_pool_candidates!r}")
            if hidden_run.returncode != 0:
                failures.append(
                    f"03.83 setup: -vl {verbosity} SFTP sync failed (exit {hidden_run.returncode})\n"
                    f"  stdout: {hidden_run.stdout!r}\n  stderr: {hidden_run.stderr!r}"
                )
            elif hidden_pool_candidates:
                failures.append(
                    f"03.83: pool events appeared at -vl {verbosity}: {hidden_pool_candidates!r}\n"
                    f"  stdout: {hidden_run.stdout!r}"
                )

        # 03.85: a list_dir failure logs the affected peer and directory at error verbosity.
        lf_sftp = TMP / "listfail" / "sftp"
        lf_file = TMP / "listfail" / "file"
        (lf_sftp / "bad").mkdir(parents=True)
        (lf_file / "bad").mkdir(parents=True)
        _write(lf_file / "bad" / "kept.txt", "kept")
        fail_list_relatives.add((lf_sftp / "bad").relative_to(TMP).as_posix())
        listfail_run = _invoke(
            ["-vl", "error", "+" + lf_file.as_uri(), _sftp_url(sftp_server.port, lf_sftp)],
            test_home=True,
        )
        listfail_lines = [
            line for line in listfail_run.stdout.splitlines()
            if "listing failed" in line and "bad" in line and USER in line and HOST in line
        ]
        print(f"[03.85] list_dir failure lines with peer+dir: {listfail_lines!r}")
        if listfail_run.returncode != 0:
            failures.append(
                f"03.85 setup: list-failure sync failed (exit {listfail_run.returncode})\n"
                f"  stdout: {listfail_run.stdout!r}\n  stderr: {listfail_run.stderr!r}"
            )
        elif len(listfail_lines) != 1:
            failures.append(
                "03.85: expected exactly one error log identifying the affected peer and directory\n"
                f"  stdout: {listfail_run.stdout!r}"
            )

        # 03.80: the expected log lines for copy, displacement, trace pool, and list errors
        # are all observed on stdout by the checks above.
        stdout_logged = (
            copy_count == 1
            and disp_count == 1
            and has_acquire
            and has_release
            and len(listfail_lines) == 1
        )
        print(f"[03.80] expected log output observed on stdout: {stdout_logged}")
        if not stdout_logged:
            failures.append("03.80: one or more expected log messages were not observed on stdout")

        # 03.90: stderr remains empty for sync runs. This assertion uses file://
        # runs because the SFTP fixture must set user.home for known_hosts through
        # JVM launcher options, which can produce harness stderr before KitchenSync starts.
        for label, run in stderr_runs:
            stderr_empty = run.stderr == ""
            print(f"[03.90] {label} stderr empty: {stderr_empty}")
            if not stderr_empty:
                failures.append(f"03.90: stderr was not empty for {label}: {run.stderr!r}")

    finally:
        if sftp_server is not None:
            sftp_server.close()
        shutil.rmtree(TMP, ignore_errors=True)

    if failures:
        print("\nFAILURES:")
        for failure in failures:
            print(f"  - {failure}")
        return 1
    print("\nAll assertions passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
