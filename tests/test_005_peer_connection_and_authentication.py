# /// script
# requires-python = ">=3.11"
# dependencies = ["cryptography"]
# ///
from __future__ import annotations

import os
import platform
import socket
import subprocess
import sys
import tempfile
import threading
import time
from dataclasses import dataclass
from pathlib import Path
from urllib.parse import quote

from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric import ed25519

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

WORKSPACE = Path("/home/ace/Desktop/prjx/kitchensync")
EXE = WORKSPACE / "released" / "kitchensync.exe"
SFTP_SERVER = WORKSPACE / "extart" / "ephemeral-sftp-server.py"


@dataclass
class RunResult:
    name: str
    code: int
    stdout: str
    stderr: str
    elapsed: float


class FailureCollector:
    def __init__(self) -> None:
        self.failures: list[str] = []

    def check(self, condition: bool, message: str) -> None:
        if not condition:
            self.failures.append(message)

    def result_ok(self, result: RunResult, context: str) -> None:
        self.check(
            result.code == 0,
            f"{context}: expected exit 0, got {result.code}; stdout={result.stdout!r}; stderr={result.stderr!r}",
        )
        self.check(result.stderr == "", f"{context}: stderr must be empty, got {result.stderr!r}")

    def result_failed(self, result: RunResult, context: str) -> None:
        self.check(
            result.code != 0,
            f"{context}: expected nonzero exit; stdout={result.stdout!r}; stderr={result.stderr!r}",
        )
        self.check(result.stderr == "", f"{context}: stderr must be empty, got {result.stderr!r}")


def uv_path() -> Path:
    system = platform.system().lower()
    if system == "windows":
        return WORKSPACE / "aitc" / "bin" / "uv.exe"
    if system == "darwin":
        return WORKSPACE / "aitc" / "bin" / "uv.mac"
    return WORKSPACE / "aitc" / "bin" / "uv.linux"


def file_url(path: Path) -> str:
    return path.resolve().as_uri()


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")


def create_home(root: Path, include_ed25519: bool = False) -> Path:
    home = root / "home"
    ssh = home / ".ssh"
    ssh.mkdir(parents=True, exist_ok=True)
    if include_ed25519:
        private_key = ed25519.Ed25519PrivateKey.generate()
        private_bytes = private_key.private_bytes(
            encoding=serialization.Encoding.PEM,
            format=serialization.PrivateFormat.OpenSSH,
            encryption_algorithm=serialization.NoEncryption(),
        )
        public_bytes = private_key.public_key().public_bytes(
            encoding=serialization.Encoding.OpenSSH,
            format=serialization.PublicFormat.OpenSSH,
        )
        private_path = ssh / "id_ed25519"
        private_path.write_bytes(private_bytes)
        (ssh / "id_ed25519.pub").write_bytes(public_bytes + b"\n")
        try:
            private_path.chmod(0o600)
        except OSError:
            pass
    return home


def base_env(home: Path) -> dict[str, str]:
    env = os.environ.copy()
    env["HOME"] = str(home)
    env["USERPROFILE"] = str(home)
    env.pop("SSH_AUTH_SOCK", None)
    env.pop("SSH_AGENT_PID", None)
    return env


def run_kitchensync(name: str, args: list[str], home: Path, timeout: float = 35.0) -> RunResult:
    start = time.monotonic()
    completed = subprocess.run(
        [str(EXE), *args],
        cwd=str(WORKSPACE),
        env=base_env(home),
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=timeout,
        shell=False,
    )
    return RunResult(
        name=name,
        code=completed.returncode,
        stdout=completed.stdout,
        stderr=completed.stderr,
        elapsed=time.monotonic() - start,
    )


