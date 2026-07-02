# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

from __future__ import annotations

import subprocess
import sys
import tempfile
from pathlib import Path


sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")


WORKSPACE_ROOT_LITERAL = Path("/home/ace/Desktop/prjx/kitchensync")
RELEASED_EXE_LITERAL = Path(
    "/home/ace/Desktop/prjx/kitchensync/released/kitchensync.exe"
)


def workspace_root() -> Path:
    if WORKSPACE_ROOT_LITERAL.exists():
        return WORKSPACE_ROOT_LITERAL
    return Path(__file__).resolve().parents[1]


def released_exe() -> Path:
    if RELEASED_EXE_LITERAL.exists():
        return RELEASED_EXE_LITERAL
    return workspace_root() / "released" / "kitchensync.exe"


class FailureCollector:
    def __init__(self) -> None:
        self.failures: list[str] = []

    def check(self, condition: bool, message: str) -> None:
        if not condition:
            self.failures.append(message)

    def extend(self, messages: list[str]) -> None:
        self.failures.extend(messages)


def run_command(args: list[str], cwd: Path, timeout_seconds: float = 20.0) -> tuple[int, str, str, list[str]]:
    failures: list[str] = []
    try:
        completed = subprocess.run(
            args,
            cwd=str(cwd),
            text=True,
            encoding="utf-8",
            errors="replace",
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=timeout_seconds,
            shell=False,
            check=False,
        )
        return completed.returncode, completed.stdout, completed.stderr, failures
    except subprocess.TimeoutExpired as exc:
        failures.append(
            f"process timed out after {timeout_seconds} seconds: {' '.join(args)}"
        )
        return -999, exc.stdout or "", exc.stderr or "", failures
    except OSError as exc:
        failures.append(f"failed to launch process {' '.join(args)}: {exc}")
        return -998, "", "", failures


def assert_successful_cli_run(
    collector: FailureCollector,
    label: str,
    args: list[str],
    cwd: Path,
) -> None:
    returncode, stdout, stderr, run_failures = run_command(args, cwd)
    collector.extend([f"{label}: {failure}" for failure in run_failures])
    collector.check(returncode == 0, f"{label}: expected exit 0, got {returncode}")
    collector.check(stderr == "", f"{label}: expected empty stderr, got {stderr!r}")
    collector.check(
        "sync complete" in stdout.splitlines(),
        f"{label}: expected stdout to include a 'sync complete' line, got {stdout!r}",
    )
    collector.check(
        "dry run" in stdout.lower(),
        f"{label}: expected dry-run invocation to mention dry run on stdout, got {stdout!r}",
    )


def check_release_artifacts(collector: FailureCollector, exe: Path) -> None:
    released_dir = workspace_root() / "released"
    collector.check(exe.is_file(), "001.1: released/kitchensync.exe must exist as a file")

    if not released_dir.exists():
        collector.check(False, "001.2: released directory must exist")
        return

    shipped_files = sorted(
        path.relative_to(released_dir).as_posix()
        for path in released_dir.rglob("*")
        if path.is_file()
    )
    collector.check(
        shipped_files == ["kitchensync.exe"],
        "001.2: released directory must contain exactly kitchensync.exe; "
        f"found {shipped_files!r}",
    )


def check_direct_invocation(collector: FailureCollector, exe: Path) -> None:
    returncode, stdout, stderr, run_failures = run_command([str(exe)], exe.parent)
    collector.extend([f"001.3/001.4: {failure}" for failure in run_failures])
    collector.check(
        returncode == 0,
        f"001.3/001.4: no-argument CLI invocation should exit 0, got {returncode}",
    )
    collector.check(
        stderr == "",
        f"001.3/001.4: no-argument CLI invocation should leave stderr empty, got {stderr!r}",
    )
    collector.check(
        "Usage: kitchensync [options] <peer> <peer> [<peer>...]" in stdout,
        "001.3/001.4: direct command-line invocation should print the CLI help text",
    )


def check_local_path_invocation(collector: FailureCollector, exe: Path) -> None:
    with tempfile.TemporaryDirectory(prefix="kitchensync-001-paths-") as tmp_name:
        tmp = Path(tmp_name)
        peer_a = tmp / "peer-a"
        peer_b = tmp / "peer-b"
        peer_a.mkdir()
        peer_b.mkdir()

        assert_successful_cli_run(
            collector,
            "001.5/001.6/001.7 path peers",
            [str(exe), "--dry-run", f"+{peer_a}", str(peer_b)],
            exe.parent,
        )


def check_file_url_invocation(collector: FailureCollector, exe: Path) -> None:
    with tempfile.TemporaryDirectory(prefix="kitchensync-001-urls-") as tmp_name:
        tmp = Path(tmp_name)
        peer_a = tmp / "peer-a"
        peer_b = tmp / "peer-b"
        peer_a.mkdir()
        peer_b.mkdir()

        assert_successful_cli_run(
            collector,
            "001.6 file URL peers",
            [str(exe), "--dry-run", f"+{peer_a.as_uri()}", peer_b.as_uri()],
            exe.parent,
        )


def check_additional_peer_invocation(collector: FailureCollector, exe: Path) -> None:
    with tempfile.TemporaryDirectory(prefix="kitchensync-001-extra-peer-") as tmp_name:
        tmp = Path(tmp_name)
        peer_a = tmp / "peer-a"
        peer_b = tmp / "peer-b"
        peer_c = tmp / "peer-c"
        peer_a.mkdir()
        peer_b.mkdir()
        peer_c.mkdir()

        assert_successful_cli_run(
            collector,
            "001.8 additional peer operand",
            [str(exe), "--dry-run", f"+{peer_a}", str(peer_b), str(peer_c)],
            exe.parent,
        )


def main() -> int:
    collector = FailureCollector()
    exe = released_exe()

    check_release_artifacts(collector, exe)
    check_direct_invocation(collector, exe)
    check_local_path_invocation(collector, exe)
    check_file_url_invocation(collector, exe)
    check_additional_peer_invocation(collector, exe)

    if collector.failures:
        print("FAIL")
        for index, failure in enumerate(collector.failures, start=1):
            print(f"{index}. {failure}")
        return 1

    print("PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
