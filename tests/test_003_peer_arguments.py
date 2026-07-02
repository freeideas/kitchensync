# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

import os
import queue
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from dataclasses import dataclass
from pathlib import Path


sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")


WORKSPACE = Path("/home/ace/Desktop/prjx/kitchensync")
EXE = Path("/home/ace/Desktop/prjx/kitchensync/released/kitchensync.exe")


# not reasonably testable: 003.16, 003.17. The value-level effect of URL
# timeout override settings requires a timing-sensitive SFTP endpoint.
# not reasonably testable: 003.29, 003.30. The idle keep-alive timeout value is
# only observable through connection lifetime behavior.
# not reasonably testable: 003.37, 003.38. Deletion-record retention is stored
# in peer snapshot databases, which are not part of this syntax requirement's
# released command-line surface.
# not reasonably testable: 003.40. A Windows drive path cannot be exercised
# portably without writing outside the temp workspace on Windows.
# not reasonably testable: 003.45. The no-user SFTP form depends on host user
# and key fallback state rather than only command-line parsing.
# not reasonably testable: 003.58. Python subprocess APIs reject embedded NUL
# characters before the product process can receive the argument.


@dataclass
class RunResult:
    args: list[str]
    returncode: int
    stdout: str
    stderr: str


def run_kitchensync(
    args: list[str],
    cwd: Path,
    env: dict[str, str] | None = None,
    timeout: float = 30.0,
) -> RunResult:
    full_env = os.environ.copy()
    if env:
        full_env.update(env)
    try:
        completed = subprocess.run(
            [str(EXE), *args],
            cwd=str(cwd),
            env=full_env,
            text=True,
            encoding="utf-8",
            errors="replace",
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=timeout,
            shell=False,
            check=False,
        )
        return RunResult(args, completed.returncode, completed.stdout, completed.stderr)
    except subprocess.TimeoutExpired as exc:
        return RunResult(
            args,
            124,
            exc.stdout or "",
            (exc.stderr or "") + "\nprocess timed out",
        )


def record(condition: bool, failures: list[str], message: str) -> None:
    if not condition:
        failures.append(message)


def record_success(result: RunResult, failures: list[str], label: str) -> None:
    record(
        result.returncode == 0,
        failures,
        f"{label}: expected exit 0, got {result.returncode}; "
        f"stdout={result.stdout!r}; stderr={result.stderr!r}; args={result.args!r}",
    )
    record(
        result.stderr == "",
        failures,
        f"{label}: expected stderr to be empty, got {result.stderr!r}",
    )


def record_validation_error(result: RunResult, failures: list[str], label: str) -> None:
    record(
        result.returncode == 1,
        failures,
        f"{label}: expected exit 1, got {result.returncode}; "
        f"stdout={result.stdout!r}; stderr={result.stderr!r}; args={result.args!r}",
    )
    record(
        "Usage: kitchensync" in result.stdout,
        failures,
        f"{label}: expected validation error to include help on stdout; "
        f"stdout={result.stdout!r}",
    )
    record(
        result.stderr == "",
        failures,
        f"{label}: expected stderr to be empty, got {result.stderr!r}",
    )


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")


def assert_file_text(path: Path, expected: str, failures: list[str], label: str) -> None:
    if not path.exists():
        failures.append(f"{label}: expected {path} to exist")
        return
    actual = path.read_text(encoding="utf-8", errors="replace")
    record(actual == expected, failures, f"{label}: expected {expected!r}, got {actual!r}")


def read_line_with_timeout(stream, timeout: float, label: str) -> str:
    lines: queue.Queue[str] = queue.Queue(maxsize=1)

    def reader() -> None:
        line = stream.readline()
        lines.put(line)

    thread = threading.Thread(target=reader, daemon=True)
    thread.start()
    try:
        return lines.get(timeout=timeout)
    except queue.Empty as exc:
        raise TimeoutError(f"timed out reading {label}") from exc


def bundled_uv() -> Path:
    if sys.platform.startswith("win"):
        return WORKSPACE / "aitc" / "bin" / "uv.exe"
    if sys.platform == "darwin":
        return WORKSPACE / "aitc" / "bin" / "uv.mac"
    return WORKSPACE / "aitc" / "bin" / "uv.linux"


