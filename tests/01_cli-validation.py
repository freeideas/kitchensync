#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""CLI argument validation: invalid arguments fail before sync work."""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

PROJECT_ROOT = Path(__file__).resolve().parents[1]
TMP = PROJECT_ROOT / "tmp" / "testks" / "01_cli_validation"
PEER_A_ROOT = TMP / "peer_a"
PEER_B_ROOT = TMP / "peer_b"
PEER_A = PEER_A_ROOT.resolve().as_uri()
PEER_B = PEER_B_ROOT.resolve().as_uri()

HELP_TEXT = """Usage: java -jar kitchensync.jar [options] <peer> <peer> [<peer>...]

Synchronize file trees across multiple peers.

Running with no arguments prints this help. See README.md for full docs.

Peers:
  /path or c:\\path                 Local path (same as file://)
  sftp://user@host/path            Remote over SSH
  sftp://user@host:port/path       Non-standard SSH port
  sftp://host/path                 Remote over SSH, current OS user
  sftp://user:password@host/path   Inline password (prefer SSH keys)

Prefix modifiers:
  +<peer>                          Canon — this peer's state wins all conflicts
  -<peer>                          Subordinate — overwritten to match the group

Fallback URLs (multiple paths to the same data):
  [url1,url2,...]                  Try in order, first that connects wins
  +[url1,url2,...]                 Canon peer with fallbacks
  -[url1,url2,...]                 Subordinate peer with fallbacks

Per-URL settings (query string, inside quotes):
  "sftp://host/path?mc=5"          Max connections for this URL
  "sftp://host/path?ct=60"         Connection timeout for this URL
  "sftp://host/path?ka=10"         SFTP idle keep-alive TTL for this URL
  "sftp://host/path?mc=5&ct=60"    Combine multiple

Options:
  -h, --help, /?                      Show this help
  --mc N             Max concurrent connections per URL (default: 10)
  --ct N             SSH handshake timeout in seconds (default: 30)
  --ka N             SFTP idle keep-alive TTL in seconds (default: 30)
  -vl LEVEL          Verbosity level: error, info, debug, trace (default: info)
  --xd N             Delete stale TMP staging after N days (default: 2)
  --bd N             Delete displaced files (BAK/) after N days (default: 90)
  --td N             Forget deletion records after N days (default: 180)

Quick start:
  java -jar kitchensync.jar +c:/photos sftp://user@host/photos      First sync (c: is canon)
  java -jar kitchensync.jar c:/photos sftp://host/photos            Bidirectional
  java -jar kitchensync.jar c:/photos sftp://host/photos -/mnt/usb  Add USB as subordinate
  java -jar kitchensync.jar c:/photos "sftp://user:p%40ss@host/photos"  Inline password

Canon (+) is required on first sync when no peer has snapshot history.
After the first sync, bidirectional sync works without canon.

Tip: if ssh user@host and cd /path works, sftp://user@host/path will too.

Displaced files are recoverable from .kitchensync/BAK/ (kept for --bd days).
"""


def invoke(args: list[str], timeout: int = 15) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(UV), "run", "--script", str(BUILD_PY), "invoke-cli", PROJECT] + args,
        capture_output=True,
        text=True,
        encoding="utf-8",
        timeout=timeout,
    )


def reset_peer_roots() -> None:
    shutil.rmtree(TMP, ignore_errors=True)
    TMP.mkdir(parents=True, exist_ok=True)


def check_validation_error(
    failures: list[str],
    req_id: str,
    label: str,
    args: list[str],
    error_terms: list[str],
) -> None:
    reset_peer_roots()
    proc = invoke(args)
    print(f"[{req_id}] {label}")

    if proc.returncode != 1:
        failures.append(f"{req_id}: expected exit 1, got {proc.returncode}")

    if proc.stderr != "":
        failures.append(f"{req_id}: expected empty stderr, got {proc.stderr!r}")

    first_line, separator, help_text = proc.stdout.partition("\n")
    if not separator or first_line.strip() == "":
        failures.append(f"{req_id}: expected an error message before help text")
    elif first_line == HELP_TEXT.splitlines()[0]:
        failures.append(f"{req_id}: stdout started with help text, not a specific error")
    else:
        error_lower = first_line.lower()
        for term in error_terms:
            if term.lower() not in error_lower:
                failures.append(
                    f"{req_id}: error message {first_line!r} did not mention {term!r}"
                )

    if help_text != HELP_TEXT:
        failures.append(f"{req_id}: help text did not exactly follow the error message")

    created_roots = [str(path.relative_to(PROJECT_ROOT)) for path in (PEER_A_ROOT, PEER_B_ROOT) if path.exists()]
    if created_roots:
        failures.append(
            f"{req_id}: validation error created peer root(s): {', '.join(created_roots)}"
        )


def main() -> int:
    failures: list[str] = []

    # Zero args is the help shortcut covered by 01_help-text, so the
    # validation case for "fewer than two peers" is one peer.
    check_validation_error(failures, "01.10", "one peer", [PEER_A], ["peer"])
    check_validation_error(
        failures,
        "01.11",
        "two canon peers",
        ["+" + PEER_A, "+" + PEER_B],
        ["canon"],
    )
    check_validation_error(
        failures,
        "01.12",
        "unrecognized flag",
        ["--unknown-flag", PEER_A, PEER_B],
        ["--unknown-flag"],
    )

    for flag in ["--mc", "--ct", "--ka", "--xd", "--bd", "--td"]:
        for value in ["0", "-1", "abc"]:
            check_validation_error(
                failures,
                "01.13",
                f"{flag} {value}",
                [flag, value, PEER_A, PEER_B],
                [flag, value],
            )

    check_validation_error(
        failures,
        "01.14",
        "invalid -vl value",
        ["-vl", "verbose", PEER_A, PEER_B],
        ["-vl", "verbose"],
    )

    if failures:
        print("\nFAILURES:")
        for failure in failures:
            print(f"  - {failure}")
        return 1
    print("\nAll assertions passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
