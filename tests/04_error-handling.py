#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = ["paramiko==3.5.1"]
# ///

from __future__ import annotations

import errno
import os
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

import paramiko
from paramiko import SFTPAttributes, SFTPHandle, SFTPServerInterface
from paramiko.sftp import SFTP_FAILURE, SFTP_NO_SUCH_FILE, SFTP_OK


sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

PROJECT_DIR = Path("C:/Users/human/Desktop/prjx/kitchensync")
JAVA = PROJECT_DIR / "tools" / "compiler" / "jdk" / "bin" / "java.exe"
JAR = PROJECT_DIR / "released" / "kitchensync.jar"
SFTP_USER = "ks_test"
SFTP_PASSWORD = "ks_test"


def errno_to_sftp(error: int) -> int:
    if error == errno.ENOENT:
        return SFTP_NO_SUCH_FILE
    return SFTP_FAILURE


class Faults:
    def __init__(self) -> None:
        self.list_failures: set[str] = set()
        self.mtime_failures: set[str] = set()
        self.read_failures: set[str] = set()
        self.fail_snapshot_publish = False

    def norm(self, path: str) -> str:
        return "/" + path.strip("/")


class RootedSFTPHandle(SFTPHandle):
    def stat(self):
        try:
            file_obj = self.readfile or self.writefile
            return SFTPAttributes.from_stat(os.fstat(file_obj.fileno()))
        except OSError as exc:
            return errno_to_sftp(exc.errno)


class RootedSFTPServer(SFTPServerInterface):
    def __init__(self, server, root: Path, faults: Faults):
        super().__init__(server)
        self.root = root.resolve()
        self.faults = faults

    def _local(self, path: str) -> Path:
        resolved = (self.root / path.lstrip("/")).resolve()
        if resolved != self.root and self.root not in resolved.parents:
            raise OSError(errno.EACCES, "path escapes SFTP root")
        return resolved

    def _remote(self, path: str) -> str:
        return self.faults.norm(path)

    def canonicalize(self, path: str) -> str:
        return "/" + self._local(path).relative_to(self.root).as_posix()

    def list_folder(self, path: str):
        if self._remote(path) in self.faults.list_failures:
            return SFTP_FAILURE
        try:
            entries = []
            local = self._local(path)
            for name in os.listdir(local):
                attrs = SFTPAttributes.from_stat(os.stat(local / name))
                attrs.filename = name
                entries.append(attrs)
            return entries
        except OSError as exc:
            return errno_to_sftp(exc.errno)

    def stat(self, path: str):
        try:
            return SFTPAttributes.from_stat(os.stat(self._local(path)))
        except OSError as exc:
            return errno_to_sftp(exc.errno)

    def lstat(self, path: str):
        try:
            return SFTPAttributes.from_stat(os.lstat(self._local(path)))
        except OSError as exc:
            return errno_to_sftp(exc.errno)

    def open(self, path: str, flags: int, attr):
        if not (flags & (os.O_WRONLY | os.O_RDWR)) and self._remote(path) in self.faults.read_failures:
            return SFTP_FAILURE
        try:
            local = self._local(path)
            local.parent.mkdir(parents=True, exist_ok=True)
            fd = os.open(local, flags, getattr(attr, "st_mode", None) or 0o666)
            handle = RootedSFTPHandle(flags)
            if flags & os.O_RDWR:
                file_obj = os.fdopen(fd, "r+b", buffering=0)
                handle.readfile = file_obj
                handle.writefile = file_obj
            elif flags & os.O_WRONLY:
                handle.writefile = os.fdopen(fd, "wb", buffering=0)
            else:
                handle.readfile = os.fdopen(fd, "rb", buffering=0)
            return handle
        except OSError as exc:
            return errno_to_sftp(exc.errno)

    def remove(self, path: str):
        try:
            os.remove(self._local(path))
            return SFTP_OK
        except OSError as exc:
            return errno_to_sftp(exc.errno)

    def rename(self, oldpath: str, newpath: str):
        if (
            self.faults.fail_snapshot_publish
            and self._remote(newpath).endswith("/.kitchensync/snapshot.db")
        ):
            return SFTP_FAILURE
        try:
            old = self._local(oldpath)
            new = self._local(newpath)
            new.parent.mkdir(parents=True, exist_ok=True)
            os.replace(old, new)
            return SFTP_OK
        except OSError as exc:
            return errno_to_sftp(exc.errno)

    def mkdir(self, path: str, attr):
        try:
            os.mkdir(self._local(path), getattr(attr, "st_mode", None) or 0o777)
            return SFTP_OK
        except OSError as exc:
            return errno_to_sftp(exc.errno)

    def rmdir(self, path: str):
        try:
            os.rmdir(self._local(path))
            return SFTP_OK
        except OSError as exc:
            return errno_to_sftp(exc.errno)

    def chattr(self, path: str, attr):
        if attr.st_mtime is not None and self._remote(path) in self.faults.mtime_failures:
            return SFTP_FAILURE
        try:
            local = self._local(path)
            if attr.st_mode is not None:
                os.chmod(local, attr.st_mode)
            if attr.st_atime is not None and attr.st_mtime is not None:
                os.utime(local, (attr.st_atime, attr.st_mtime))
            return SFTP_OK
        except OSError as exc:
            return errno_to_sftp(exc.errno)