def start_sftp_server(temp_root: Path, failures: list[str]):
    server = WORKSPACE / "extart" / "ephemeral-sftp-server.py"
    process = subprocess.Popen(
        [
            str(bundled_uv()),
            "run",
            "--script",
            str(server),
            "--user",
            "syncuser",
            "--password",
            "p@ss:word",
        ],
        cwd=str(WORKSPACE),
        text=True,
        encoding="utf-8",
        errors="replace",
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        shell=False,
    )
    try:
        assert process.stdout is not None
        assert process.stderr is not None
        port_line = read_line_with_timeout(process.stdout, 10.0, "SFTP port")
        host_key_line = read_line_with_timeout(process.stderr, 10.0, "SFTP host key")
        port = int(port_line.strip())
        prefix = "host key: "
        if not host_key_line.startswith(prefix):
            failures.append(f"SFTP server: expected host key line, got {host_key_line!r}")
            process.terminate()
            return None
        known_hosts = temp_root / "home" / ".ssh" / "known_hosts"
        known_hosts.parent.mkdir(parents=True, exist_ok=True)
        known_hosts.write_text(
            f"[127.0.0.1]:{port} {host_key_line[len(prefix):].strip()}\n",
            encoding="utf-8",
            newline="\n",
        )
        return process, port, known_hosts.parent.parent
    except Exception as exc:
        failures.append(f"SFTP server failed to start: {exc}")
        process.terminate()
        try:
            process.wait(timeout=5.0)
        except subprocess.TimeoutExpired:
            process.kill()
        return None


def stop_process(process: subprocess.Popen) -> None:
    if process.poll() is not None:
        return
    process.terminate()
    try:
        process.wait(timeout=5.0)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=5.0)


def test_local_peer_forms(root: Path, failures: list[str]) -> None:
    root.mkdir(parents=True, exist_ok=True)
    canon = root / "canon"
    peer = root / "peer"
    canon.mkdir()
    peer.mkdir()
    write_text(canon / "alpha.txt", "canon\n")

    result = run_kitchensync([f"+{canon}", str(peer)], root)
    record_success(result, failures, "003.1 003.3 003.4 local canon path")
    assert_file_text(peer / "alpha.txt", "canon\n", failures, "canon local copy")

    write_text(peer / "alpha.txt", "peer newer\n")
    newer = time.time() + 20.0
    os.utime(peer / "alpha.txt", (newer, newer))
    result = run_kitchensync([str(canon), str(peer)], root)
    record_success(result, failures, "003.6 normal bidirectional peer")
    assert_file_text(canon / "alpha.txt", "peer newer\n", failures, "normal peer update")

    absolute_a = root / "absolute_a"
    absolute_b = root / "absolute_b"
    result = run_kitchensync([f"+{absolute_a}", str(absolute_b)], root)
    record_success(result, failures, "003.39 Unix-style absolute local peer")
    record(absolute_a.exists() and absolute_b.exists(), failures, "absolute peers should be created")

    relative_cwd = root / "relative_case"
    relative_cwd.mkdir()
    result = run_kitchensync(["+rel_a", "rel_b"], relative_cwd)
    record_success(result, failures, "003.41 relative local peer")
    record((relative_cwd / "rel_a").exists(), failures, "relative canon path should be created")
    record((relative_cwd / "rel_b").exists(), failures, "relative peer path should be created")


