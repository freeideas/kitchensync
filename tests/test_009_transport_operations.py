# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
from __future__ import annotations

import os
import platform
import queue
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

LITERAL_WORKSPACE = Path("/home/ace/Desktop/prjx/kitchensync")
WORKSPACE = LITERAL_WORKSPACE if LITERAL_WORKSPACE.exists() else Path(__file__).resolve().parents[1]
LITERAL_EXE = Path("/home/ace/Desktop/prjx/kitchensync/released/kitchensync.exe")
KITCHENSYNC_EXE = LITERAL_EXE if LITERAL_EXE.exists() else WORKSPACE / "released" / "kitchensync.exe"
SFTP_SERVER = WORKSPACE / "extart" / "ephemeral-sftp-server.py"


# not reasonably testable: 009.6, 009.10. The testing guidelines forbid creating
# symbolic links for KitchenSync tests, and the released CLI exposes no direct
# stat/list_dir primitive for an existing natural symlink.
# not reasonably testable: 009.7, 009.11. Portable creation of devices, FIFOs,
# and sockets is not available on every supported platform, and the released CLI
# exposes no direct primitive-operation API for naturally occurring special files.
# not reasonably testable: 009.22. The requirement is a negative transport
# assumption: KitchenSync must not require rename-over-existing. The observable
# replacement checks below verify successful replacement through the released
# sync surface, but they cannot prove the internal rename call was never aimed at
# an existing destination.
# not reasonably testable: 009.28, 009.29, 009.30. The released CLI reports
# sync-level outcomes. Triggering every primitive error category, including
# mid-operation SFTP network failure, would require sabotaging runtime state or
# using a controllable fault-injection transport that is not a released surface.


def uv_executable() -> Path:
    system = platform.system().lower()
    if system == "windows":
        return WORKSPACE / "aitc" / "bin" / "uv.exe"
    if system == "darwin":
        return WORKSPACE / "aitc" / "bin" / "uv.mac"
    return WORKSPACE / "aitc" / "bin" / "uv.linux"


def decode_output(data: bytes | str | None) -> str:
    if data is None:
        return ""
    if isinstance(data, str):
        return data
    return data.decode("utf-8", errors="replace")


def run_kitchensync(args: list[str], env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(KITCHENSYNC_EXE), *args],
        cwd=str(WORKSPACE),
        env=env,
        text=True,
        encoding="utf-8",
        errors="replace",
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=45,
        shell=False,
    )


def check(failures: list[str], condition: bool, message: str) -> None:
    if not condition:
        failures.append(message)


def read_bytes(path: Path) -> bytes:
    return path.read_bytes()


def assert_file(
    failures: list[str],
    path: Path,
    expected: bytes,
    req_ids: str,
) -> None:
    if not path.is_file():
        failures.append(f"{req_ids}: expected file to exist: {path}")
        return
    actual = read_bytes(path)
    if actual != expected:
        failures.append(
            f"{req_ids}: wrong content for {path}: expected {expected!r}, got {actual!r}"
        )


def assert_absent(failures: list[str], path: Path, req_ids: str) -> None:
    if path.exists():
        failures.append(f"{req_ids}: expected path to be absent after sync: {path}")


def assert_mtime_close(
    failures: list[str],
    path: Path,
    expected: float,
    tolerance: float,
    req_ids: str,
) -> None:
    if not path.exists():
        failures.append(f"{req_ids}: cannot check modification time for missing path: {path}")
        return
    actual = path.stat().st_mtime
    if abs(actual - expected) > tolerance:
        failures.append(
            f"{req_ids}: modification time for {path} was {actual}, expected near {expected}"
        )


def prepare_source_tree(src: Path) -> dict[str, float]:
    (src / "nested").mkdir(parents=True)
    (src / "empty-dir").mkdir()
    (src / "alpha.txt").write_bytes(b"alpha from canon\n")
    (src / "nested" / "beta.bin").write_bytes(b"\x00beta bytes\xff")
    file_time = 1_700_000_111.0
    nested_time = 1_700_000_222.0
    empty_time = 1_700_000_333.0
    os.utime(src / "alpha.txt", (file_time, file_time))
    os.utime(src / "nested" / "beta.bin", (nested_time, nested_time))
    os.utime(src / "empty-dir", (empty_time, empty_time))
    return {
        "alpha": file_time,
        "beta": nested_time,
        "empty_dir": empty_time,
    }


