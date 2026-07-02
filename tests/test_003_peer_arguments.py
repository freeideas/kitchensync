# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

from __future__ import annotations

import getpass
import os
import platform
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path
from urllib.parse import quote


sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

WORKSPACE = Path("/home/ace/Desktop/prjx/kitchensync")
EXE = Path("/home/ace/Desktop/prjx/kitchensync/released/kitchensync.exe")


class CheckRun:
    def __init__(self) -> None:
        self.failures: list[str] = []

    def check(self, condition: bool, message: str) -> None:
        if not condition:
            self.failures.append(message)

    def equal(self, actual: object, expected: object, message: str) -> None:
        if actual != expected:
            self.failures.append(f"{message}: expected {expected!r}, got {actual!r}")


def bundled_uv() -> Path:
    system = platform.system().lower()
    if system == "windows":
        return WORKSPACE / "aitc" / "bin" / "uv.exe"
    if system == "darwin":
        return WORKSPACE / "aitc" / "bin" / "uv.mac"
    return WORKSPACE / "aitc" / "bin" / "uv.linux"


def run_ks(args: list[str], cwd: Path | None = None, env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
    merged_env = os.environ.copy()
    if env:
        merged_env.update(env)
    return subprocess.run(
        [str(EXE), *args],
        cwd=str(cwd) if cwd else None,
        env=merged_env,
        text=True,
        encoding="utf-8",
        errors="replace",
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=20,
        shell=False,
        check=False,
    )


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")


def read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def reset_dir(path: Path) -> None:
    if path.exists():
        shutil.rmtree(path)
    path.mkdir(parents=True)


def assert_success(checks: CheckRun, proc: subprocess.CompletedProcess[str], label: str) -> None:
    checks.equal(proc.returncode, 0, f"{label} should exit 0")
    checks.equal(proc.stderr, "", f"{label} should leave stderr empty")
    checks.check("sync complete" in proc.stdout.splitlines(), f"{label} should print sync complete")


def assert_validation_error(checks: CheckRun, proc: subprocess.CompletedProcess[str], label: str) -> None:
    checks.check(proc.returncode != 0, f"{label} should fail validation")
    checks.equal(proc.stderr, "", f"{label} should leave stderr empty")
    checks.check("Usage: kitchensync" in proc.stdout, f"{label} should print help text on stdout")


def local_sync_case(checks: CheckRun, root: Path) -> None:
    src = root / "src"
    dst = root / "dst"
    reset_dir(src)
    reset_dir(dst)
    write_text(src / "alpha.txt", "alpha\n")

    proc = run_ks([f"+{src}", str(dst)])
    assert_success(checks, proc, "003.2/003.4 local path peer")
    checks.equal(read_text(dst / "alpha.txt"), "alpha\n", "003.2 local path peer should receive copied content")


def relative_path_case(checks: CheckRun, root: Path) -> None:
    cwd = root / "relative"
    reset_dir(cwd)
    write_text(cwd / "left" / "rel.txt", "relative\n")
    (cwd / "right").mkdir()

    proc = run_ks(["+left", "right"], cwd=cwd)
    assert_success(checks, proc, "003.7 relative path peer")
    checks.equal(read_text(cwd / "right" / "rel.txt"), "relative\n", "003.7 relative path peer should sync")


def absolute_path_cases(checks: CheckRun, root: Path) -> None:
    if os.name != "nt":
        src = root / "unix_abs_src"
        dst = root / "unix_abs_dst"
        reset_dir(src)
        reset_dir(dst)
        write_text(src / "abs.txt", "absolute\n")
        proc = run_ks([f"+{src}", str(dst)])
        assert_success(checks, proc, "003.5 Unix-style absolute path peer")
        checks.equal(read_text(dst / "abs.txt"), "absolute\n", "003.5 Unix-style absolute path should sync")
    else:
        src = root / "windows_abs_src"
        dst = root / "windows_abs_dst"
        reset_dir(src)
        reset_dir(dst)
        write_text(src / "drive.txt", "drive\n")
        proc = run_ks([f"+{src}", str(dst)])
        assert_success(checks, proc, "003.6 Windows drive path peer")
        checks.equal(read_text(dst / "drive.txt"), "drive\n", "003.6 Windows drive path should sync")


def role_cases(checks: CheckRun, root: Path) -> None:
    src = root / "roles_src"
    sub_a = root / "roles_sub_a"
    sub_b = root / "roles_sub_b"
    reset_dir(src)
    if sub_a.exists():
        shutil.rmtree(sub_a)
    if sub_b.exists():
        shutil.rmtree(sub_b)
    write_text(src / "role.txt", "canon\n")

    proc = run_ks([f"+{src}", f"-{sub_a}", f"-{sub_b}"])
    assert_success(checks, proc, "003.15/003.16/003.19 role markers")
    checks.equal(read_text(sub_a / "role.txt"), "canon\n", "003.16 subordinate peer should receive outcome")
    checks.equal(read_text(sub_b / "role.txt"), "canon\n", "003.19 multiple subordinate peers should be accepted")

    normal_left = root / "normal_left"
    normal_right = root / "normal_right"
    reset_dir(normal_left)
    reset_dir(normal_right)
    write_text(normal_left / "normal.txt", "normal\n")
    proc = run_ks([f"+{normal_left}", str(normal_right)])
    assert_success(checks, proc, "003.17 normal peer setup")
    write_text(normal_right / "after-first-sync.txt", "normal peer contributes after snapshot\n")
    proc = run_ks([str(normal_left), str(normal_right)])
    assert_success(checks, proc, "003.17 unmarked normal bidirectional peer")
    checks.equal(
        read_text(normal_left / "after-first-sync.txt"),
        "normal peer contributes after snapshot\n",
        "003.17 unmarked peer should contribute after it has snapshot history",
    )


def validation_cases(checks: CheckRun, root: Path) -> None:
    one = root / "one_peer"
    reset_dir(one)
    proc = run_ks([str(one)])
    assert_validation_error(checks, proc, "003.1 fewer than two peers")

    a = root / "plus_a"
    b = root / "plus_b"
    reset_dir(a)
    reset_dir(b)
    proc = run_ks([f"+{a}", f"+{b}"])
    assert_validation_error(checks, proc, "003.18 more than one canon peer")

    proc = run_ks([f"+{a.as_uri()}?max-copies=1", str(b)])
    assert_validation_error(checks, proc, "003.31 URL query max-copies")


def fallback_cases(checks: CheckRun, root: Path) -> None:
    src = root / "fallback_src"
    blocked = root / "fallback_blocked"
    dst = root / "fallback_dst"
    reset_dir(src)
    if dst.exists():
        shutil.rmtree(dst)
    if blocked.exists():
        if blocked.is_dir():
            shutil.rmtree(blocked)
        else:
            blocked.unlink()
    write_text(blocked, "not a directory\n")
    write_text(src / "fallback.txt", "fallback\n")

    proc = run_ks([f"+{src}", f"[{blocked},{dst}]"])
    assert_success(checks, proc, "003.20/003.21/003.23 fallback local peer")
    checks.equal(read_text(dst / "fallback.txt"), "fallback\n", "003.23 second fallback should be tried after first fails")

    canon_a = root / "fallback_canon_a"
    canon_b = root / "fallback_canon_b"
    canon_dst = root / "fallback_canon_dst"
    reset_dir(canon_a)
    reset_dir(canon_b)
    reset_dir(canon_dst)
    write_text(canon_a / "canon.txt", "bracket canon\n")
    proc = run_ks([f"+[{canon_a},{canon_b}]", str(canon_dst)])
    assert_success(checks, proc, "003.24 bracket canon role")
    checks.equal(read_text(canon_dst / "canon.txt"), "bracket canon\n", "003.24 bracket canon should be authoritative")

    sub_src = root / "fallback_sub_src"
    sub_dst = root / "fallback_sub_dst"
    sub_alt = root / "fallback_sub_alt"
    reset_dir(sub_src)
    if sub_dst.exists():
        shutil.rmtree(sub_dst)
    reset_dir(sub_alt)
    write_text(sub_src / "sub.txt", "bracket subordinate\n")
    proc = run_ks([f"+{sub_src}", f"-[{sub_dst},{sub_alt}]"])
    assert_success(checks, proc, "003.25/003.26 bracket subordinate role")
    checks.equal(read_text(sub_dst / "sub.txt"), "bracket subordinate\n", "003.25 bracket subordinate should receive outcome")


def dry_run_cases(checks: CheckRun, root: Path) -> None:
    src = root / "dry_src"
    dst = root / "dry_dst"
    reset_dir(src)
    reset_dir(dst)
    write_text(src / "planned.txt", "planned\n")

    proc = run_ks(["--dry-run", f"+{src}", str(dst)])
    checks.equal(proc.stderr, "", "003.32 dry-run should leave stderr empty")
    checks.check("dry run" in proc.stdout.lower(), "003.32 dry-run should identify read-only planning mode")
    checks.check(not (dst / "planned.txt").exists(), "003.32 dry-run should not write destination files")

    proc = run_ks([f"+{src}", str(dst)])
    assert_success(checks, proc, "003.33 normal run")
    checks.equal(read_text(dst / "planned.txt"), "planned\n", "003.33 without dry-run peer changes should occur")


def option_acceptance_case(checks: CheckRun, root: Path) -> None:
    src = root / "options_src"
    dst = root / "options_dst"
    reset_dir(src)
    reset_dir(dst)
    write_text(src / "options.txt", "options\n")
    proc = run_ks(
        [
            "--max-copies",
            "1",
            "--retries-copy",
            "2",
            "--retries-list",
            "2",
            "--timeout-conn",
            "5",
            "--timeout-idle",
            "5",
            "--verbosity",
            "error",
            "--keep-tmp-days",
            "3",
            "--keep-bak-days",
            "4",
            "--keep-del-days",
            "5",
            f"+{src}",
            str(dst),
        ]
    )
    assert_success(checks, proc, "003.34/003.36/003.38/003.40/003.42/003.44/003.46/003.48/003.50 global options")
    checks.equal(read_text(dst / "options.txt"), "options\n", "global option run should still sync")


def exclude_cases(checks: CheckRun, root: Path) -> None:
    src = root / "exclude_src"
    dst = root / "exclude_dst"
    reset_dir(src)
    reset_dir(dst)
    write_text(src / "keep.txt", "keep\n")
    write_text(src / "skip.txt", "skip\n")
    write_text(src / "nested" / "skip.txt", "nested skip\n")
    write_text(dst / "skip.txt", "destination stays\n")

    proc = run_ks(["-x", "skip.txt", "-x", "nested/skip.txt", f"+{src}", str(dst)])
    assert_success(checks, proc, "003.52/003.53/003.54 excludes")
    checks.equal(read_text(dst / "keep.txt"), "keep\n", "003.52 non-excluded file should sync")
    checks.equal(read_text(dst / "skip.txt"), "destination stays\n", "003.52 excluded existing file should be left untouched")
    checks.check(not (dst / "nested" / "skip.txt").exists(), "003.53 repeated exclude should add another excluded path")

    invalids = [
        ("/leading", "003.55 leading slash"),
        ("trailing/", "003.56 trailing slash"),
        ("bad\\separator", "003.57 backslash separator"),
        ("empty//segment", "003.58 empty segment"),
        ("dot/./segment", "003.59 dot segment"),
        ("dot/../segment", "003.60 dot-dot segment"),
    ]
    for relpath, label in invalids:
        proc = run_ks(["-x", relpath, f"+{src}", str(dst)])
        assert_validation_error(checks, proc, label)


def read_line_with_timeout(stream, timeout: float, label: str) -> str:
    box: dict[str, str | BaseException] = {}

    def reader() -> None:
        try:
            box["value"] = stream.readline()
        except BaseException as exc:  # pragma: no cover - reported as test failure
            box["value"] = exc

    thread = threading.Thread(target=reader, daemon=True)
    thread.start()
    thread.join(timeout)
    if thread.is_alive():
        raise TimeoutError(f"timed out reading {label}")
    value = box.get("value", "")
    if isinstance(value, BaseException):
        raise value
    return value


class SftpServer:
    def __init__(self, checks: CheckRun, root: Path, args: list[str]) -> None:
        self.checks = checks
        self.root = root
        self.stderr_lines: list[str] = []
        cmd = [str(bundled_uv()), "run", "--script", str(WORKSPACE / "extart" / "ephemeral-sftp-server.py"), *args]
        self.proc = subprocess.Popen(
            cmd,
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
        self.stderr_thread = threading.Thread(target=self._read_stderr, daemon=True)
        self.stderr_thread.start()
        port_line = read_line_with_timeout(self.proc.stdout, 10, "SFTP server port").strip()
        self.port = int(port_line)
        self.host_key = self._wait_for_host_key()
        ssh_dir = root / "home" / ".ssh"
        ssh_dir.mkdir(parents=True, exist_ok=True)
        known_hosts = ssh_dir / "known_hosts"
        known_hosts.write_text(f"[127.0.0.1]:{self.port} {self.host_key}\n", encoding="utf-8", newline="\n")
        self.env = {
            "HOME": str(root / "home"),
            "USERPROFILE": str(root / "home"),
            "SSH_AUTH_SOCK": "",
        }

    def _read_stderr(self) -> None:
        assert self.proc.stderr is not None
        for line in self.proc.stderr:
            self.stderr_lines.append(line.rstrip("\n"))

    def _wait_for_host_key(self) -> str:
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            for line in self.stderr_lines:
                if line.startswith("host key: "):
                    return line.removeprefix("host key: ").strip()
            if self.proc.poll() is not None:
                break
            time.sleep(0.05)
        raise TimeoutError("SFTP server did not report a host key")

    def stop(self) -> None:
        if self.proc.poll() is None:
            self.proc.terminate()
            try:
                self.proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait(timeout=5)


def sftp_cases(checks: CheckRun, root: Path) -> None:
    servers: list[SftpServer] = []
    try:
        password = "p@ss:word"
        server = SftpServer(checks, root / "sftp_password_server", ["--user", "alice", "--password", password])
        servers.append(server)
        local = root / "sftp_local"
        reset_dir(local)
        write_text(local / "sftp.txt", "sftp\n")
        encoded_password = quote(password, safe="")
        proc = run_ks([f"+{local}", f"sftp://alice:{encoded_password}@127.0.0.1:{server.port}/remote"], env=server.env)
        assert_success(checks, proc, "003.3/003.10/003.12/003.13/003.14 SFTP URL")

        user_server = SftpServer(checks, root / "sftp_current_user_server", ["--user", getpass.getuser()])
        servers.append(user_server)
        user_local = root / "sftp_current_user_local"
        reset_dir(user_local)
        write_text(user_local / "user.txt", "current user\n")
        proc = run_ks([f"+{user_local}", f"sftp://127.0.0.1:{user_server.port}/current-user"], env=user_server.env)
        assert_success(checks, proc, "003.11 SFTP URL without user")

        fallback_server = SftpServer(checks, root / "sftp_fallback_server", ["--user", "bob", "--password", "pw"])
        servers.append(fallback_server)
        fb_local = root / "sftp_fallback_local"
        blocked = root / "sftp_fallback_blocked"
        reset_dir(fb_local)
        write_text(blocked, "not a directory\n")
        write_text(fb_local / "fallback-sftp.txt", "fallback sftp\n")
        proc = run_ks(
            [
                f"+{fb_local}",
                f"[{blocked},sftp://bob:pw@127.0.0.1:{fallback_server.port}/fallback]",
            ],
            env=fallback_server.env,
        )
        assert_success(checks, proc, "003.22 fallback peer accepts SFTP URL")
    except Exception as exc:
        checks.failures.append(f"SFTP setup or execution failed: {exc}")
    finally:
        for server in reversed(servers):
            server.stop()


def main() -> int:
    checks = CheckRun()
    with tempfile.TemporaryDirectory(prefix="ks_req_003_") as temp_name:
        root = Path(temp_name)
        validation_cases(checks, root)
        local_sync_case(checks, root)
        relative_path_case(checks, root)
        absolute_path_cases(checks, root)
        role_cases(checks, root)
        fallback_cases(checks, root)
        dry_run_cases(checks, root)
        option_acceptance_case(checks, root)
        exclude_cases(checks, root)
        sftp_cases(checks, root)

    # not reasonably testable: 003.6 on non-Windows hosts, because a real
    # Windows drive path cannot be created portably on Linux or macOS.
    # not reasonably testable: 003.8 and 003.9 together without binding SSH port
    # 22, which would require privileged or environment-specific setup.
    # not reasonably testable: 003.27, 003.28, 003.29, and 003.30. Successful
    # timeout parsing has no stable end-to-end observable effect unless the
    # test relies on timing or sabotaged network behavior.
    # not reasonably testable: 003.35, 003.37, 003.39, 003.41, 003.43, 003.45,
    # 003.47, 003.49, and 003.51. These defaults are internal run settings
    # with no required stdout, stderr, exit-code, or filesystem signal in this
    # requirement.
    # not reasonably testable: 003.61 because operating systems do not allow a
    # NUL character inside a subprocess argument.

    if checks.failures:
        print("FAIL")
        for failure in checks.failures:
            print(f"- {failure}")
        return 1
    print("PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
