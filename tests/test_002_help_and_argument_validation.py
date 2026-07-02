# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

from __future__ import annotations

import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path


sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")


WORKSPACE_ROOT = Path("/home/ace/Desktop/prjx/kitchensync")
KITCHENSYNC = Path("/home/ace/Desktop/prjx/kitchensync/released/kitchensync.exe")

EXPECTED_HELP = """Usage: kitchensync [options] <peer> <peer> [<peer>...]

Synchronize file trees across multiple peers.

Running with no arguments prints this help. See the specs for full behavior.

Peers:
  /path or c:\\path                 Local path (same as file://)
  sftp://user@host/path            Remote over SSH
  sftp://user@host:port/path       Non-standard SSH port
  sftp://host/path                 Remote over SSH, current OS user
  sftp://user:password@host/path   Inline password (prefer SSH keys)

Prefix modifiers:
  +<peer>                          Canon - this peer's state wins all conflicts
  -<peer>                          Subordinate - overwritten to match the group

Fallback URLs (multiple paths to the same data):
  [url1,url2,...]                  Try in order, first that connects wins
  +[url1,url2,...]                 Canon peer with fallbacks
  -[url1,url2,...]                 Subordinate peer with fallbacks

Per-URL settings (query string, inside quotes):
  "sftp://host/path?timeout-conn=60"     Connection timeout for this URL
  "sftp://host/path?timeout-idle=10"     SFTP idle keep-alive TTL for this URL
  "sftp://host/path?timeout-conn=60&timeout-idle=10"  Combine multiple

Options:
  --dry-run          Read-only and plan, but make no peer changes
  --max-copies N     Max active file copies across the whole run (default: 10)
  --retries-copy N   Give up copying after this many tries (default: 3)
  --retries-list N   Give up listing after this many tries (default: 3)
  --timeout-conn N   SSH handshake timeout in seconds (default: 30)
  --timeout-idle N   SFTP idle keep-alive TTL in seconds (default: 30)
  --verbosity LEVEL  Verbosity: error, info, debug, trace (default: info)
  -x RELPATH         Exclude relative slash path from sync; repeatable
  --keep-tmp-days N  Delete stale TMP staging after N days (default: 2)
  --keep-bak-days N  Delete displaced files (BAK/) after N days (default: 90)
  --keep-del-days N  Forget deletion records after N days (default: 180)

Quick start:
  kitchensync +c:/photos sftp://user@host/photos      First sync (c: is canon)
  kitchensync c:/photos sftp://host/photos            Bidirectional
  kitchensync c:/photos sftp://host/photos -/mnt/usb  Add USB as subordinate
  kitchensync c:/photos "sftp://user:p%40ss@host/photos"  Inline password

Canon (+) is required on first sync when no peer has snapshot history.
After the first sync, bidirectional sync works without canon.

Tip: if ssh user@host and cd /path works, sftp://user@host/path will too.

Displaced files are recoverable from nearby:
  .kitchensync/BAK/ directories (kept for --keep-bak-days days).
"""


@dataclass(frozen=True)
class RunResult:
    args: list[str]
    returncode: int
    stdout: str
    stderr: str


def run_kitchensync(args: list[str], timeout_seconds: float = 8.0) -> RunResult:
    completed = subprocess.run(
        [str(KITCHENSYNC), *args],
        cwd=str(WORKSPACE_ROOT),
        text=True,
        encoding="utf-8",
        errors="replace",
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=timeout_seconds,
        shell=False,
        check=False,
    )
    return RunResult(
        args=args,
        returncode=completed.returncode,
        stdout=completed.stdout,
        stderr=completed.stderr,
    )


def make_peers(parent: Path, name: str) -> tuple[Path, Path]:
    left = parent / name / "left"
    right = parent / name / "right"
    left.mkdir(parents=True, exist_ok=True)
    right.mkdir(parents=True, exist_ok=True)
    (left / "sample.txt").write_text("sample\n", encoding="utf-8", newline="\n")
    return left, right


def format_args(args: list[str]) -> str:
    return " ".join(args) if args else "<no arguments>"


def expect_help_success(failures: list[str]) -> None:
    try:
        result = run_kitchensync([])
    except Exception as exc:
        failures.append(f"002.1/002.2/002.3 no-argument help raised {exc!r}")
        return

    if result.stdout != EXPECTED_HELP:
        failures.append(
            "002.1 no-argument invocation did not print the exact help text "
            f"to stdout; got {result.stdout!r}"
        )
    if result.returncode != 0:
        failures.append(
            f"002.2 no-argument invocation exited {result.returncode}, expected 0"
        )
    if result.stderr != "":
        failures.append(
            f"002.3 no-argument invocation wrote to stderr: {result.stderr!r}"
        )


def expect_validation_failure(
    failures: list[str],
    req_ids: str,
    args: list[str],
    timeout_seconds: float = 8.0,
) -> None:
    try:
        result = run_kitchensync(args, timeout_seconds=timeout_seconds)
    except Exception as exc:
        failures.append(f"{req_ids} validation failure case raised {exc!r}")
        return

    prefix = f"{req_ids} for `{format_args(args)}`"
    if result.returncode != 1:
        failures.append(f"{prefix} exited {result.returncode}, expected 1")
    if result.stderr != "":
        failures.append(f"{prefix} wrote to stderr: {result.stderr!r}")
    if not result.stdout.endswith(EXPECTED_HELP):
        failures.append(f"{prefix} stdout did not end with the help text")
    error_text = result.stdout[: -len(EXPECTED_HELP)] if result.stdout.endswith(EXPECTED_HELP) else result.stdout
    if error_text.strip() == "":
        failures.append(f"{prefix} did not print an error message before help")