def test_role_markers_and_fallbacks(root: Path, failures: list[str]) -> None:
    root.mkdir(parents=True, exist_ok=True)
    canon = root / "roles_canon"
    sub_a = root / "roles_sub_a"
    sub_b = root / "roles_sub_b"
    for path in (canon, sub_a, sub_b):
        path.mkdir()
    write_text(canon / "role.txt", "role source\n")

    result = run_kitchensync([f"+{canon}", f"-{sub_a}", f"-{sub_b}"], root)
    record_success(result, failures, "003.5 003.8 multiple subordinate peers")
    assert_file_text(sub_a / "role.txt", "role source\n", failures, "first subordinate")
    assert_file_text(sub_b / "role.txt", "role source\n", failures, "second subordinate")

    too_many = run_kitchensync([f"+{canon}", f"+{sub_a}"], root)
    record_validation_error(too_many, failures, "003.7 rejects multiple canon peers")

    first = root / "fallback_first"
    second = root / "fallback_second"
    target = root / "fallback_target"
    for path in (first, second, target):
        path.mkdir()
    write_text(first / "winner.txt", "first fallback\n")
    result = run_kitchensync([f"+[{first},{second}]", str(target), "--dry-run"], root)
    record_success(result, failures, "003.9 003.10 003.11 bracketed canon fallback")
    record("C winner.txt" in result.stdout, failures, "fallback order should choose first URL")
    record("dry run" in result.stdout.lower(), failures, "003.19 dry-run output should mention dry run")
    record(not (target / "winner.txt").exists(), failures, "003.19 dry-run must not write destination")

    bracket_sub_a = root / "bracket_sub_a"
    bracket_sub_b = root / "bracket_sub_b"
    bracket_canon = root / "bracket_canon"
    for path in (bracket_sub_a, bracket_sub_b, bracket_canon):
        path.mkdir()
    write_text(bracket_canon / "subordinate.txt", "bracket subordinate\n")
    result = run_kitchensync([f"+{bracket_canon}", f"-[{bracket_sub_a},{bracket_sub_b}]"], root)
    record_success(result, failures, "003.12 bracketed subordinate fallback")
    assert_file_text(
        bracket_sub_a / "subordinate.txt",
        "bracket subordinate\n",
        failures,
        "bracket subordinate first fallback",
    )
    record(
        not (bracket_sub_b / "subordinate.txt").exists(),
        failures,
        "fallback group should use one winning URL, not every URL",
    )


def test_options_and_excludes(root: Path, failures: list[str]) -> None:
    root.mkdir(parents=True, exist_ok=True)
    canon = root / "options_canon"
    peer = root / "options_peer"
    canon.mkdir()
    peer.mkdir()
    write_text(canon / "copy_one.txt", "one\n")
    write_text(canon / "copy_two.txt", "two\n")

    result = run_kitchensync(
        [
            f"+{canon}",
            str(peer),
            "--max-copies",
            "2",
            "--retries-copy",
            "2",
            "--retries-list",
            "2",
            "--timeout-conn",
            "5",
            "--timeout-idle",
            "5",
            "--verbosity",
            "trace",
            "--keep-tmp-days",
            "3",
            "--keep-bak-days",
            "4",
            "--keep-del-days",
            "5",
        ],
        root,
    )
    record_success(result, failures, "003.21 003.23 003.25 003.27 003.31 003.33 003.35 003.37 options")
    record("copy-slots active=" in result.stdout, failures, "trace verbosity should show copy-slot logs")
    record("/2" in result.stdout, failures, "--max-copies value should appear in trace denominator")

    default_canon = root / "default_canon"
    default_peer = root / "default_peer"
    default_canon.mkdir()
    default_peer.mkdir()
    write_text(default_canon / "default.txt", "default\n")
    result = run_kitchensync([f"+{default_canon}", str(default_peer), "--verbosity", "trace"], root)
    record_success(result, failures, "003.22 default max-copies")
    record("/10" in result.stdout, failures, "default max-copies should appear as 10 in trace logs")

    info_canon = root / "info_canon"
    info_peer = root / "info_peer"
    info_canon.mkdir()
    info_peer.mkdir()
    write_text(info_canon / "info.txt", "info\n")
    result = run_kitchensync([f"+{info_canon}", str(info_peer)], root)
    record_success(result, failures, "003.20 003.24 003.26 003.28 003.32 003.34 003.36 003.38 defaults")
    record(
        "copy-slots active=" not in result.stdout,
        failures,
        "default verbosity info should not include trace copy-slot logs",
    )

    dry_missing_a = root / "dry_missing_a"
    dry_missing_b = root / "dry_missing_b"
    result = run_kitchensync([f"+{dry_missing_a}", str(dry_missing_b), "--dry-run"], root)
    record(
        result.returncode != 0,
        failures,
        "dry-run with missing peer roots should fail before creating them",
    )
    record(not dry_missing_a.exists(), failures, "dry-run should not create missing canon root")
    record(not dry_missing_b.exists(), failures, "dry-run should not create missing peer root")

    excl_canon = root / "exclude_canon"
    excl_peer = root / "exclude_peer"
    excl_canon.mkdir()
    excl_peer.mkdir()
    write_text(excl_canon / "keep.txt", "keep\n")
    write_text(excl_canon / "skip.txt", "skip\n")
    write_text(excl_canon / "dir" / "skip2.txt", "skip2\n")
    result = run_kitchensync(
        [f"+{excl_canon}", str(excl_peer), "-x", "skip.txt", "-x", "dir/skip2.txt"],
        root,
    )
    record_success(result, failures, "003.49 003.50 003.51 repeated slash excludes")
    assert_file_text(excl_peer / "keep.txt", "keep\n", failures, "non-excluded file")
    record(not (excl_peer / "skip.txt").exists(), failures, "excluded file should not copy")
    record(not (excl_peer / "dir" / "skip2.txt").exists(), failures, "excluded nested file should not copy")

    invalid_excludes = {
        "003.52 leading slash": "/absolute",
        "003.53 trailing slash": "trailing/",
        "003.54 backslash separator": "bad\\path",
        "003.55 empty segment": "bad//path",
        "003.56 dot segment": "bad/./path",
        "003.57 dotdot segment": "bad/../path",
    }
    for label, relpath in invalid_excludes.items():
        result = run_kitchensync([f"+{excl_canon}", str(excl_peer), "-x", relpath], root)
        record_validation_error(result, failures, label)


