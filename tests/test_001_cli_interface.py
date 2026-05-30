#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# ///

"""End-to-end CLI validation test for reqs/001_cli-interface.md."""

from __future__ import annotations

import subprocess
import sys
import tempfile
from pathlib import Path


sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")



def resolve_workspace_root() -> Path:
    literal_root = Path(r"C:\\Users\\human\\Desktop\\prjx\\kitchensync")
    if literal_root.exists():
        return literal_root
    return Path(__file__).resolve().parents[1]


WORKSPACE_ROOT = resolve_workspace_root()
BINARY = WORKSPACE_ROOT / "released" / ("kitchensync.exe" if sys.platform == "win32" else "kitchensync")


def run_cli(args: list[str], timeout_seconds: float = 5.0) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(BINARY), *args],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=timeout_seconds,
    )


def run_with_peers(
    *,
    req_id: str,
    failures: list[str],
    before: tuple[str, ...] = (),
    after: tuple[str, ...] = (),
    expected_exit: int,
    peer_count: int = 2,
    plus_peer_positions: frozenset[int] = frozenset(),
    timeout_seconds: float = 5.0,
) -> None:
    with tempfile.TemporaryDirectory(prefix="ks-cli-interface-") as root:
        peer_root = Path(root)
        peers: list[str] = []
        for index in range(peer_count):
            peer = peer_root / f"peer-{index}"
            peer.mkdir()
            peer_arg = str(peer)
            if index in plus_peer_positions:
                peer_arg = f"+{peer_arg}"
            peers.append(peer_arg)

        command = [*before, *peers, *after]

        try:
            result = run_cli(command, timeout_seconds=timeout_seconds)
        except FileNotFoundError:
            failures.append(f"{req_id}: executable not found at {BINARY}")
            return
        except subprocess.TimeoutExpired:
            failures.append(f"{req_id}: command timed out after {timeout_seconds:.1f}s; command={command!r}")
            return

    if result.returncode != expected_exit:
        failures.append(
            f"{req_id}: expected exit code {expected_exit}, got {result.returncode}; "
            f"command={command!r}; stdout={result.stdout!r}; stderr={result.stderr!r}"
        )