def expect_accepted(
    failures: list[str],
    req_ids: str,
    args: list[str],
    timeout_seconds: float = 12.0,
) -> None:
    try:
        result = run_kitchensync(args, timeout_seconds=timeout_seconds)
    except Exception as exc:
        failures.append(f"{req_ids} accepted invocation raised {exc!r}")
        return

    prefix = f"{req_ids} for `{format_args(args)}`"
    if result.returncode != 0:
        failures.append(
            f"{prefix} exited {result.returncode}, expected accepted dry-run exit 0; "
            f"stdout was {result.stdout!r}"
        )
    if result.stderr != "":
        failures.append(f"{prefix} wrote to stderr: {result.stderr!r}")
    if result.stdout.endswith(EXPECTED_HELP):
        failures.append(f"{prefix} printed validation help even though it should be accepted")


def accepted_cases(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks-002-accepted-") as tmp:
        root = Path(tmp)

        left, right = make_peers(root, "all-numeric-and-excludes")
        expect_accepted(
            failures,
            "002.7/002.8/002.9/002.10/002.11/002.12/002.13/002.14/002.15/002.27/002.29",
            [
                "--dry-run",
                "--max-copies",
                "1",
                "--retries-copy",
                "1",
                "--retries-list",
                "1",
                "--timeout-conn",
                "1",
                "--timeout-idle",
                "1",
                "--keep-tmp-days",
                "1",
                "--keep-bak-days",
                "1",
                "--keep-del-days",
                "1",
                "--verbosity",
                "trace",
                "-x",
                "excluded-file.txt",
                "-x",
                "dir/excluded-file.txt",
                f"+{left}",
                str(right),
            ],
        )

        for level, req_id in [
            ("error", "002.24"),
            ("info", "002.25"),
            ("debug", "002.26"),
        ]:
            left, right = make_peers(root, f"verbosity-{level}")
            expect_accepted(
                failures,
                req_id,
                ["--dry-run", "--verbosity", level, f"+{left}", str(right)],
            )

        left, right = make_peers(root, "url-timeout-conn")
        expect_accepted(
            failures,
            "002.36",
            ["--dry-run", f"+{left.as_uri()}?timeout-conn=1", right.as_uri()],
        )

        left, right = make_peers(root, "url-timeout-idle")
        expect_accepted(
            failures,
            "002.37",
            ["--dry-run", f"+{left.as_uri()}?timeout-idle=1", right.as_uri()],
        )

        left, right = make_peers(root, "url-both-timeouts")
        expect_accepted(
            failures,
            "002.38",
            [
                "--dry-run",
                f"+{left.as_uri()}?timeout-conn=1&timeout-idle=1",
                right.as_uri(),
            ],
        )


def invalid_cases(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks-002-invalid-") as tmp:
        root = Path(tmp)
        left, right = make_peers(root, "peers")
        canon = f"+{left}"
        other = str(right)

        cases = [
            ("002.4/002.43/002.44/002.45", ["--dry-run", canon]),
            ("002.5/002.43/002.44/002.45", ["--dry-run", canon, f"+{right}"]),
            ("002.6/002.43/002.44/002.45", ["--not-a-real-flag", canon, other]),
            ("002.16/002.43/002.44/002.45", ["--max-copies", "0", canon, other]),
            ("002.17/002.43/002.44/002.45", ["--retries-copy", "0", canon, other]),
            ("002.18/002.43/002.44/002.45", ["--retries-list", "0", canon, other]),
            ("002.19/002.43/002.44/002.45", ["--timeout-conn", "0", canon, other]),
            ("002.20/002.43/002.44/002.45", ["--timeout-idle", "0", canon, other]),
            ("002.21/002.43/002.44/002.45", ["--keep-tmp-days", "0", canon, other]),
            ("002.22/002.43/002.44/002.45", ["--keep-bak-days", "0", canon, other]),
            ("002.23/002.43/002.44/002.45", ["--keep-del-days", "0", canon, other]),
            ("002.28/002.43/002.44/002.45", ["--verbosity", "loud", canon, other]),
            ("002.30/002.43/002.44/002.45", ["--dry-run", "-x", "/absolute", canon, other]),
            ("002.31/002.43/002.44/002.45", ["--dry-run", "-x", "trailing/", canon, other]),
            ("002.32/002.43/002.44/002.45", ["--dry-run", "-x", "bad\\path", canon, other]),
            ("002.33/002.43/002.44/002.45", ["--dry-run", "-x", "bad//path", canon, other]),
            ("002.34/002.43/002.44/002.45", ["--dry-run", "-x", "bad/./path", canon, other]),
            ("002.35/002.43/002.44/002.45", ["--dry-run", "-x", "bad/../path", canon, other]),
            (
                "002.39/002.43/002.44/002.45",
                ["--dry-run", f"+{left.as_uri()}?max-copies=1", right.as_uri()],
            ),
            (
                "002.40/002.43/002.44/002.45",
                ["--dry-run", f"+{left.as_uri()}?timeout-conn=0", right.as_uri()],
            ),
            (
                "002.41/002.43/002.44/002.45",
                ["--dry-run", f"+{left.as_uri()}?timeout-idle=0", right.as_uri()],
            ),
            ("002.42/002.43/002.44/002.45", ["--dry-run", canon, other, "--max-copies"]),
        ]

        for req_ids, args in cases:
            expect_validation_failure(failures, req_ids, args)


def main() -> int:
    failures: list[str] = []

    expect_help_success(failures)
    accepted_cases(failures)
    invalid_cases(failures)

    if failures:
        print("FAIL")
        for failure in failures:
            print(f"- {failure}")
        return 1

    print("PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