class PasswordServer(paramiko.ServerInterface):
    def check_auth_password(self, username: str, password: str) -> int:
        if username == SFTP_USER and password == SFTP_PASSWORD:
            return paramiko.AUTH_SUCCESSFUL
        return paramiko.AUTH_FAILED

    def get_allowed_auths(self, username: str) -> str:
        return "password"

    def check_channel_request(self, kind: str, chanid: int) -> int:
        if kind == "session":
            return paramiko.OPEN_SUCCEEDED
        return paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED


class SFTPFixture:
    def __init__(self, root: Path, faults: Faults | None = None):
        self.root = root
        self.faults = faults or Faults()
        self.host_key = paramiko.RSAKey.generate(2048)
        self._stop = threading.Event()
        self._threads: list[threading.Thread] = []
        self._sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self._sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self._sock.bind(("127.0.0.1", 0))
        self._sock.listen(100)
        self.port = self._sock.getsockname()[1]
        self._accept_thread = threading.Thread(target=self._accept_loop, daemon=True)

    def __enter__(self) -> SFTPFixture:
        self.root.mkdir(parents=True, exist_ok=True)
        self._accept_thread.start()
        return self

    def __exit__(self, _exc_type, _exc, _tb) -> None:
        self.close()

    def close(self) -> None:
        self._stop.set()
        try:
            self._sock.close()
        except OSError:
            pass
        self._accept_thread.join(timeout=2)
        for thread in self._threads:
            thread.join(timeout=2)

    def known_hosts_line(self) -> str:
        return f"[127.0.0.1]:{self.port} {self.host_key.get_name()} {self.host_key.get_base64()}\n"

    def url(self, path: str) -> str:
        return f"sftp://{SFTP_USER}:{SFTP_PASSWORD}@127.0.0.1:{self.port}{path}"

    def peer(self, path: str, canon: bool = False, subordinate: bool = False) -> str:
        prefix = "+" if canon else "-" if subordinate else ""
        return prefix + self.url(path)

    def _accept_loop(self) -> None:
        while not self._stop.is_set():
            try:
                client, _addr = self._sock.accept()
            except OSError:
                break
            thread = threading.Thread(target=self._serve_client, args=(client,), daemon=True)
            self._threads.append(thread)
            thread.start()

    def _serve_client(self, client: socket.socket) -> None:
        transport = paramiko.Transport(client)
        try:
            transport.add_server_key(self.host_key)
            transport.set_subsystem_handler(
                "sftp", paramiko.SFTPServer, RootedSFTPServer, self.root, self.faults
            )
            transport.start_server(server=PasswordServer())
            while not self._stop.is_set() and transport.is_active():
                time.sleep(0.05)
        except Exception:
            pass
        finally:
            transport.close()


class Checks:
    def __init__(self) -> None:
        self.failures: list[str] = []

    def ok(self, condition: bool, message: str) -> None:
        if not condition:
            self.failures.append(message)


def run_cli(*args: str, home: Path | None = None, timeout: int = 60) -> subprocess.CompletedProcess[str]:
    command = [str(JAVA), "-jar", str(JAR), *args]
    env = None
    if home is not None:
        env = {**os.environ, "HOME": str(home), "USERPROFILE": str(home)}
        env.pop("SSH_AUTH_SOCK", None)
        env.pop("SSH_AGENT_PID", None)
        command = [str(JAVA), f"-Duser.home={home}", "-jar", str(JAR), *args]
    return subprocess.run(
        command,
        cwd=str(PROJECT_DIR),
        env=env,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=timeout,
        check=False,
    )


