# /// script
# requires-python = ">=3.11"
# dependencies = ["cryptography"]
# ///
from __future__ import annotations

import os
import platform
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
from dataclasses import dataclass
from pathlib import Path

from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric import ed25519, rsa

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

WORKSPACE = Path("/home/ace/Desktop/prjx/kitchensync")
KITCHENSYNC = WORKSPACE / "released" / "kitchensync.exe"
SFTP_SERVER = WORKSPACE / "extart" / "ephemeral-sftp-server.py"


@dataclass
class Failure:
    name: str
    message: str


@dataclass
class RunResult:
    args: list[str]
    returncode: int
    stdout: str
    stderr: str
    elapsed: float


class Checks:
    def __init__(self) -> None:
        self.failures: list[Failure] = []

    def check(self, condition: bool, name: str, message: str) -> None:
        if not condition:
            self.failures.append(Failure(name, message))


def host_uv() -> Path:
    system = platform.system().lower()
    if system == "windows":
        return WORKSPACE / "aitc" / "bin" / "uv.exe"
    if system == "darwin":
        return WORKSPACE / "aitc" / "bin" / "uv.mac"
    return WORKSPACE / "aitc" / "bin" / "uv.linux"


def run_kitchensync(args: list[str], home: Path, timeout: float = 20.0) -> RunResult:
    env = os.environ.copy()
    env["HOME"] = str(home)
    env["USERPROFILE"] = str(home)
    env.pop("SSH_AUTH_SOCK", None)
    start = time.monotonic()
    proc = subprocess.run(
        [str(KITCHENSYNC), *args],
        cwd=str(WORKSPACE),
        env=env,
        text=True,
        encoding="utf-8",
        errors="replace",
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=timeout,
        shell=False,
        check=False,
    )
    return RunResult(args, proc.returncode, proc.stdout, proc.stderr, time.monotonic() - start)


def file_url(path: Path) -> str:
    return path.resolve().as_uri()