def test_local_transport(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks-009-local-") as tmp_name:
        tmp = Path(tmp_name)
        src = tmp / "canon"
        dst = tmp / "replica"
        src.mkdir()
        times = prepare_source_tree(src)

        first = run_kitchensync(["--verbosity", "info", f"+{src}", str(dst)])
        check(failures, first.returncode == 0, f"009.1: local first sync exited {first.returncode}; stdout={first.stdout!r}")
        check(failures, first.stderr == "", f"009.1: stderr must be empty, got {first.stderr!r}")
        assert_file(failures, dst / "alpha.txt", b"alpha from canon\n", "009.1, 009.12-009.20")
        assert_file(failures, dst / "nested" / "beta.bin", b"\x00beta bytes\xff", "009.3-009.5, 009.12-009.20")
        check(failures, (dst / "empty-dir").is_dir(), "009.5, 009.24: expected empty directory to be created")
        assert_mtime_close(failures, dst / "alpha.txt", times["alpha"], 5.0, "009.8, 009.26")
        assert_mtime_close(failures, dst / "nested" / "beta.bin", times["beta"], 5.0, "009.8, 009.26")

        (src / "alpha.txt").write_bytes(b"replacement content\n")
        replacement_time = 1_700_001_000.0
        os.utime(src / "alpha.txt", (replacement_time, replacement_time))
        (src / "nested" / "beta.bin").unlink()
        (src / "empty-dir").rmdir()

        second = run_kitchensync(["--verbosity", "info", f"+{src}", str(dst)])
        check(failures, second.returncode == 0, f"009.1: local second sync exited {second.returncode}; stdout={second.stdout!r}")
        check(failures, second.stderr == "", f"009.1: stderr must remain empty, got {second.stderr!r}")
        assert_file(failures, dst / "alpha.txt", b"replacement content\n", "009.16-009.21, 009.26")
        assert_mtime_close(failures, dst / "alpha.txt", replacement_time, 5.0, "009.26")
        assert_absent(failures, dst / "nested" / "beta.bin", "009.21, 009.23")
        assert_absent(failures, dst / "empty-dir", "009.21, 009.25")
        check(failures, "C alpha.txt" in second.stdout, "009.12-009.21: expected copy progress for replaced local file")
        check(failures, "X nested/beta.bin" in second.stdout, "009.23: expected delete/displace progress for removed local file")


class SftpFixture:
    def __init__(self, failures: list[str], home: Path) -> None:
        self.failures = failures
        self.home = home
        self.proc: subprocess.Popen[str] | None = None
        self.stdout_lines: queue.Queue[str] = queue.Queue()
        self.stderr_lines: queue.Queue[str] = queue.Queue()
        self.port: int | None = None
        self.root: Path | None = None
        self.host_key_line: str | None = None

    def __enter__(self) -> "SftpFixture":
        self.proc = subprocess.Popen(
            [
                str(uv_executable()),
                "run",
                "--script",
                str(SFTP_SERVER),
                "--user",
                "ksuser",
                "--password",
                "kspass",
            ],
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
        threading.Thread(target=self._reader, args=(self.proc.stdout, self.stdout_lines), daemon=True).start()
        threading.Thread(target=self._reader, args=(self.proc.stderr, self.stderr_lines), daemon=True).start()

        port_line = self._get_line(self.stdout_lines, 20.0, "SFTP server did not print a port")
        if port_line is None:
            return self
        try:
            self.port = int(port_line.strip())
        except ValueError:
            self.failures.append(f"009.2: SFTP server printed a non-port stdout line: {port_line!r}")
            return self

        deadline = time.monotonic() + 20.0
        while time.monotonic() < deadline and (self.root is None or self.host_key_line is None):
            try:
                line = self.stderr_lines.get(timeout=0.2)
            except queue.Empty:
                continue
            if line.startswith("sftp root: "):
                self.root = Path(line.removeprefix("sftp root: ").strip())
            if line.startswith("host key: "):
                self.host_key_line = line.removeprefix("host key: ").strip()

        if self.root is None:
            self.failures.append("009.2: SFTP server did not report its temporary root")
        if self.host_key_line is None:
            self.failures.append("009.2: SFTP server did not report its host key")
        if self.port is not None and self.host_key_line is not None:
            ssh_dir = self.home / ".ssh"
            ssh_dir.mkdir(parents=True, exist_ok=True)
            known_hosts = ssh_dir / "known_hosts"
            known_hosts.write_text(
                f"[127.0.0.1]:{self.port} {self.host_key_line}\n",
                encoding="ascii",
                newline="\n",
            )
        return self

    def __exit__(self, _exc_type: object, _exc: object, _tb: object) -> None:
        if self.proc is None:
            return
        self.proc.terminate()
        try:
            self.proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            self.proc.kill()
            self.proc.wait(timeout=10)

    def _reader(self, stream: object, lines: queue.Queue[str]) -> None:
        for line in stream:
            lines.put(str(line))

    def _get_line(self, lines: queue.Queue[str], timeout: float, message: str) -> str | None:
        try:
            return lines.get(timeout=timeout)
        except queue.Empty:
            self.failures.append(f"009.2: {message}")
            return None

    def url(self, path: str) -> str:
        assert self.port is not None
        return f"sftp://ksuser:kspass@127.0.0.1:{self.port}/{path}"


def test_sftp_transport(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks-009-sftp-") as tmp_name:
        tmp = Path(tmp_name)
        home = tmp / "home"
        home.mkdir()
        env = os.environ.copy()
        env["HOME"] = str(home)
        env["USERPROFILE"] = str(home)
        env["SSH_AUTH_SOCK"] = ""

        with SftpFixture(failures, home) as sftp:
            if sftp.port is None or sftp.root is None:
                return

            src = tmp / "canon"
            src.mkdir()
            times = prepare_source_tree(src)
            remote_url = sftp.url("remote-root")
            remote_root = sftp.root / "remote-root"

            first = run_kitchensync(["--verbosity", "info", f"+{src}", remote_url], env=env)
            check(failures, first.returncode == 0, f"009.2: SFTP first sync exited {first.returncode}; stdout={first.stdout!r}")
            check(failures, first.stderr == "", f"009.2: SFTP sync stderr must be empty, got {first.stderr!r}")
            assert_file(failures, remote_root / "alpha.txt", b"alpha from canon\n", "009.2, 009.12-009.20")
            assert_file(failures, remote_root / "nested" / "beta.bin", b"\x00beta bytes\xff", "009.2-009.5, 009.12-009.20")
            check(failures, (remote_root / "empty-dir").is_dir(), "009.2, 009.24: SFTP directory creation failed")
            assert_mtime_close(failures, remote_root / "alpha.txt", times["alpha"], 5.0, "009.8, 009.26")

            (src / "alpha.txt").write_bytes(b"sftp replacement\n")
            replacement_time = 1_700_002_000.0
            os.utime(src / "alpha.txt", (replacement_time, replacement_time))
            (src / "nested" / "beta.bin").unlink()
            (src / "empty-dir").rmdir()

            second = run_kitchensync(["--verbosity", "info", f"+{src}", remote_url], env=env)
            check(failures, second.returncode == 0, f"009.2: SFTP second sync exited {second.returncode}; stdout={second.stdout!r}")
            check(failures, second.stderr == "", f"009.2: SFTP sync stderr must remain empty, got {second.stderr!r}")
            assert_file(failures, remote_root / "alpha.txt", b"sftp replacement\n", "009.2, 009.16-009.21, 009.26")
            assert_mtime_close(failures, remote_root / "alpha.txt", replacement_time, 5.0, "009.2, 009.26")
            assert_absent(failures, remote_root / "nested" / "beta.bin", "009.2, 009.21, 009.23")
            assert_absent(failures, remote_root / "empty-dir", "009.2, 009.21, 009.25")


def main() -> int:
    failures: list[str] = []
    check(failures, KITCHENSYNC_EXE.is_file(), f"released executable is missing: {KITCHENSYNC_EXE}")
    check(failures, SFTP_SERVER.is_file(), f"SFTP fixture is missing: {SFTP_SERVER}")
    if not failures:
        try:
            test_local_transport(failures)
        except Exception as exc:  # noqa: BLE001
            failures.append(f"local transport check crashed: {exc!r}")
        try:
            test_sftp_transport(failures)
        except Exception as exc:  # noqa: BLE001
            failures.append(f"SFTP transport check crashed: {exc!r}")

    if failures:
        print("FAILURES:")
        for failure in failures:
            print(f"- {failure}")
        return 1
    print("test_009_transport_operations passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
