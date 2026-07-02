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


LITERAL_WORKSPACE_ROOT = Path("/home/ace/Desktop/prjx/kitchensync")
LITERAL_RELEASED_EXECUTABLE = LITERAL_WORKSPACE_ROOT / "released" / "kitchensync.exe"


def workspace_root() -> Path:
    if LITERAL_WORKSPACE_ROOT.exists():
        return LITERAL_WORKSPACE_ROOT
    return Path(__file__).resolve().parents[1]


def released_executable() -> Path:
    if LITERAL_RELEASED_EXECUTABLE.exists():
        return LITERAL_RELEASED_EXECUTABLE
    return workspace_root() / "released" / "kitchensync.exe"


class Checks:
    def __init__(self) -> None:
        self.failures: list[str] = []

    def equal(self, actual: object, expected: object, message: str) -> None:
        if actual != expected:
            self.failures.append(f"{message}: expected {expected!r}, got {actual!r}")

    def true(self, condition: bool, message: str) -> None:
        if not condition:
            self.failures.append(message)

    def finish(self) -> None:
        if not self.failures:
            print("PASS")
            raise SystemExit(0)
        print("FAIL")
        for index, failure in enumerate(self.failures, start=1):
            print(f"{index}. {failure}")
        raise SystemExit(1)


def run_kitchensync(checks: Checks, args: list[str], label: str) -> subprocess.CompletedProcess[str] | None:
    executable = released_executable()
    try:
        return subprocess.run(
            [str(executable), *args],
            cwd=str(executable.parent),
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=20,
            check=False,
        )
    except FileNotFoundError as exc:
        checks.failures.append(f"{label}: executable not found: {exc}")
    except PermissionError as exc:
        checks.failures.append(f"{label}: executable is not directly invocable: {exc}")
    except subprocess.TimeoutExpired:
        checks.failures.append(f"{label}: process did not exit within the timeout")
    except OSError as exc:
        checks.failures.append(f"{label}: process launch failed: {exc}")
    return None


def assert_clean_process(
    checks: Checks,
    result: subprocess.CompletedProcess[str] | None,
    label: str,
) -> None:
    if result is None:
        return
    checks.equal(result.returncode, 0, f"{label}: exit code")
    checks.equal(result.stderr, "", f"{label}: stderr must be empty")


def check_released_artifact_boundary(checks: Checks) -> None:
    executable = released_executable()
    released_dir = executable.parent
    checks.true(released_dir.is_dir(), "001.1: released directory must exist")
    checks.true(executable.is_file(), "001.1: released/kitchensync.exe must be a file")
    if released_dir.is_dir():
        entries = sorted(path.name for path in released_dir.iterdir())
        checks.equal(entries, ["kitchensync.exe"], "001.1: released directory contents")


def check_direct_cli_invocation(checks: Checks) -> None:
    result = run_kitchensync(checks, [], "001.2/001.3: direct no-argument CLI invocation")
    assert_clean_process(checks, result, "001.2/001.3: direct no-argument CLI invocation")
    if result is not None:
        checks.true(
            "Usage: kitchensync [options] <peer> <peer> [<peer>...]" in result.stdout,
            "001.2/001.3: direct invocation should print the CLI help to stdout",
        )


def check_two_peer_invocation_with_option_first(checks: Checks) -> None:
    with tempfile.TemporaryDirectory(prefix="kitchensync-001-two-peer-") as temp_root:
        root = Path(temp_root)
        peer_a = root / "peer-a"
        peer_b = root / "peer-b"
        peer_a.mkdir()
        peer_b.mkdir()

        result = run_kitchensync(
            checks,
            ["--dry-run", f"+{peer_a}", str(peer_b)],
            "001.4/001.5: option before two peer operands",
        )
        assert_clean_process(checks, result, "001.4/001.5: option before two peer operands")
        if result is not None:
            checks.true(
                "dry run" in result.stdout.lower(),
                "001.4/001.5: dry-run two-peer invocation should report dry run on stdout",
            )


def check_additional_peer_operand(checks: Checks) -> None:
    with tempfile.TemporaryDirectory(prefix="kitchensync-001-three-peer-") as temp_root:
        root = Path(temp_root)
        peer_a = root / "peer-a"
        peer_b = root / "peer-b"
        peer_c = root / "peer-c"
        peer_a.mkdir()
        peer_b.mkdir()
        peer_c.mkdir()

        result = run_kitchensync(
            checks,
            ["--dry-run", f"+{peer_a}", str(peer_b), str(peer_c)],
            "001.6: additional peer operand after the first two peers",
        )
        assert_clean_process(checks, result, "001.6: additional peer operand after the first two peers")
        if result is not None:
            checks.true(
                "dry run" in result.stdout.lower(),
                "001.6: dry-run three-peer invocation should report dry run on stdout",
            )


def main() -> None:
    checks = Checks()
    check_released_artifact_boundary(checks)
    check_direct_cli_invocation(checks)
    check_two_peer_invocation_with_option_first(checks)
    check_additional_peer_operand(checks)
    checks.finish()


if __name__ == "__main__":
    main()