class SftpServer:
    def __init__(
        self,
        temp_root: Path,
        user: str = "alice",
        password: str | None = "pw",
        authorized_key: Path | None = None,
    ) -> None:
        self.temp_root = temp_root
        self.user = user
        self.password = password
        self.authorized_key = authorized_key
        self.process: subprocess.Popen[str] | None = None
        self.stderr_lines: list[str] = []
        self._stderr_thread: threading.Thread | None = None
        self.port: int | None = None
        self.root: Path | None = None
        self.host_key: str | None = None

    def __enter__(self) -> SftpServer:
        cmd = [str(uv_path()), "run", "--script", str(SFTP_SERVER), "--user", self.user]
        if self.password is not None:
            cmd.extend(["--password", self.password])
        if self.authorized_key is not None:
            cmd.extend(["--authorized-key", str(self.authorized_key)])
        self.process = subprocess.Popen(
            cmd,
            cwd=str(WORKSPACE),
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            errors="replace",
            shell=False,
        )
        assert self.process.stdout is not None
        assert self.process.stderr is not None
        self._stderr_thread = threading.Thread(target=self._read_stderr, daemon=True)
        self._stderr_thread.start()
        line = self.process.stdout.readline().strip()
        if not line.isdigit():
            raise RuntimeError(f"SFTP server did not print a port: {line!r}")
        self.port = int(line)
        deadline = time.monotonic() + 10.0
        while time.monotonic() < deadline:
            self._parse_stderr()
            if self.root is not None and self.host_key is not None:
                return self
            time.sleep(0.05)
        raise RuntimeError(f"SFTP server did not publish root and host key: {self.stderr_text()!r}")

    def __exit__(self, _exc_type: object, _exc: object, _tb: object) -> None:
        if self.process is None:
            return
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=5.0)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=5.0)

    def _read_stderr(self) -> None:
        assert self.process is not None
        assert self.process.stderr is not None
        for line in self.process.stderr:
            self.stderr_lines.append(line.rstrip("\n"))

    def _parse_stderr(self) -> None:
        for line in list(self.stderr_lines):
            if line.startswith("sftp root: "):
                self.root = Path(line.removeprefix("sftp root: "))
            if line.startswith("host key: "):
                self.host_key = line.removeprefix("host key: ")

    def stderr_text(self) -> str:
        return "\n".join(self.stderr_lines)

    def url(self, remote_path: str = "/root", password: str | None = None, query: str = "") -> str:
        assert self.port is not None
        auth = self.user
        if password is not None:
            auth = f"{self.user}:{quote(password, safe='')}"
        return f"sftp://{auth}@127.0.0.1:{self.port}{remote_path}{query}"


def trust_host(home: Path, server: SftpServer) -> None:
    assert server.port is not None
    assert server.host_key is not None
    known_hosts = home / ".ssh" / "known_hosts"
    write_text(known_hosts, f"[127.0.0.1]:{server.port} {server.host_key}\n")


class BlackholeSsh:
    def __init__(self) -> None:
        self._stop = threading.Event()
        self._thread: threading.Thread | None = None
        self._listener: socket.socket | None = None
        self.port: int | None = None

    def __enter__(self) -> BlackholeSsh:
        self._listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self._listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self._listener.bind(("127.0.0.1", 0))
        self._listener.listen(20)
        self._listener.settimeout(0.2)
        self.port = self._listener.getsockname()[1]
        self._thread = threading.Thread(target=self._serve, daemon=True)
        self._thread.start()
        return self

    def __exit__(self, _exc_type: object, _exc: object, _tb: object) -> None:
        self._stop.set()
        if self._listener is not None:
            self._listener.close()
        if self._thread is not None:
            self._thread.join(timeout=1.0)

    def _serve(self) -> None:
        assert self._listener is not None
        sockets: list[socket.socket] = []
        try:
            while not self._stop.is_set():
                try:
                    conn, _addr = self._listener.accept()
                    conn.settimeout(0.2)
                    sockets.append(conn)
                except socket.timeout:
                    continue
                except OSError:
                    break
        finally:
            for conn in sockets:
                try:
                    conn.close()
                except OSError:
                    pass

    def url(self, path: str = "/root", query: str = "") -> str:
        assert self.port is not None
        return f"sftp://alice@127.0.0.1:{self.port}{path}{query}"


def test_file_fallback_primary_wins(root: Path, failures: FailureCollector) -> None:
    home = create_home(root / "home-file-fallback")
    primary = root / "file-fallback" / "primary"
    fallback = root / "file-fallback" / "fallback"
    dest = root / "file-fallback" / "dest"
    write_text(primary / "from-primary.txt", "primary\n")
    write_text(fallback / "from-fallback.txt", "fallback\n")
    result = run_kitchensync(
        "file fallback primary wins",
        [f"+[{file_url(primary)},{file_url(fallback)}]", file_url(dest)],
        home,
    )
    failures.result_ok(result, "005.2 005.4 005.5 005.6 file fallback primary winner")
    failures.check(
        (dest / "from-primary.txt").is_file(),
        "005.6: later operations must use the winning primary file URL",
    )
    failures.check(
        not (dest / "from-fallback.txt").exists(),
        "005.5: remaining fallback file URLs must not be used after the primary connects",
    )