def main() -> None:
    failures: list[str] = []

    # 001.1
    expected_name = "kitchensync.exe" if sys.platform == "win32" else "kitchensync"

    if BINARY.name != expected_name:
        failures.append(
            f"001.1: expected released executable name {expected_name!r}, got {BINARY.name!r}"
        )

    if not BINARY.exists():
        failures.append(f"001.1: released executable not found at {BINARY}")

    if not failures:
        # 001.2
        run_with_peers(
            req_id="001.2",
            failures=failures,
            before=("--dry-run", "--verbosity", "info"),
            expected_exit=0,
        )

        # 001.3
        run_with_peers(
            req_id="001.3",
            failures=failures,
            before=("--dry-run",),
            peer_count=1,
            expected_exit=1,
        )

        # 001.4
        run_with_peers(
            req_id="001.4",
            failures=failures,
            before=("--dry-run",),
            plus_peer_positions=frozenset({0, 1}),
            expected_exit=1,
        )

        # 001.5
        run_with_peers(req_id="001.5", failures=failures, before=("--dry-run",), expected_exit=0)

        # 001.6, 001.9, 001.12, 001.15, 001.18, 001.21, 001.24, 001.27
        accepted_positive_int_flags = {
            "001.6": ("--max-copies", "2"),
            "001.9": ("--retries-copy", "2"),
            "001.12": ("--retries-list", "2"),
            "001.15": ("--timeout-conn", "30"),
            "001.18": ("--timeout-idle", "30"),
            "001.21": ("--keep-tmp-days", "30"),
            "001.24": ("--keep-bak-days", "30"),
            "001.27": ("--keep-del-days", "30"),
        }

        for req_id, (flag, value) in accepted_positive_int_flags.items():
            run_with_peers(
                req_id=req_id,
                failures=failures,
                before=("--dry-run", flag, value),
                expected_exit=0,
            )

        # 001.7, 001.8, 001.10, 001.11, 001.13, 001.14, 001.16, 001.17,
        # 001.19, 001.20, 001.22, 001.23, 001.25, 001.26, 001.28, 001.29
        rejected_int_flags = {
            "001.7": ("--max-copies", "0"),
            "001.8": ("--max-copies", "bad"),
            "001.10": ("--retries-copy", "0"),
            "001.11": ("--retries-copy", "bad"),
            "001.13": ("--retries-list", "0"),
            "001.14": ("--retries-list", "bad"),
            "001.16": ("--timeout-conn", "0"),
            "001.17": ("--timeout-conn", "bad"),
            "001.19": ("--timeout-idle", "0"),
            "001.20": ("--timeout-idle", "bad"),
            "001.22": ("--keep-tmp-days", "0"),
            "001.23": ("--keep-tmp-days", "bad"),
            "001.25": ("--keep-bak-days", "0"),
            "001.26": ("--keep-bak-days", "bad"),
            "001.28": ("--keep-del-days", "0"),
            "001.29": ("--keep-del-days", "bad"),
        }

        for req_id, (flag, value) in rejected_int_flags.items():
            run_with_peers(
                req_id=req_id,
                failures=failures,
                before=("--dry-run", flag, value),
                expected_exit=1,
            )

        # 001.30, 001.31, 001.32, 001.33
        for req_id, level in {
            "001.30": "error",
            "001.31": "info",
            "001.32": "debug",
            "001.33": "trace",
        }.items():
            run_with_peers(
                req_id=req_id,
                failures=failures,
                before=("--dry-run", "--verbosity", level),
                expected_exit=0,
            )

        # 001.34
        run_with_peers(
            req_id="001.34",
            failures=failures,
            before=("--dry-run", "--verbosity", "quiet"),
            expected_exit=1,
        )

        # 001.35
        run_with_peers(
            req_id="001.35",
            failures=failures,
            before=("--dry-run", "--not-a-real-flag"),
            expected_exit=1,
        )

        # 001.36
        run_with_peers(
            req_id="001.36",
            failures=failures,
            before=("--dry-run", "-x", "single"),
            expected_exit=0,
        )

        # 001.37
        run_with_peers(
            req_id="001.37",
            failures=failures,
            before=("--dry-run", "-x", "a/b/c"),
            expected_exit=0,
        )

        # 001.38
        run_with_peers(
            req_id="001.38",
            failures=failures,
            before=("--dry-run", "-x", "a", "-x", "a/b"),
            expected_exit=0,
        )

        # 001.39
        run_with_peers(
            req_id="001.39",
            failures=failures,
            before=("--dry-run", "-x", "/leading"),
            expected_exit=1,
        )

        # 001.40
        run_with_peers(
            req_id="001.40",
            failures=failures,
            before=("--dry-run", "-x", "trailing/"),
            expected_exit=1,
        )

        # 001.41
        run_with_peers(
            req_id="001.41",
            failures=failures,
            before=("--dry-run", "-x", "bad\\path"),
            expected_exit=1,
        )

        # 001.42
        run_with_peers(
            req_id="001.42",
            failures=failures,
            before=("--dry-run", "-x", "a//b"),
            expected_exit=1,
        )

        # 001.43
        run_with_peers(
            req_id="001.43",
            failures=failures,
            before=("--dry-run", "-x", "."),
            expected_exit=1,
        )

        # 001.44
        run_with_peers(
            req_id="001.44",
            failures=failures,
            before=("--dry-run", "-x", ".."),
            expected_exit=1,
        )

        # 001.45
        # not reasonably testable: 001.45

        # 001.46
        run_with_peers(
            req_id="001.46",
            failures=failures,
            before=("--dry-run", "--max-copies", "0"),
            expected_exit=1,
        )

        # 001.47
        for case in (
            ("001.47", ("--dry-run", "--max-copies", "--retries-copy")),
            ("001.47", ("--dry-run", "--retries-copy", "--retries-list")),
            ("001.47", ("--dry-run", "--retries-list", "--timeout-conn")),
            ("001.47", ("--dry-run", "--timeout-conn", "--timeout-idle")),
            ("001.47", ("--dry-run", "--timeout-idle", "--keep-tmp-days")),
            ("001.47", ("--dry-run", "--keep-tmp-days", "--keep-bak-days")),
            ("001.47", ("--dry-run", "--keep-bak-days", "--keep-del-days")),
            ("001.47", ("--dry-run", "--keep-del-days", "--verbosity")),
            ("001.47", ("--dry-run", "--verbosity", "--max-copies")),
        ):
            req_id, command_before = case
            run_with_peers(
                req_id=req_id,
                failures=failures,
                before=command_before,
                expected_exit=1,
            )

        # 001.48
        run_with_peers(
            req_id="001.48",
            failures=failures,
            before=("--dry-run",),
            after=("-x", "nested/path"),
            expected_exit=0,
            peer_count=2,
        )

    if failures:
        print("FAIL: test_001_cli_interface.py", file=sys.stderr)
        for failure in failures:
            print(f"  - {failure}", file=sys.stderr)
        raise SystemExit(1)

    print("PASS: test_001_cli_interface.py")
    raise SystemExit(0)


if __name__ == "__main__":
    main()