def test_sftp_and_query_forms(root: Path, failures: list[str]) -> None:
    root.mkdir(parents=True, exist_ok=True)
    started = start_sftp_server(root, failures)
    if started is None:
        return
    process, port, home = started
    try:
        canon = root / "sftp_canon"
        canon.mkdir()
        write_text(canon / "remote.txt", "remote\n")
        env = {
            "HOME": str(home),
            "USERPROFILE": str(home),
            "SSH_AUTH_SOCK": "",
        }
        sftp_url = (
            f"sftp://syncuser:p%40ss%3Aword@127.0.0.1:{port}"
            "/remote-peer?timeout-conn=5&timeout-idle=5"
        )
        result = run_kitchensync([f"+{canon}", sftp_url, "--timeout-conn", "1", "--timeout-idle", "1"], root, env=env)
        record_success(
            result,
            failures,
            "003.2 003.14 003.15 003.42 003.44 003.46 003.47 003.48 SFTP URL",
        )

        default_port_url = "sftp://syncuser:p%40ss%3Aword@127.0.0.1/remote-peer"
        result = run_kitchensync([f"+{canon}", default_port_url, "--timeout-conn", "1"], root, env=env)
        record(
            "Usage: kitchensync" not in result.stdout,
            failures,
            "003.43 default-port SFTP URL should pass argument validation",
        )
        record(result.stderr == "", failures, "003.43 default-port SFTP URL should keep stderr empty")

        bad_query = run_kitchensync([f"+{canon}", f"sftp://syncuser@127.0.0.1:{port}/x?max-copies=2"], root, env=env)
        record_validation_error(bad_query, failures, "003.18 rejects URL max-copies query parameter")
        record("max-copies" in bad_query.stdout, failures, "max-copies rejection should name max-copies")
    finally:
        stop_process(process)


def main() -> int:
    failures: list[str] = []
    record(EXE.exists(), failures, f"released executable should exist at {EXE}")

    with tempfile.TemporaryDirectory(prefix="kitchensync-003-") as temp:
        root = Path(temp)
        test_local_peer_forms(root / "local", failures)
        test_role_markers_and_fallbacks(root / "roles", failures)
        test_options_and_excludes(root / "options", failures)
        test_sftp_and_query_forms(root / "sftp", failures)

    if failures:
        print(f"{len(failures)} failure(s):")
        for failure in failures:
            print(f"- {failure}")
        return 1
    print("all checks passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