def test_sftp_fallback_order_and_timeout(root: Path, failures: FailureCollector) -> None:
    home = create_home(root / "home-sftp-fallback")
    local_dest = root / "sftp-fallback" / "dest"
    with SftpServer(root / "sftp-a", password="pw") as server_a, SftpServer(root / "sftp-b", password="pw") as server_b:
        trust_host(home, server_a)
        trust_host(home, server_b)
        assert server_a.root is not None
        assert server_b.root is not None
        write_text(server_a.root / "winner" / "from-a.txt", "a\n")
        write_text(server_b.root / "winner" / "from-b.txt", "b\n")
        with BlackholeSsh() as blackhole:
            result = run_kitchensync(
                "sftp fallback order timeout",
                [
                    "--timeout-conn",
                    "1",
                    f"+[{blackhole.url('/winner')},{server_a.url('/winner', password='pw')},{server_b.url('/winner', password='pw')}]",
                    file_url(local_dest),
                ],
                home,
                timeout=20.0,
            )
    failures.result_ok(result, "005.3 005.4 005.7 SFTP fallback order after global timeout")
    failures.check(
        result.elapsed < 8.0,
        f"005.7: --timeout-conn should bound the failed SSH handshake before fallback; elapsed={result.elapsed:.2f}s",
    )
    failures.check(
        (local_dest / "from-a.txt").is_file(),
        "005.3: first successful fallback URL in command-line order must win",
    )
    failures.check(
        not (local_dest / "from-b.txt").exists(),
        "005.5: later fallback URL must not be used after an earlier fallback connects",
    )


def test_url_timeout_override(root: Path, failures: FailureCollector) -> None:
    home = create_home(root / "home-url-timeout")
    local_dest = root / "url-timeout" / "dest"
    with SftpServer(root / "sftp-timeout", password="pw") as server:
        trust_host(home, server)
        assert server.root is not None
        write_text(server.root / "ok" / "remote.txt", "remote\n")
        with BlackholeSsh() as blackhole:
            result = run_kitchensync(
                "url timeout override",
                [
                    "--timeout-conn",
                    "10",
                    f"+[{blackhole.url('/nope', '?timeout-conn=1')},{server.url('/ok', password='pw')}]",
                    file_url(local_dest),
                ],
                home,
                timeout=20.0,
            )
    failures.result_ok(result, "005.8 URL timeout override")
    failures.check(
        result.elapsed < 8.0,
        f"005.8: URL timeout-conn should override longer --timeout-conn; elapsed={result.elapsed:.2f}s",
    )
    failures.check((local_dest / "remote.txt").is_file(), "005.8: fallback after URL timeout should sync")


def test_file_connection_ignores_sftp_tuning_and_creates_root(root: Path, failures: FailureCollector) -> None:
    home = create_home(root / "home-file-create")
    source = root / "file-create" / "source"
    dest = root / "file-create" / "missing" / "parents" / "dest"
    write_text(source / "note.txt", "note\n")
    result = run_kitchensync(
        "file ignores sftp tuning and creates root",
        ["--timeout-conn", "1", "--timeout-idle", "1", f"+{file_url(source)}", file_url(dest)],
        home,
    )
    failures.result_ok(result, "005.9 005.10 local file connection")
    failures.check(dest.is_dir(), "005.10: normal run must create missing local peer root and parents")
    failures.check((dest / "note.txt").is_file(), "005.9: SFTP timeout settings must not block file:// sync")


def test_sftp_root_creation_and_auth_fallback(root: Path, failures: FailureCollector) -> None:
    home = create_home(root / "home-ed25519", include_ed25519=True)
    source = root / "sftp-create-auth" / "source"
    write_text(source / "seed.txt", "seed\n")
    authorized_key = home / ".ssh" / "id_ed25519.pub"
    with SftpServer(root / "sftp-create-auth", password="correct", authorized_key=authorized_key) as server:
        trust_host(home, server)
        result = run_kitchensync(
            "sftp root creation and auth fallback",
            [
                f"+{file_url(source)}",
                server.url("/new/remote/root", password="wrong"),
            ],
            home,
        )
        assert server.root is not None
        remote_file = server.root / "new" / "remote" / "root" / "seed.txt"
        remote_root = server.root / "new" / "remote" / "root"
    failures.result_ok(result, "005.11 005.18 005.19 SFTP create root and saved Ed25519 fallback")
    failures.check(remote_root.is_dir(), "005.11: normal run must create missing SFTP root and parents")
    failures.check(
        remote_file.is_file(),
        "005.18 005.19: rejected inline password and absent agent must continue to ~/.ssh/id_ed25519",
    )


