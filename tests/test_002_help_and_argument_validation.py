# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

from __future__ import annotations

import subprocess
import sys
import tempfile
from pathlib import Path
from urllib.parse import quote


sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")


WORKSPACE_ROOT = Path("/home/ace/Desktop/prjx/kitchensync")
KITCHENSYNC_EXE = WORKSPACE_ROOT / "released" / "kitchensync.exe"
TIMEOUT_SECONDS = 20

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


class Result:
    def __init__(self, args: list[str], completed: subprocess.CompletedProcess[str] | None, error: str | None):
        self.args = args
        self.completed = completed
        self.error = error

    @property
    def returncode(self) -> int | None:
        if self.completed is None:
            return None
        return self.completed.returncode

    @property
    def stdout(self) -> str:
        if self.completed is None:
            return ""
        return self.completed.stdout

    @property
    def stderr(self) -> str:
        if self.completed is None:
            return ""
        return self.completed.stderr


def run_kitchensync(args: list[str]) -> Result:
    command = [str(KITCHENSYNC_EXE), *args]
    try:
        completed = subprocess.run(
            command,
            cwd=str(WORKSPACE_ROOT),
            text=True,
            encoding="utf-8",
            errors="replace",
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=TIMEOUT_SECONDS,
            check=False,
        )
    except subprocess.TimeoutExpired as exc:
        return Result(args, None, f"timed out after {exc.timeout} seconds")
    except OSError as exc:
        return Result(args, None, f"could not launch process: {exc}")
    return Result(args, completed, None)


def record(condition: bool, failures: list[str], message: str) -> None:
    if not condition:
        failures.append(message)


def display_args(args: list[str]) -> str:
    if not args:
        return "<no arguments>"
    return " ".join(args)


def path_arg(path: Path, prefix: str = "") -> str:
    return prefix + str(path)


def file_url(path: Path, prefix: str = "", query: str = "") -> str:
    uri = path.resolve().as_uri()
    return prefix + uri + query


def assert_no_process_error(result: Result, failures: list[str], req_id: str) -> bool:
    if result.error is None:
        return True
    failures.append(f"{req_id}: {display_args(result.args)} {result.error}")
    return False


def assert_help_invocation(failures: list[str]) -> None:
    result = run_kitchensync([])
    if not assert_no_process_error(result, failures, "002.1"):
        return
    record(
        result.stdout == EXPECTED_HELP,
        failures,
        "002.1: no-argument invocation did not print the exact help text to stdout",
    )
    record(result.returncode == 0, failures, f"002.2: expected exit 0, got {result.returncode}")
    record(result.stderr == "", failures, f"002.3: expected empty stderr, got {result.stderr!r}")


def assert_validation_failure(args: list[str], req_ids: str, failures: list[str]) -> None:
    result = run_kitchensync(args)
    if not assert_no_process_error(result, failures, req_ids):
        return
    record(
        result.returncode == 1,
        failures,
        f"{req_ids}: expected validation failure exit 1 for {display_args(args)}, got {result.returncode}",
    )
    record(
        result.stderr == "",
        failures,
        f"{req_ids}: expected empty stderr for {display_args(args)}, got {result.stderr!r}",
    )
    record(
        result.stdout.endswith(EXPECTED_HELP),
        failures,
        f"{req_ids}: validation failure did not print the help text after the error for {display_args(args)}",
    )
    record(
        result.stdout != EXPECTED_HELP,
        failures,
        f"{req_ids}: validation failure did not include an error message before help for {display_args(args)}",
    )


def assert_validation_accepts(args: list[str], req_ids: str, failures: list[str]) -> None:
    result = run_kitchensync(args)
    if not assert_no_process_error(result, failures, req_ids):
        return
    record(
        result.returncode == 0,
        failures,
        f"{req_ids}: expected accepted invocation to exit 0 for {display_args(args)}, got {result.returncode}; stdout={result.stdout!r}",
    )
    record(
        result.stderr == "",
        failures,
        f"{req_ids}: expected empty stderr for accepted invocation {display_args(args)}, got {result.stderr!r}",
    )
    record(
        not result.stdout.endswith(EXPECTED_HELP),
        failures,
        f"{req_ids}: accepted invocation printed validation help for {display_args(args)}",
    )


def accepted_base_args(left: Path, right: Path) -> list[str]:
    return ["--dry-run", path_arg(left, "+"), path_arg(right)]


def assert_common_validation_failures(left: Path, right: Path, failures: list[str]) -> None:
    assert_validation_failure([path_arg(left)], "002.4,002.46,002.47,002.48", failures)
    assert_validation_failure([path_arg(left, "+"), path_arg(right, "+")], "002.5,002.46,002.47,002.48", failures)
    assert_validation_failure(["--unknown-option", path_arg(left, "+"), path_arg(right)], "002.6,002.46,002.47,002.48", failures)