def peer(root: Path, name: str) -> Path:
    path = root / name
    path.mkdir(parents=True, exist_ok=True)
    return path


def write_file(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def read_file(path: Path) -> str:
    return path.read_text(encoding="utf-8", errors="replace")


def remove_tree(path: Path) -> None:
    if not path.exists():
        return
    for item in path.rglob("*"):
        try:
            item.chmod(0o700)
        except OSError:
            pass
    shutil.rmtree(path)


def free_dead_sftp_url() -> str:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    return f"sftp://user:password@127.0.0.1:{port}/missing"


def combined_output(result: subprocess.CompletedProcess[str]) -> str:
    return result.stdout + result.stderr


def snapshot_bytes(root: Path) -> bytes | None:
    path = root / ".kitchensync" / "snapshot.db"
    if not path.is_file():
        return None
    return path.read_bytes()


def write_known_hosts(home: Path, *fixtures: SFTPFixture) -> None:
    ssh_dir = home / ".ssh"
    ssh_dir.mkdir(parents=True, exist_ok=True)
    (ssh_dir / "known_hosts").write_text(
        "".join(fixture.known_hosts_line() for fixture in fixtures),
        encoding="utf-8",
        newline="\n",
    )


def init_two_way(c: Checks, left: Path, right: Path, label: str) -> None:
    result = run_cli("-vl", "error", f"+{left}", str(right))
    c.ok(result.returncode == 0, f"{label}: initial canon sync should exit 0, got {result.returncode}\n{combined_output(result)}")


def check_reachability_failures(root: Path, c: Checks) -> None:
    dead = free_dead_sftp_url()

    p1 = peer(root, "reachable-skip-a")
    p2 = peer(root, "reachable-skip-b")
    write_file(p1 / "from-canon.txt", "canon survives unreachable peer\n")
    result = run_cli("-vl", "error", f"+{p1}", str(p2), dead)
    output = combined_output(result)
    c.ok(result.returncode == 0, f"04.7: unreachable non-canon peer should be skipped while run continues, got {result.returncode}\n{output}")
    c.ok((p2 / "from-canon.txt").is_file(), "04.7: remaining reachable peers should still synchronize")
    c.ok(output.strip() != "", "04.7: unreachable peer should produce an error-verbosity warning")

    p3 = peer(root, "too-few-reachable")
    result = run_cli("-vl", "error", str(p3), dead)
    c.ok(result.returncode == 1, f"04.8: fewer than two reachable peers should exit 1, got {result.returncode}\n{combined_output(result)}")

    p4 = peer(root, "canon-unreachable-a")
    p5 = peer(root, "canon-unreachable-b")
    result = run_cli("-vl", "error", f"+{dead}", str(p4), str(p5))
    c.ok(result.returncode == 1, f"04.9: unreachable + canon peer should exit 1, got {result.returncode}\n{combined_output(result)}")

    p6 = peer(root, "all-subordinate-explicit")
    p7 = peer(root, "all-subordinate-snapshotted")
    p8 = peer(root, "all-subordinate-auto")
    write_file(p6 / "seed.txt", "seed\n")
    init_two_way(c, p6, p7, "all-subordinate setup")
    result = run_cli("-vl", "error", f"-{p6}", str(p8))
    output = combined_output(result)
    c.ok(result.returncode == 1, f"04.10: all auto-subordinate peers should exit 1, got {result.returncode}\n{output}")
    c.ok(
        "No contributing peer reachable" in output and "cannot make sync decisions" in output,
        f"04.10: all-subordinate failure should print the required diagnostic\n{output}",
    )


def check_snapshot_download_failure(root: Path, c: Checks) -> None:
    p1 = peer(root, "snapshot-download-a")
    p2 = peer(root, "snapshot-download-b")
    p3 = peer(root, "snapshot-download-bad")
    write_file(p1 / "kept-going.txt", "reachable peers still sync\n")
    (p3 / ".kitchensync").mkdir(parents=True)
    (p3 / ".kitchensync" / "snapshot.db").mkdir()

    result = run_cli("-vl", "error", f"+{p1}", str(p2), str(p3))
    output = combined_output(result)
    c.ok(result.returncode == 0, f"04.17: bad snapshot download on one peer should exclude it while enough peers remain, got {result.returncode}\n{output}")
    c.ok((p2 / "kept-going.txt").is_file(), "04.17: reachable-count checks should be re-evaluated after excluding the peer")
    c.ok(output.strip() != "", "04.17: snapshot-download failure should be logged at error verbosity")
    c.ok((p3 / ".kitchensync" / "snapshot.db").is_dir(), "04.16: unreachable peer snapshot artifact should not be replaced or modified")

    p4 = peer(root, "snapshot-download-only-good")
    p5 = peer(root, "snapshot-download-too-few")
    (p5 / ".kitchensync").mkdir(parents=True)
    (p5 / ".kitchensync" / "snapshot.db").mkdir()
    result = run_cli("-vl", "error", str(p4), str(p5))
    c.ok(result.returncode == 1, f"04.17: excluding a snapshot-download-failed peer should trigger the fewer-than-two-reachable exit, got {result.returncode}\n{combined_output(result)}")

    src = peer(root, "snapshot-download-sftp-source")
    good = peer(root, "snapshot-download-sftp-good")
    sftp_root = root / "snapshot-download-sftp-root"
    home = root / "snapshot-download-home"
    faults = Faults()
    write_file(src / "initial.txt", "initial\n")
    with SFTPFixture(sftp_root, faults) as fixture:
        write_known_hosts(home, fixture)
        init = run_cli("-vl", "error", f"+{src}", str(good), fixture.peer("/bad-download"), home=home)
        c.ok(init.returncode == 0, f"04.16 setup: initial SFTP snapshot sync should exit 0, got {init.returncode}\n{combined_output(init)}")
        snapshot = sftp_root / "bad-download" / ".kitchensync" / "snapshot.db"
        before = snapshot.read_bytes()
        write_file(src / "after-download-failure.txt", "still syncs elsewhere\n")
        faults.read_failures.add("/bad-download/.kitchensync/snapshot.db")

        result = run_cli("-vl", "error", f"+{src}", str(good), fixture.peer("/bad-download"), home=home)
        output = combined_output(result)
        c.ok(result.returncode == 0, f"04.16/04.17: snapshot-download failed peer should be excluded while enough peers remain, got {result.returncode}\n{output}")
        c.ok((good / "after-download-failure.txt").is_file(), "04.17: remaining reachable peers should still synchronize after SFTP snapshot-download failure")
        c.ok(snapshot.read_bytes() == before, "04.16: unreachable peer snapshot rows should not be modified during the run")
        c.ok(not (sftp_root / "bad-download" / "after-download-failure.txt").exists(), "04.17: snapshot-download failed peer should be excluded from the reachable set")


def check_transfer_and_displacement_failures(root: Path, c: Checks) -> None:
    src = peer(root, "transfer-source")
    dst = peer(root, "transfer-dest")
    survivor = peer(root, "transfer-survivor")
    write_file(src / "seed.txt", "seed\n")
    result = run_cli("-vl", "error", f"+{src}", str(dst), str(survivor))
    c.ok(result.returncode == 0, f"transfer setup: initial canon sync should exit 0, got {result.returncode}\n{combined_output(result)}")

    write_file(src / "blocked.txt", "blocked by TMP obstruction\n")
    tmp_root = dst / ".kitchensync" / "TMP"
    if tmp_root.is_dir():
        shutil.rmtree(tmp_root)
    elif tmp_root.exists():
        tmp_root.unlink()
    tmp_root.write_text("not a directory\n", encoding="utf-8")

    result = run_cli("-vl", "error", f"+{src}", str(dst), str(survivor))
    output = combined_output(result)
    c.ok(result.returncode == 0, f"04.12/04.21: transfer failure for one file should not abort run, got {result.returncode}\n{output}")
    c.ok((survivor / "blocked.txt").is_file(), "04.12: other transfers should continue after a file transfer failure")
    c.ok(not (dst / "blocked.txt").exists(), "04.21: TMP staging creation failure should skip that file on the failing peer")
    c.ok(output.strip() != "", "04.12/04.21: transfer failure should be logged at error verbosity")

    canon = peer(root, "displace-canon")
    target = peer(root, "displace-target")
    write_file(canon / "replace.txt", "first\n")
    init_two_way(c, canon, target, "displacement setup")

    write_file(canon / "replace.txt", "canon replacement\n")
    write_file(target / "replace.txt", "target should remain\n")
    bak = target / ".kitchensync" / "BAK"
    if bak.is_dir():
        shutil.rmtree(bak)
    bak.write_text("not a directory\n", encoding="utf-8")

    result = run_cli("-vl", "error", f"+{canon}", str(target))
    output = combined_output(result)
    c.ok(result.returncode == 0, f"04.13/04.15: displacement failure should not abort run, got {result.returncode}\n{output}")
    c.ok(read_file(target / "replace.txt") == "target should remain\n", "04.13: failed displacement should leave the target file in place")
    c.ok(read_file(target / "replace.txt") != "canon replacement\n", "04.15: copy associated with a failed displacement should be skipped")
    tmp_root = target / ".kitchensync" / "TMP"
    staged_files = [p for p in tmp_root.rglob("*") if p.is_file()] if tmp_root.is_dir() else []
    c.ok(not staged_files, "04.15: failed displacement should not leave a staged TMP file for the skipped copy")
    c.ok(output.strip() != "", "04.13/04.15: displacement failure should be logged at error verbosity")


def check_listing_failures(root: Path, c: Checks) -> None:
    p1 = peer(root, "list-one-fails-a")
    p3 = peer(root, "list-one-fails-c")
    write_file(p1 / "blocked" / "old.txt", "old\n")
    sftp_root = root / "list-sftp-root"
    home = root / "list-home"
    faults = Faults()

    with SFTPFixture(sftp_root, faults) as fixture:
        write_known_hosts(home, fixture)
        init = run_cli("-vl", "error", f"+{p1}", fixture.peer("/list-one-fails-b"), str(p3), home=home)
        c.ok(init.returncode == 0, f"04.11 setup: initial sync should exit 0, got {init.returncode}\n{combined_output(init)}")

        before = snapshot_bytes(sftp_root / "list-one-fails-b")
        write_file(p1 / "blocked" / "new.txt", "new from peer one\n")
        faults.list_failures.add("/list-one-fails-b/blocked")
        result = run_cli("-vl", "error", f"+{p1}", fixture.peer("/list-one-fails-b"), str(p3), home=home)
        output = combined_output(result)
        c.ok(result.returncode == 0, f"04.11: one peer list_dir failure should not abort run, got {result.returncode}\n{output}")
        c.ok((p3 / "blocked" / "new.txt").is_file(), "04.11: other peers should still participate in decisions for the affected directory")
        c.ok(not (sftp_root / "list-one-fails-b" / "blocked" / "new.txt").exists(), "04.11: peer with list_dir failure should be excluded from that directory subtree")
        c.ok(snapshot_bytes(sftp_root / "list-one-fails-b") == before, "04.20: failed-listing peer snapshot rows for the affected subtree should not be modified")
        c.ok(output.strip() != "", "04.11: list_dir failure should be logged at error verbosity")

    sub = peer(root, "list-all-fail-subordinate")
    write_file(sftp_root / "list-all-fail-a" / "blocked" / "seed.txt", "seed\n")
    write_file(sftp_root / "list-all-fail-b" / "blocked" / "seed.txt", "seed\n")
    write_file(sub / "blocked" / "victim.txt", "must not be displaced\n")
    faults = Faults()
    home = root / "list-all-home"
    with SFTPFixture(sftp_root, faults) as fixture:
        write_known_hosts(home, fixture)
        init = run_cli(
            "-vl",
            "error",
            fixture.peer("/list-all-fail-a", canon=True),
            fixture.peer("/list-all-fail-b"),
            str(sub),
            home=home,
        )
        c.ok(init.returncode == 0, f"04.19 setup: initial sync should exit 0, got {init.returncode}\n{combined_output(init)}")
        write_file(sub / "blocked" / "victim.txt", "subordinate-only change must remain\n")

        faults.list_failures.update({"/list-all-fail-a/blocked", "/list-all-fail-b/blocked"})
        result = run_cli(
            "-vl",
            "error",
            fixture.peer("/list-all-fail-a"),
            fixture.peer("/list-all-fail-b"),
            f"-{sub}",
            home=home,
        )
        output = combined_output(result)
        c.ok(result.returncode == 0, f"04.19: all contributing peers failing one directory should skip that subtree, got {result.returncode}\n{output}")
        c.ok(read_file(sub / "blocked" / "victim.txt") == "subordinate-only change must remain\n", "04.19: subordinate files below the skipped directory should not be displaced")


def check_snapshot_upload_and_mtime_failures(root: Path, c: Checks) -> None:
    src = peer(root, "mtime-source")
    sftp_root = root / "mtime-sftp-root"
    home = root / "mtime-home"
    faults = Faults()
    write_file(src / "mtime.txt", "mtime copy survives\n")
    os.utime(src / "mtime.txt", (1_700_000_000, 1_700_000_000))
    faults.mtime_failures.add("/mtime-dest/mtime.txt")

    with SFTPFixture(sftp_root, faults) as fixture:
        write_known_hosts(home, fixture)
        result = run_cli("-vl", "error", f"+{src}", fixture.peer("/mtime-dest"), home=home)
        output = combined_output(result)
        copied = sftp_root / "mtime-dest" / "mtime.txt"
        c.ok(result.returncode == 0, f"04.14: set_mod_time failure should not abort run, got {result.returncode}\n{output}")
        c.ok(read_file(copied) == "mtime copy survives\n", "04.14: copy should not be undone when set_mod_time fails")
        c.ok(output.strip() != "", "04.14: set_mod_time failure should be logged at error verbosity")

        faults.mtime_failures.clear()
        result = run_cli("-vl", "error", f"+{src}", fixture.peer("/mtime-dest"), home=home)
        output = combined_output(result)
        c.ok(result.returncode == 0, f"04.14: next run after set_mod_time failure should complete, got {result.returncode}\n{output}")
        c.ok(abs(copied.stat().st_mtime - (src / "mtime.txt").stat().st_mtime) < 2, "04.14: next run should correct the modification-time discrepancy")

    upload_src = peer(root, "snapshot-upload-source")
    upload_root = root / "upload-sftp-root"
    upload_home = root / "upload-home"
    upload_faults = Faults()
    write_file(upload_src / "seed.txt", "seed\n")
    with SFTPFixture(upload_root, upload_faults) as fixture:
        write_known_hosts(upload_home, fixture)
        init = run_cli("-vl", "error", f"+{upload_src}", fixture.peer("/upload-dest"), home=upload_home)
        c.ok(init.returncode == 0, f"04.18 setup: initial sync should exit 0, got {init.returncode}\n{combined_output(init)}")
        snapshot = upload_root / "upload-dest" / ".kitchensync" / "snapshot.db"
        before = snapshot.read_bytes()
        write_file(upload_src / "second.txt", "second\n")
        upload_faults.fail_snapshot_publish = True

        result = run_cli("-vl", "error", f"+{upload_src}", fixture.peer("/upload-dest"), home=upload_home)
        output = combined_output(result)
        tmp = upload_root / "upload-dest" / ".kitchensync" / "TMP"
        staged = [p for p in tmp.rglob("*") if p.is_file()] if tmp.is_dir() else []
        c.ok(result.returncode == 0, f"04.18: snapshot-upload failure should complete normally, got {result.returncode}\n{output}")
        c.ok(snapshot.read_bytes() == before, "04.18: failed snapshot upload should leave the existing snapshot.db untouched")
        c.ok(bool(staged), "04.18: failed snapshot upload should retain the staging file under .kitchensync/TMP")
        c.ok(output.strip() != "", "04.18: snapshot-upload failure should be logged at error verbosity")


def main() -> int:
    checks = Checks()
    tmp_parent = PROJECT_DIR / "tests" / ".tmp"
    tmp_parent.mkdir(parents=True, exist_ok=True)

    work = Path(tempfile.mkdtemp(prefix="04_error_handling_", dir=str(tmp_parent)))
    try:
        check_reachability_failures(work, checks)
        check_snapshot_download_failure(work, checks)
        check_transfer_and_displacement_failures(work, checks)
        check_listing_failures(work, checks)
        check_snapshot_upload_and_mtime_failures(work, checks)
    finally:
        remove_tree(work)

    if checks.failures:
        print("\nFAILURES:")
        for failure in checks.failures:
            print(f"- {failure}")
        return 1

    print("All observable 04_error-handling checks passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