def test_sftp_root_creation_failure(root: Path, failures: FailureCollector) -> None:
    home = create_home(root / "home-create-fail")
    local = root / "create-fail" / "local"
    write_text(local / "only.txt", "only\n")
    with SftpServer(root / "sftp-create-fail", password="pw") as server:
        trust_host(home, server)
        assert server.root is not None
        write_text(server.root / "blocked", "not a directory\n")
        result = run_kitchensync(
            "sftp root creation failure",
            [f"+{file_url(local)}", server.url("/blocked/child", password="pw")],
            home,
        )
    failures.result_failed(result, "005.12 005.13 005.14 005.15 failed SFTP root creation")
    failures.check(
        "error" in result.stdout.lower() or "unreachable" in result.stdout.lower(),
        "005.14: unreachable peer should produce an error-level diagnostic on stdout",
    )


def test_unreachable_and_canon_startup_errors(root: Path, failures: FailureCollector) -> None:
    home = create_home(root / "home-unreachable")
    existing = root / "unreachable" / "existing"
    missing = root / "unreachable" / "missing"
    canon_missing = root / "unreachable" / "canon-missing"
    write_text(existing / "file.txt", "file\n")
    result_less_than_two = run_kitchensync(
        "less than two reachable",
        ["--dry-run", file_url(existing), file_url(missing)],
        home,
    )
    result_canon = run_kitchensync(
        "canon unreachable",
        ["--dry-run", f"+{file_url(canon_missing)}", file_url(existing)],
        home,
    )
    failures.result_failed(result_less_than_two, "005.13 005.14 005.15 unreachable peer")
    failures.check(
        "error" in result_less_than_two.stdout.lower() or "unreachable" in result_less_than_two.stdout.lower(),
        "005.14: unreachable peer must be diagnosed on stdout",
    )
    failures.result_failed(result_canon, "005.16 canon unreachable")
    failures.check(
        "canon" in result_canon.stdout.lower() or "unreachable" in result_canon.stdout.lower(),
        "005.16: canon-unreachable startup failure should be observable in stdout",
    )
    failures.check(not missing.exists(), "005.13: dry-run missing file peer must remain uncreated when unreachable")
    failures.check(not canon_missing.exists(), "005.16: dry-run missing canon peer must remain uncreated")


def test_unknown_sftp_host_key_rejected(root: Path, failures: FailureCollector) -> None:
    home = create_home(root / "home-unknown-host")
    local = root / "unknown-host" / "local"
    write_text(local / "file.txt", "file\n")
    with SftpServer(root / "sftp-unknown-host", password="pw") as server:
        result = run_kitchensync(
            "unknown sftp host key rejected",
            [f"+{file_url(local)}", server.url("/target", password="pw")],
            home,
        )
    failures.result_failed(result, "005.17 unknown SFTP host key")
    failures.check(
        not (local / ".kitchensync").exists() or result.code != 0,
        "005.17: untrusted SFTP host must not be accepted as a successful peer",
    )


def main() -> int:
    failures = FailureCollector()
    failures.check(EXE.is_file(), f"released executable is missing: {EXE}")
    if not failures.failures:
        with tempfile.TemporaryDirectory(prefix="kitchensync-005-") as tmp:
            root = Path(tmp)
            tests = [
                test_file_fallback_primary_wins,
                test_sftp_fallback_order_and_timeout,
                test_url_timeout_override,
                test_file_connection_ignores_sftp_tuning_and_creates_root,
                test_sftp_root_creation_and_auth_fallback,
                test_sftp_root_creation_failure,
                test_unreachable_and_canon_startup_errors,
                test_unknown_sftp_host_key_rejected,
            ]
            for test in tests:
                try:
                    test(root, failures)
                except subprocess.TimeoutExpired as exc:
                    failures.failures.append(f"{test.__name__}: timed out: {exc}")
                except Exception as exc:  # noqa: BLE001
                    failures.failures.append(f"{test.__name__}: unexpected exception: {exc!r}")

    # not reasonably testable: 005.1

    if failures.failures:
        print("FAIL")
        for failure in failures.failures:
            print(f"- {failure}")
        return 1
    print("PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