def assert_global_option_values(left: Path, right: Path, failures: list[str]) -> None:
    positive_integer_options = [
        ("--max-copies", "002.8", "002.16"),
        ("--retries-copy", "002.9", "002.17"),
        ("--retries-list", "002.10", "002.18"),
        ("--timeout-conn", "002.11", "002.19"),
        ("--timeout-idle", "002.12", "002.20"),
        ("--keep-tmp-days", "002.13", "002.21"),
        ("--keep-bak-days", "002.14", "002.22"),
        ("--keep-del-days", "002.15", "002.23"),
    ]
    assert_validation_accepts(["--dry-run", *accepted_base_args(left, right)[1:]], "002.7", failures)
    for option, accept_req, reject_req in positive_integer_options:
        assert_validation_accepts([option, "1", *accepted_base_args(left, right)], accept_req, failures)
        assert_validation_failure([option, "0", *accepted_base_args(left, right)], f"{reject_req},002.46,002.47,002.48", failures)
        assert_validation_failure([option, "abc", *accepted_base_args(left, right)], f"{reject_req},002.46,002.47,002.48", failures)
        assert_validation_failure([option, *accepted_base_args(left, right)], "002.45,002.46,002.47,002.48", failures)


def assert_verbosity_values(left: Path, right: Path, failures: list[str]) -> None:
    for level, req_id in [
        ("error", "002.24"),
        ("info", "002.25"),
        ("debug", "002.26"),
        ("trace", "002.27"),
    ]:
        assert_validation_accepts(["--verbosity", level, *accepted_base_args(left, right)], req_id, failures)
    assert_validation_failure(["--verbosity", "verbose", *accepted_base_args(left, right)], "002.28,002.46,002.47,002.48", failures)
    assert_validation_failure(["--verbosity", *accepted_base_args(left, right)], "002.45,002.46,002.47,002.48", failures)


def assert_exclude_values(left: Path, right: Path, failures: list[str]) -> None:
    assert_validation_accepts(
        ["-x", "cache", "-x", "nested/path.txt", *accepted_base_args(left, right)],
        "002.29",
        failures,
    )
    invalid_excludes = [
        ("/absolute", "002.30"),
        ("trailing/", "002.31"),
        ("bad\\path", "002.32"),
        ("bad//path", "002.33"),
        ("./path", "002.34"),
        ("path/./leaf", "002.34"),
        ("../path", "002.35"),
        ("path/../leaf", "002.35"),
    ]
    for exclude, req_id in invalid_excludes:
        assert_validation_failure(["-x", exclude, *accepted_base_args(left, right)], f"{req_id},002.46,002.47,002.48", failures)
    assert_validation_failure(["-x", *accepted_base_args(left, right)], "002.45,002.46,002.47,002.48", failures)
    # not reasonably testable: 002.36 - Python subprocess APIs reject embedded NUL
    # characters before launching the executable, so the product cannot observe one.


def assert_url_query_values(left: Path, right: Path, failures: list[str]) -> None:
    left_timeout_conn = file_url(left, "+", "?timeout-conn=1")
    left_timeout_idle = file_url(left, "+", "?timeout-idle=1")
    left_both = file_url(left, "+", "?timeout-conn=1&timeout-idle=1")
    right_url = file_url(right)
    assert_validation_accepts(["--dry-run", left_timeout_conn, right_url], "002.37,002.41", failures)
    assert_validation_accepts(["--dry-run", left_timeout_idle, right_url], "002.38,002.42", failures)
    assert_validation_accepts(["--dry-run", left_both, right_url], "002.37,002.38,002.41,002.42", failures)
    assert_validation_failure(["--dry-run", file_url(left, "+", "?max-copies=1"), right_url], "002.39,002.46,002.47,002.48", failures)
    assert_validation_failure(["--dry-run", file_url(left, "+", "?unknown=1"), right_url], "002.40,002.46,002.47,002.48", failures)
    assert_validation_failure(["--dry-run", file_url(left, "+", "?timeout-conn=0"), right_url], "002.43,002.46,002.47,002.48", failures)
    assert_validation_failure(["--dry-run", file_url(left, "+", "?timeout-conn=no"), right_url], "002.43,002.46,002.47,002.48", failures)
    assert_validation_failure(["--dry-run", file_url(left, "+", "?timeout-idle=0"), right_url], "002.44,002.46,002.47,002.48", failures)
    assert_validation_failure(["--dry-run", file_url(left, "+", "?timeout-idle=no"), right_url], "002.44,002.46,002.47,002.48", failures)


def make_peer(root: Path, name: str) -> Path:
    peer = root / quote(name, safe="")
    peer.mkdir(parents=True, exist_ok=True)
    return peer


def main() -> int:
    failures: list[str] = []
    assert_help_invocation(failures)
    with tempfile.TemporaryDirectory(prefix="kitchensync-args-") as temp_name:
        temp_root = Path(temp_name)
        left = make_peer(temp_root, "left peer")
        right = make_peer(temp_root, "right peer")
        assert_common_validation_failures(left, right, failures)
        assert_global_option_values(left, right, failures)
        assert_verbosity_values(left, right, failures)
        assert_exclude_values(left, right, failures)
        assert_url_query_values(left, right, failures)

    if failures:
        print(f"{len(failures)} failure(s):")
        for failure in failures:
            print(f"- {failure}")
        return 1
    print("test_002_help_and_argument_validation passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
