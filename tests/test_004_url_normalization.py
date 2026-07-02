# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
from __future__ import annotations

import getpass
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

WORKSPACE = Path("/home/ace/Desktop/prjx/kitchensync")
if not WORKSPACE.exists():
    WORKSPACE = Path(__file__).resolve().parents[1]

PRODUCT = Path("/home/ace/Desktop/prjx/kitchensync/released/kitchensync.exe")
if not PRODUCT.exists():
    PRODUCT = WORKSPACE / "released" / "kitchensync.exe"

SFTP_SERVER = WORKSPACE / "extart" / "ephemeral-sftp-server.py"


def bundled_uv() -> Path:
    if sys.platform.startswith("win"):
        return WORKSPACE / "aitc" / "bin" / "uv.exe"
    if sys.platform == "darwin":
        return WORKSPACE / "aitc" / "bin" / "uv.mac"
    return WORKSPACE / "aitc" / "bin" / "uv.linux"


def record(failures: list[str], condition: bool, message: str) -> None:
    if not condition:
        failures.append(message)


def run_product(
    failures: list[str],
    args: list[str],
    *,
    cwd: Path,
    env: dict[str, str] | None = None,
    label: str,
    timeout: float = 35.0,
) -> subprocess.CompletedProcess[str] | None:
    try:
        completed = subprocess.run(
            [str(PRODUCT), *args],
            cwd=str(cwd),
            env=env,
            text=True,
            encoding="utf-8",
            errors="replace",
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=timeout,
            shell=False,
        )
    except subprocess.TimeoutExpired:
        failures.append(f"{label}: released product timed out")
        return None

    record(failures, completed.returncode == 0, f"{label}: exit code {completed.returncode}, stdout={completed.stdout!r}, stderr={completed.stderr!r}")
    record(failures, completed.stderr == "", f"{label}: stderr must be empty, got {completed.stderr!r}")
    record(failures, "sync complete" in completed.stdout.splitlines(), f"{label}: stdout must include sync complete, got {completed.stdout!r}")
    return completed


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")


def assert_file_text(failures: list[str], path: Path, expected: str, label: str) -> None:
    if not path.exists():
        failures.append(f"{label}: expected file {path} to exist")
        return
    actual = path.read_text(encoding="utf-8")
    record(failures, actual == expected, f"{label}: expected {expected!r}, got {actual!r}")


def test_local_path_normalization(failures: list[str], root: Path) -> None:
    source = root / "local-source"
    bare_dest = root / "bare-dest"
    relative_dest = root / "relative-dest"
    source.mkdir()
    write_text(source / "item.txt", "local normalization\n")

    run_product(
        failures,
        ["--verbosity", "error", "+" + str(source), str(bare_dest)],
        cwd=root,
        label="004.1 bare path becomes file URL",
    )
    assert_file_text(
        failures,
        bare_dest / "item.txt",
        "local normalization\n",
        "004.1 bare path becomes file URL",
    )

    run_product(
        failures,
        ["--verbosity", "error", "+" + str(source), "relative-dest"],
        cwd=root,
        label="004.3 relative path resolves from cwd",
    )
    assert_file_text(
        failures,
        relative_dest / "item.txt",
        "local normalization\n",
        "004.3 relative path resolves from cwd",
    )

    if os.name == "nt":
        drive_dest = root / "drive-dest"
        run_product(
            failures,
            ["--verbosity", "error", "+" + str(source), str(drive_dest)],
            cwd=root,
            label="004.2 Windows drive path becomes file URL",
        )
        assert_file_text(
            failures,
            drive_dest / "item.txt",
            "local normalization\n",
            "004.2 Windows drive path becomes file URL",
        )
    # not reasonably testable: 004.2 on non-Windows hosts has no native Windows drive path.