def seed_file(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")


def assert_clean_process(checks: Checks, name: str, result: RunResult, ok: bool) -> None:
    if ok:
        checks.check(
            result.returncode == 0,
            name,
            f"expected success, got exit {result.returncode}; stdout={result.stdout!r}; stderr={result.stderr!r}",
        )
    else:
        checks.check(
            result.returncode != 0,
            name,
            f"expected failure, got exit 0; stdout={result.stdout!r}; stderr={result.stderr!r}",
        )
    checks.check(result.stderr == "", name, f"stderr must stay empty, got {result.stderr!r}")


class DeadSshEndpoint:
    def __init__(self) -> None:
        self._stop = threading.Event()
        self._ready = threading.Event()
        self._thread = threading.Thread(target=self._serve, daemon=True)
        self._sockets: list[socket.socket] = []
        self.port = 0

    def __enter__(self) -> "DeadSshEndpoint":
        self._thread.start()
        if not self._ready.wait(timeout=5.0):
            raise RuntimeError("dead SSH endpoint did not start")
        return self

    def __exit__(self, _exc_type: object, _exc: object, _tb: object) -> None:
        self._stop.set()
        for sock in self._sockets:
            try:
                sock.close()
            except OSError:
                pass
        self._thread.join(timeout=2.0)

    def _serve(self) -> None:
        listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        listener.bind(("127.0.0.1", 0))
        listener.listen()
        listener.settimeout(0.2)
        self.port = int(listener.getsockname()[1])
        self._sockets.append(listener)
        self._ready.set()
        while not self._stop.is_set():
            try:
                client, _addr = listener.accept()
            except socket.timeout:
                continue
            except OSError:
                break
            self._sockets.append(client)


class SftpServer:
    def __init__(self, args: list[str]) -> None:
        self.args = args
        self.proc: subprocess.Popen[str] | None = None
        self.port = 0
        self.root: Path | None = None
        self.host_key_line = ""

    def __enter__(self) -> "SftpServer":
        self.proc = subprocess.Popen(
            [str(host_uv()), "run", "--script", str(SFTP_SERVER), *self.args],
            cwd=str(WORKSPACE),
            text=True,
            encoding="utf-8",
            errors="replace",
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            shell=False,
        )
        assert self.proc.stdout is not None
        assert self.proc.stderr is not None
        line = self.proc.stdout.readline().strip()
        if not line.isdigit():
            raise RuntimeError(f"SFTP server did not print a port: {line!r}")
        self.port = int(line)
        deadline = time.monotonic() + 5.0
        while time.monotonic() < deadline and (self.root is None or not self.host_key_line):
            stderr_line = self.proc.stderr.readline()
            if stderr_line.startswith("sftp root: "):
                self.root = Path(stderr_line.removeprefix("sftp root: ").strip())
            if stderr_line.startswith("host key: "):
                self.host_key_line = stderr_line.removeprefix("host key: ").strip()
        if self.root is None or not self.host_key_line:
            raise RuntimeError("SFTP server did not publish root and host key")
        return self

    def __exit__(self, _exc_type: object, _exc: object, _tb: object) -> None:
        if self.proc is None:
            return
        self.proc.terminate()
        try:
            self.proc.wait(timeout=5.0)
        except subprocess.TimeoutExpired:
            self.proc.kill()
            self.proc.wait(timeout=5.0)


def write_known_hosts(home: Path, server: SftpServer) -> None:
    ssh_dir = home / ".ssh"
    ssh_dir.mkdir(parents=True, exist_ok=True)
    (ssh_dir / "known_hosts").write_text(
        f"[127.0.0.1]:{server.port} {server.host_key_line}\n",
        encoding="ascii",
        newline="\n",
    )


def write_ed25519_identity(home: Path) -> Path:
    ssh_dir = home / ".ssh"
    ssh_dir.mkdir(parents=True, exist_ok=True)
    key = ed25519.Ed25519PrivateKey.generate()
    private_bytes = key.private_bytes(
        encoding=serialization.Encoding.PEM,
        format=serialization.PrivateFormat.OpenSSH,
        encryption_algorithm=serialization.NoEncryption(),
    )
    public_bytes = key.public_key().public_bytes(
        encoding=serialization.Encoding.OpenSSH,
        format=serialization.PublicFormat.OpenSSH,
    )
    private_path = ssh_dir / "id_ed25519"
    public_path = ssh_dir / "id_ed25519.pub"
    private_path.write_bytes(private_bytes)
    public_path.write_bytes(public_bytes + b"\n")
    try:
        private_path.chmod(0o600)
    except OSError:
        pass

    rejected_rsa = rsa.generate_private_key(public_exponent=65537, key_size=2048)
    rsa_path = ssh_dir / "id_rsa"
    rsa_path.write_bytes(
        rejected_rsa.private_bytes(
            encoding=serialization.Encoding.PEM,
            format=serialization.PrivateFormat.TraditionalOpenSSL,
            encryption_algorithm=serialization.NoEncryption(),
        )
    )
    try:
        rsa_path.chmod(0o600)
    except OSError:
        pass
    return public_path


def fallback_local_behavior(checks: Checks, tmp: Path, home: Path) -> None:
    canon = tmp / "canon"
    seed_file(canon / "from-canon.txt", "canon data\n")
    primary = tmp / "primary-wins"
    fallback = tmp / "fallback-should-not-exist"
    result = run_kitchensync(
        ["--timeout-conn", "1", "--timeout-idle", "1", f"+{file_url(canon)}", f"[{file_url(primary)},{file_url(fallback)}]"],
        home,
    )
    assert_clean_process(checks, "005.2 005.4 005.5 005.6 005.10 005.11 primary file fallback", result, True)
    checks.check((primary / "from-canon.txt").read_text(encoding="utf-8") == "canon data\n", "005.6", "winning primary URL must receive later copy operations")
    checks.check(not fallback.exists(), "005.5", "remaining fallback URL must not be connected or created after primary succeeds")

    canon2 = tmp / "canon2"
    seed_file(canon2 / "ordered.txt", "ordered fallback\n")
    blocker = tmp / "not-a-directory"
    blocker.write_text("blocks parent creation\n", encoding="utf-8", newline="\n")
    bad_primary = blocker / "child"
    first_fallback = tmp / "first-fallback"
    second_fallback = tmp / "second-fallback"
    result = run_kitchensync(
        [f"+{file_url(canon2)}", f"[{file_url(bad_primary)},{file_url(first_fallback)},{file_url(second_fallback)}]"],
        home,
    )
    assert_clean_process(checks, "005.3 005.4 005.5 005.13 ordered file fallback", result, True)
    checks.check((first_fallback / "ordered.txt").exists(), "005.3", "first successful fallback in command-line order must win")
    checks.check(not second_fallback.exists(), "005.5", "later fallback must not be touched after earlier fallback succeeds")


def unreachable_startup_behavior(checks: Checks, tmp: Path, home: Path) -> None:
    existing = tmp / "existing"
    existing.mkdir(parents=True, exist_ok=True)
    missing = tmp / "missing-dry-run-peer"
    result = run_kitchensync(["--dry-run", f"+{file_url(existing)}", file_url(missing)], home)
    assert_clean_process(checks, "005.14 005.15 005.16 005.17 unreachable peer count", result, False)
    checks.check("unreachable" in result.stdout.lower() or "error" in result.stdout.lower(), "005.15", "unreachable peer should emit an error-level diagnostic on stdout")

    canon_missing = tmp / "missing-canon"
    reachable = tmp / "reachable"
    reachable.mkdir(parents=True, exist_ok=True)
    result = run_kitchensync(["--dry-run", f"+{file_url(canon_missing)}", file_url(reachable)], home)
    assert_clean_process(checks, "005.18 unreachable canon", result, False)
    checks.check("canon" in result.stdout.lower() or "unreachable" in result.stdout.lower(), "005.18", "unreachable canon failure should be observable on stdout")


def sftp_timeout_fallback(checks: Checks, tmp: Path, home: Path) -> None:
    canon = tmp / "timeout-canon"
    seed_file(canon / "timeout.txt", "timeout fallback\n")
    fallback = tmp / "timeout-fallback"
    with DeadSshEndpoint() as dead:
        result = run_kitchensync(
            ["--timeout-conn", "1", f"+{file_url(canon)}", f"[sftp://user@127.0.0.1:{dead.port}/missing,{file_url(fallback)}]"],
            home,
            timeout=10.0,
        )
    assert_clean_process(checks, "005.7 005.9 global SFTP timeout fallback", result, True)
    checks.check((fallback / "timeout.txt").exists(), "005.9", "SFTP handshake timeout must fail that URL and try the next fallback")

    canon2 = tmp / "url-timeout-canon"
    seed_file(canon2 / "url-timeout.txt", "url timeout fallback\n")
    fallback2 = tmp / "url-timeout-fallback"
    with DeadSshEndpoint() as dead:
        result = run_kitchensync(
            ["--timeout-conn", "30", f"+{file_url(canon2)}", f"[sftp://user@127.0.0.1:{dead.port}/missing?timeout-conn=1,{file_url(fallback2)}]"],
            home,
            timeout=10.0,
        )
    assert_clean_process(checks, "005.8 URL SFTP timeout fallback", result, True)
    checks.check(result.elapsed < 10.0 and (fallback2 / "url-timeout.txt").exists(), "005.8", "URL timeout-conn must bound the handshake instead of the larger global value")


def sftp_host_key_and_auth(checks: Checks, tmp: Path, home: Path) -> None:
    canon = tmp / "sftp-canon"
    seed_file(canon / "remote.txt", "remote root\n")
    with SftpServer(["--user", "alice", "--password", "secret"]) as server:
        result = run_kitchensync([f"+{file_url(canon)}", f"sftp://alice:secret@127.0.0.1:{server.port}/created/root"], home)
        assert_clean_process(checks, "005.19 unknown SFTP host key rejected", result, False)
        checks.check("host" in result.stdout.lower() or "unreachable" in result.stdout.lower(), "005.19", "unknown host key rejection should be diagnosed on stdout")

    trusted_home = tmp / "trusted-home"
    public_key = write_ed25519_identity(trusted_home)
    with SftpServer(["--user", "alice", "--password", "correct-password", "--authorized-key", str(public_key)]) as server:
        write_known_hosts(trusted_home, server)
        canon2 = tmp / "sftp-canon2"
        seed_file(canon2 / "key-auth.txt", "ed25519 auth\n")
        result = run_kitchensync(
            [f"+{file_url(canon2)}", f"sftp://alice:wrong-password@127.0.0.1:{server.port}/made/by/sftp"],
            trusted_home,
        )
        assert_clean_process(checks, "005.12 005.20 005.21 SFTP root creation and auth fallback", result, True)
        assert server.root is not None
        checks.check((server.root / "made" / "by" / "sftp").is_dir(), "005.12", "normal SFTP run must create missing remote root parents")
        checks.check((server.root / "made" / "by" / "sftp" / "key-auth.txt").read_text(encoding="utf-8") == "ed25519 auth\n", "005.20", "client must authenticate with id_ed25519 after rejected inline password and without SSH agent")


def main() -> int:
    checks = Checks()
    # not reasonably testable: 005.1 requires observing connection start
    # concurrency without a specified peer-side timing or tracing surface.
    with tempfile.TemporaryDirectory(prefix="ks-005-") as raw_tmp:
        tmp = Path(raw_tmp)
        home = tmp / "home"
        home.mkdir(parents=True, exist_ok=True)
        try:
            fallback_local_behavior(checks, tmp, home)
            unreachable_startup_behavior(checks, tmp, home)
            sftp_timeout_fallback(checks, tmp, home)
            sftp_host_key_and_auth(checks, tmp, home)
        except Exception as exc:  # noqa: BLE001
            checks.failures.append(Failure("test harness", f"unexpected harness exception: {exc!r}"))
        finally:
            shutil.rmtree(home, ignore_errors=True)

    if checks.failures:
        for failure in checks.failures:
            print(f"FAIL {failure.name}: {failure.message}")
        return 1
    print("all checks passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