class SftpServer:
    def __init__(self, failures: list[str], home: Path, user: str, password: str) -> None:
        self.failures = failures
        self.home = home
        self.user = user
        self.password = password
        self.process: subprocess.Popen[str] | None = None
        self.port: int | None = None

    def __enter__(self) -> "SftpServer":
        env = os.environ.copy()
        env["HOME"] = str(self.home)
        env["USERPROFILE"] = str(self.home)
        self.process = subprocess.Popen(
            [
                str(bundled_uv()),
                "run",
                "--script",
                str(SFTP_SERVER),
                "--user",
                self.user,
                "--password",
                self.password,
            ],
            cwd=str(WORKSPACE),
            env=env,
            text=True,
            encoding="utf-8",
            errors="replace",
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            shell=False,
        )
        assert self.process.stdout is not None
        assert self.process.stderr is not None
        line = self.process.stdout.readline().strip()
        try:
            self.port = int(line)
        except ValueError:
            self.failures.append(f"SFTP server did not print a port, got {line!r}")
            self.close()
            return self

        host_key_line = ""
        deadline = time.monotonic() + 10.0
        while time.monotonic() < deadline:
            stderr_line = self.process.stderr.readline()
            if stderr_line.startswith("host key: "):
                host_key_line = stderr_line.removeprefix("host key: ").strip()
                break
            if self.process.poll() is not None:
                break
        if not host_key_line:
            self.failures.append("SFTP server did not print a host key")
            self.close()
            return self

        ssh_dir = self.home / ".ssh"
        ssh_dir.mkdir(parents=True, exist_ok=True)
        known_hosts = ssh_dir / "known_hosts"
        known_hosts.write_text(
            f"[localhost]:{self.port} {host_key_line}\n"
            f"[LOCALHOST]:{self.port} {host_key_line}\n"
            f"[127.0.0.1]:{self.port} {host_key_line}\n",
            encoding="ascii",
            newline="\n",
        )
        return self

    def __exit__(self, _exc_type: object, _exc: object, _tb: object) -> None:
        self.close()

    def close(self) -> None:
        if self.process is None:
            return
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=5.0)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=5.0)

    def url(self, path: str, *, host: str = "localhost", scheme: str = "sftp") -> str:
        if self.port is None:
            return "sftp://invalid.invalid/missing"
        return f"{scheme}://{self.user}:{self.password}@{host}:{self.port}{path}"


def test_sftp_url_normalization(failures: list[str], root: Path) -> None:
    home = root / "home"
    user = "urlnormuser"
    password = "pw"
    env = os.environ.copy()
    env["HOME"] = str(home)
    env["USERPROFILE"] = str(home)
    env.pop("SSH_AUTH_SOCK", None)

    with SftpServer(failures, home, user, password) as server:
        if server.port is None:
            return

        source = root / "sftp-source"
        roundtrip = root / "sftp-roundtrip"
        source.mkdir()
        write_text(source / "marker.txt", "sftp normalized identity\n")

        variant = server.url(
            "/UrlNorm//Space%41/?timeout-conn=5&timeout-idle=5",
            host="LOCALHOST",
            scheme="SFTP",
        )
        normalized = server.url("/UrlNorm/SpaceA")

        run_product(
            failures,
            ["--verbosity", "error", "+" + str(source), variant],
            cwd=root,
            env=env,
            label="004.4/004.5/004.7/004.8/004.9/004.10/004.12/004.14 SFTP variant upload",
        )
        run_product(
            failures,
            ["--verbosity", "error", "+" + normalized, str(roundtrip)],
            cwd=root,
            env=env,
            label="004.4/004.5/004.7/004.8/004.9/004.10/004.12/004.14 SFTP normalized lookup",
        )
        assert_file_text(
            failures,
            roundtrip / "marker.txt",
            "sftp normalized identity\n",
            "SFTP normalized URL should address the same peer root",
        )

        reserved_source = root / "reserved-source"
        reserved_result = root / "reserved-result"
        reserved_source.mkdir()
        write_text(reserved_source / "reserved.txt", "reserved stays encoded\n")

        encoded_reserved = server.url("/Reserved%2FSlash")
        literal_reserved = server.url("/Reserved/Slash")
        run_product(
            failures,
            ["--verbosity", "error", "+" + str(reserved_source), encoded_reserved],
            cwd=root,
            env=env,
            label="004.11 encoded reserved upload",
        )
        run_product(
            failures,
            ["--verbosity", "error", "+" + literal_reserved, str(reserved_result)],
            cwd=root,
            env=env,
            label="004.11 literal reserved comparison root",
        )
        record(
            failures,
            not (reserved_result / "reserved.txt").exists(),
            "004.11 percent-encoded reserved slash must not identify the literal slash path",
        )

    # not reasonably testable: 004.6 needs observing identity removal for port 22,
    # but the offline test SFTP server is required to bind an OS-assigned non-22 port.
    # not reasonably testable: 004.13 requires no SFTP username while still
    # authenticating; authentication fallback behavior belongs to requirement 005.


def main() -> int:
    failures: list[str] = []
    if not PRODUCT.exists():
        failures.append(f"released product missing: {PRODUCT}")
    if not SFTP_SERVER.exists():
        failures.append(f"ephemeral SFTP server missing: {SFTP_SERVER}")

    with tempfile.TemporaryDirectory(prefix="kitchensync-url-normalization-") as temp:
        root = Path(temp)
        test_local_path_normalization(failures, root)
        test_sftp_url_normalization(failures, root)

    if failures:
        print("FAIL")
        for failure in failures:
            print(f"- {failure}")
        return 1

    print("PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
